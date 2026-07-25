/// Toggle immediate application of waveform overlay preview actions.
#[cfg(not(test))]
const IMMEDIATE_WAVEFORM_PREVIEW_ENV: &str = "WAVECRATE_NATIVE_BRIDGE_IMMEDIATE_WAVEFORM_PREVIEW";
/// Default mode for immediate waveform overlay preview actions.
const IMMEDIATE_WAVEFORM_PREVIEW_DEFAULT: bool = true;
/// Cached immediate-waveform-preview mode resolved from environment.
#[cfg(not(test))]
static IMMEDIATE_WAVEFORM_PREVIEW_ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

/// Resolve whether waveform preview actions should apply immediately.
pub(in crate::app_core::ui_bridge) fn immediate_waveform_preview_enabled() -> bool {
    #[cfg(test)]
    {
        IMMEDIATE_WAVEFORM_PREVIEW_DEFAULT
    }
    #[cfg(not(test))]
    *IMMEDIATE_WAVEFORM_PREVIEW_ENABLED.get_or_init(|| {
        std::env::var(IMMEDIATE_WAVEFORM_PREVIEW_ENV)
            .ok()
            .map_or(IMMEDIATE_WAVEFORM_PREVIEW_DEFAULT, |value| {
                crate::env_flags::is_truthy(&value)
            })
    })
}
