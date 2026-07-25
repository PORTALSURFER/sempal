//! Process-lifetime bridge metrics registry and feature-flag state.

#[cfg(all(feature = "native-bridge-metrics", not(test)))]
use std::sync::OnceLock;
#[cfg(feature = "native-bridge-metrics")]
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

#[cfg(feature = "native-bridge-metrics")]
pub(crate) const BRIDGE_PROFILE_INTERVAL: u64 = 240;
#[cfg(not(feature = "native-bridge-metrics"))]
pub(crate) const BRIDGE_PROFILE_INTERVAL: u64 = 1;

#[cfg(all(feature = "native-bridge-metrics", not(test)))]
const BRIDGE_PROFILE_ENV: &str = "WAVECRATE_NATIVE_BRIDGE_PROFILE";
#[cfg(all(feature = "native-bridge-metrics", not(test)))]
/// Enable runtime validation that cached projection-key snapshots stay in sync.
const PROJECTION_KEY_ASSERT_ENV: &str = "WAVECRATE_NATIVE_BRIDGE_ASSERT_PROJECTION_SNAPSHOT";

#[cfg(feature = "native-bridge-metrics")]
/// Total number of projection-cache lookups that reused a cached model.
pub(crate) static PROJECTION_CACHE_HIT_COUNT: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "native-bridge-metrics")]
/// Total number of projection-cache lookups that required a fresh projection.
pub(crate) static PROJECTION_CACHE_MISS_COUNT: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "native-bridge-metrics")]
/// Total number of waveform image refresh requests applied during derived flush.
pub(crate) static WAVEFORM_IMAGE_REFRESH_APPLY_COUNT: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "native-bridge-metrics")]
/// Total number of waveform image refresh requests skipped as overlay-only.
pub(crate) static WAVEFORM_IMAGE_REFRESH_SKIP_COUNT: AtomicU64 = AtomicU64::new(0);

#[cfg(feature = "native-bridge-metrics")]
/// Grouped bridge-metrics counters to reduce registry drift and edit overhead.
pub(crate) struct BridgeMetrics {
    pub(super) pull_model_count: AtomicU64,
    pub(super) pull_model_prep_ns: AtomicU64,
    pub(super) pull_model_project_ns: AtomicU64,
    pub(super) pull_motion_count: AtomicU64,
    pub(super) pull_motion_prep_ns: AtomicU64,
    pub(super) pull_motion_project_ns: AtomicU64,
    pub(super) action_count: AtomicU64,
    pub(super) action_duration_ns: AtomicU64,
    pub(super) projection_status_segment_hit_count: AtomicU64,
    pub(super) projection_status_segment_miss_count: AtomicU64,
    pub(super) projection_browser_frame_segment_hit_count: AtomicU64,
    pub(super) projection_browser_frame_segment_miss_count: AtomicU64,
    pub(super) projection_browser_tag_sidebar_segment_hit_count: AtomicU64,
    pub(super) projection_browser_tag_sidebar_segment_miss_count: AtomicU64,
    pub(super) projection_browser_rows_segment_hit_count: AtomicU64,
    pub(super) projection_browser_rows_segment_miss_count: AtomicU64,
    pub(super) projection_map_segment_hit_count: AtomicU64,
    pub(super) projection_map_segment_miss_count: AtomicU64,
    pub(super) projection_waveform_segment_hit_count: AtomicU64,
    pub(super) projection_waveform_segment_miss_count: AtomicU64,
    pub(super) action_wheel_count: AtomicU64,
    pub(super) action_wheel_duration_ns: AtomicU64,
    pub(super) action_map_proxy_count: AtomicU64,
    pub(super) action_map_proxy_duration_ns: AtomicU64,
    pub(super) action_waveform_count: AtomicU64,
    pub(super) action_waveform_duration_ns: AtomicU64,
    pub(super) action_volume_count: AtomicU64,
    pub(super) action_volume_duration_ns: AtomicU64,
    pub(super) waveform_flush_count: AtomicU64,
    pub(super) waveform_flush_duration_ns: AtomicU64,
    pub(super) waveform_flush_emitted_actions_total: AtomicU64,
    pub(super) derived_flush_count: AtomicU64,
    pub(super) derived_flush_duration_ns: AtomicU64,
    pub(super) derived_dirty_source_total: AtomicU64,
    pub(super) derived_dirty_computed_total: AtomicU64,
    pub(super) frame_result_count: AtomicU64,
    pub(super) frame_result_animation_count: AtomicU64,
    pub(super) frame_result_primitives_total: AtomicU64,
    pub(super) frame_result_text_runs_total: AtomicU64,
    pub(super) frame_result_layout_rebuild_count: AtomicU64,
    pub(super) frame_result_static_rebuild_count: AtomicU64,
    pub(super) frame_result_state_overlay_rebuild_count: AtomicU64,
    pub(super) frame_result_motion_overlay_rebuild_count: AtomicU64,
    pub(super) frame_result_overlay_only_count: AtomicU64,
    pub(super) frame_result_presented_count: AtomicU64,
    pub(super) frame_result_missed_present_count: AtomicU64,
    pub(super) frame_result_jank_count: AtomicU64,
    pub(super) frame_result_total_us: AtomicU64,
    pub(super) frame_result_present_us_total: AtomicU64,
    pub(super) frame_result_frame_budget_us: AtomicU64,
    pub(super) projection_key_assert_count: AtomicU64,
    pub(super) projection_key_assert_stale_count: AtomicU64,
}

#[cfg(feature = "native-bridge-metrics")]
impl BridgeMetrics {
    /// Build a zeroed bridge-metrics registry for process-lifetime accumulation.
    pub(crate) const fn new() -> Self {
        Self {
            pull_model_count: AtomicU64::new(0),
            pull_model_prep_ns: AtomicU64::new(0),
            pull_model_project_ns: AtomicU64::new(0),
            pull_motion_count: AtomicU64::new(0),
            pull_motion_prep_ns: AtomicU64::new(0),
            pull_motion_project_ns: AtomicU64::new(0),
            action_count: AtomicU64::new(0),
            action_duration_ns: AtomicU64::new(0),
            projection_status_segment_hit_count: AtomicU64::new(0),
            projection_status_segment_miss_count: AtomicU64::new(0),
            projection_browser_frame_segment_hit_count: AtomicU64::new(0),
            projection_browser_frame_segment_miss_count: AtomicU64::new(0),
            projection_browser_tag_sidebar_segment_hit_count: AtomicU64::new(0),
            projection_browser_tag_sidebar_segment_miss_count: AtomicU64::new(0),
            projection_browser_rows_segment_hit_count: AtomicU64::new(0),
            projection_browser_rows_segment_miss_count: AtomicU64::new(0),
            projection_map_segment_hit_count: AtomicU64::new(0),
            projection_map_segment_miss_count: AtomicU64::new(0),
            projection_waveform_segment_hit_count: AtomicU64::new(0),
            projection_waveform_segment_miss_count: AtomicU64::new(0),
            action_wheel_count: AtomicU64::new(0),
            action_wheel_duration_ns: AtomicU64::new(0),
            action_map_proxy_count: AtomicU64::new(0),
            action_map_proxy_duration_ns: AtomicU64::new(0),
            action_waveform_count: AtomicU64::new(0),
            action_waveform_duration_ns: AtomicU64::new(0),
            action_volume_count: AtomicU64::new(0),
            action_volume_duration_ns: AtomicU64::new(0),
            waveform_flush_count: AtomicU64::new(0),
            waveform_flush_duration_ns: AtomicU64::new(0),
            waveform_flush_emitted_actions_total: AtomicU64::new(0),
            derived_flush_count: AtomicU64::new(0),
            derived_flush_duration_ns: AtomicU64::new(0),
            derived_dirty_source_total: AtomicU64::new(0),
            derived_dirty_computed_total: AtomicU64::new(0),
            frame_result_count: AtomicU64::new(0),
            frame_result_animation_count: AtomicU64::new(0),
            frame_result_primitives_total: AtomicU64::new(0),
            frame_result_text_runs_total: AtomicU64::new(0),
            frame_result_layout_rebuild_count: AtomicU64::new(0),
            frame_result_static_rebuild_count: AtomicU64::new(0),
            frame_result_state_overlay_rebuild_count: AtomicU64::new(0),
            frame_result_motion_overlay_rebuild_count: AtomicU64::new(0),
            frame_result_overlay_only_count: AtomicU64::new(0),
            frame_result_presented_count: AtomicU64::new(0),
            frame_result_missed_present_count: AtomicU64::new(0),
            frame_result_jank_count: AtomicU64::new(0),
            frame_result_total_us: AtomicU64::new(0),
            frame_result_present_us_total: AtomicU64::new(0),
            frame_result_frame_budget_us: AtomicU64::new(0),
            projection_key_assert_count: AtomicU64::new(0),
            projection_key_assert_stale_count: AtomicU64::new(0),
        }
    }
}

#[cfg(feature = "native-bridge-metrics")]
/// Process-lifetime grouped counter registry used by bridge profiling hooks.
pub(crate) static BRIDGE_METRICS: BridgeMetrics = BridgeMetrics::new();
#[cfg(all(feature = "native-bridge-metrics", not(test)))]
static BRIDGE_PROFILE_ENABLED: OnceLock<bool> = OnceLock::new();
#[cfg(all(feature = "native-bridge-metrics", not(test)))]
/// Cached projection-snapshot assertion mode resolved from environment.
static PROJECTION_KEY_ASSERT_ENABLED: OnceLock<bool> = OnceLock::new();

#[cfg(all(feature = "native-bridge-metrics", not(test)))]
pub(crate) fn bridge_profiling_enabled() -> bool {
    *BRIDGE_PROFILE_ENABLED.get_or_init(|| crate::env_flags::env_var_truthy(BRIDGE_PROFILE_ENV))
}
#[cfg(any(not(feature = "native-bridge-metrics"), test))]
#[inline]
pub(crate) fn bridge_profiling_enabled() -> bool {
    false
}

#[cfg(all(feature = "native-bridge-metrics", not(test)))]
/// Resolve whether projection-key snapshot assertions should run.
pub(crate) fn projection_key_assertions_enabled() -> bool {
    *PROJECTION_KEY_ASSERT_ENABLED
        .get_or_init(|| crate::env_flags::env_var_truthy(PROJECTION_KEY_ASSERT_ENV))
}
#[cfg(any(not(feature = "native-bridge-metrics"), test))]
#[inline]
/// Disable projection-key assertions when bridge metrics are compiled out.
pub(crate) fn projection_key_assertions_enabled() -> bool {
    false
}

#[cfg(feature = "native-bridge-metrics")]
pub(crate) fn saturating_add_duration(counter: &AtomicU64, duration: Duration) {
    let dur_ns = duration.as_nanos().min(u64::MAX as u128) as u64;
    counter.fetch_add(dur_ns, Ordering::Relaxed);
}
