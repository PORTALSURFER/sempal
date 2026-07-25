//! Shared timestamp conversion contracts for persisted Wavecrate metadata.

use std::time::{SystemTime, UNIX_EPOCH};

const NANOS_PER_SECOND: u128 = 1_000_000_000;
const MIN_NANOS_MAGNITUDE: u128 = i64::MAX as u128 + 1;

/// Convert a [`SystemTime`] to signed Unix nanoseconds for Wavecrate's `i64` fields.
///
/// The conversion is exact for every timestamp representable by `i64`, including
/// pre-epoch values. Timestamps outside that range saturate to `i64::MIN` or
/// `i64::MAX`; they never wrap, fail solely because they precede the epoch, or
/// mutate the filesystem. The returned value is a metadata timestamp only and
/// must not be used as content identity.
pub fn system_time_to_unix_nanos(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => {
            let nanos = duration_nanos(duration);
            nanos.min(i64::MAX as u128) as i64
        }
        Err(error) => {
            let nanos = duration_nanos(error.duration());
            if nanos >= MIN_NANOS_MAGNITUDE {
                i64::MIN
            } else {
                -(nanos as i64)
            }
        }
    }
}

fn duration_nanos(duration: std::time::Duration) -> u128 {
    u128::from(duration.as_secs())
        .checked_mul(NANOS_PER_SECOND)
        .and_then(|seconds| seconds.checked_add(u128::from(duration.subsec_nanos())))
        .unwrap_or(u128::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn preserves_epoch_and_signed_nanoseconds() {
        assert_eq!(system_time_to_unix_nanos(UNIX_EPOCH), 0);
        assert_eq!(
            system_time_to_unix_nanos(UNIX_EPOCH - Duration::from_nanos(1)),
            -1
        );
        assert_eq!(
            system_time_to_unix_nanos(UNIX_EPOCH + Duration::from_nanos(1)),
            1
        );
    }

    #[test]
    fn preserves_exact_signed_i64_boundaries() {
        let max = UNIX_EPOCH + Duration::from_nanos(i64::MAX as u64);
        let min = UNIX_EPOCH - Duration::from_nanos(i64::MAX as u64 + 1);

        assert_eq!(system_time_to_unix_nanos(max), i64::MAX);
        assert_eq!(system_time_to_unix_nanos(min), i64::MIN);
    }

    #[test]
    fn saturates_overflow_without_wrapping() {
        let above_max = UNIX_EPOCH + Duration::from_nanos(i64::MAX as u64 + 1);
        let below_min = UNIX_EPOCH - Duration::from_nanos(i64::MAX as u64 + 2);

        assert_eq!(system_time_to_unix_nanos(above_max), i64::MAX);
        assert_eq!(system_time_to_unix_nanos(below_min), i64::MIN);
    }

    #[test]
    fn preserves_order_across_epoch() {
        let before = system_time_to_unix_nanos(UNIX_EPOCH - Duration::from_nanos(1));
        let epoch = system_time_to_unix_nanos(UNIX_EPOCH);
        let after = system_time_to_unix_nanos(UNIX_EPOCH + Duration::from_nanos(1));

        assert!(before < epoch);
        assert!(epoch < after);
        assert_eq!(before + 1, epoch);
        assert_eq!(epoch + 1, after);
    }
}
