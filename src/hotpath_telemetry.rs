//! Shared helpers for lightweight hot-path telemetry instrumentation.
//!
//! These helpers intentionally stay small and explicit so perf-sensitive call
//! sites can reuse boilerplate without hiding metric semantics.

use std::sync::{
    OnceLock,
    atomic::{AtomicU64, Ordering},
};
use std::time::Duration;

/// Environment variable used to toggle hot-path telemetry snapshots.
pub(crate) const HOTPATH_TELEMETRY_ENV: &str = "WAVECRATE_HOTPATH_TELEMETRY";

/// Resolve hot-path telemetry mode once and cache it in production.
#[cfg(not(test))]
pub(crate) fn enabled(state: &OnceLock<bool>) -> bool {
    *state.get_or_init(|| crate::env_flags::env_var_truthy(HOTPATH_TELEMETRY_ENV))
}

/// Keep telemetry disabled in unit tests so counters and environment changes
/// cannot couple otherwise unrelated cases.
#[cfg(test)]
pub(crate) fn enabled(_state: &OnceLock<bool>) -> bool {
    false
}

/// Add a duration in nanoseconds with saturating conversion to `u64`.
pub(crate) fn add_duration_ns(counter: &AtomicU64, duration: Duration) {
    let dur_ns = duration.as_nanos().min(u64::MAX as u128) as u64;
    counter.fetch_add(dur_ns, Ordering::Relaxed);
}

/// Add a byte count to a `u64` counter with saturating `usize` conversion.
#[cfg(any(test, feature = "legacy-controller"))]
pub(crate) fn add_bytes(counter: &AtomicU64, bytes: usize) {
    counter.fetch_add(bytes.min(u64::MAX as usize) as u64, Ordering::Relaxed);
}

/// Store resident-byte counters and update peak resident bytes.
pub(crate) fn store_resident_and_peak(
    resident_counter: &AtomicU64,
    peak_counter: &AtomicU64,
    resident_bytes: usize,
) {
    let resident = resident_bytes.min(u64::MAX as usize) as u64;
    resident_counter.store(resident, Ordering::Relaxed);
    peak_counter.fetch_max(resident, Ordering::Relaxed);
}

/// Return whether a periodic telemetry snapshot should emit on `sample_tick`.
pub(crate) fn should_emit(sample_tick: u64, every: u64) -> bool {
    sample_tick != 0 && sample_tick.is_multiple_of(every)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_duration_ns_saturates_large_durations() {
        let counter = AtomicU64::new(0);
        add_duration_ns(&counter, Duration::from_secs(u64::MAX));
        assert_eq!(counter.load(Ordering::Relaxed), u64::MAX);
    }

    #[test]
    fn add_bytes_saturates_usize_inputs() {
        let counter = AtomicU64::new(0);
        add_bytes(&counter, usize::MAX);
        assert_eq!(
            counter.load(Ordering::Relaxed),
            (usize::MAX).min(u64::MAX as usize) as u64
        );
    }

    #[test]
    fn store_resident_and_peak_tracks_monotonic_peak() {
        let resident = AtomicU64::new(0);
        let peak = AtomicU64::new(0);
        store_resident_and_peak(&resident, &peak, 1024);
        store_resident_and_peak(&resident, &peak, 128);
        assert_eq!(resident.load(Ordering::Relaxed), 128);
        assert_eq!(peak.load(Ordering::Relaxed), 1024);
    }

    #[test]
    fn should_emit_requires_non_zero_multiple() {
        assert!(!should_emit(0, 128));
        assert!(!should_emit(127, 128));
        assert!(should_emit(128, 128));
        assert!(should_emit(256, 128));
    }
}
