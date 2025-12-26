use crate::audio::AudioOutputConfig;

pub(super) const MAX_ANALYSIS_WORKER_COUNT: u32 = 64;

pub(super) fn clamp_volume(volume: f32) -> f32 {
    volume.clamp(0.0, 1.0)
}

pub(super) fn clamp_analysis_worker_count(value: u32) -> u32 {
    value.min(MAX_ANALYSIS_WORKER_COUNT)
}

pub(super) fn default_true() -> bool {
    true
}

pub(super) fn default_audio_output() -> AudioOutputConfig {
    AudioOutputConfig::default()
}

pub(super) fn default_max_analysis_duration_seconds() -> f32 {
    30.0
}

pub(super) fn default_analysis_worker_count() -> u32 {
    0
}

pub(super) fn default_false() -> bool {
    false
}

pub(super) fn default_fast_similarity_prep_sample_rate() -> u32 {
    8_000
}

pub(super) fn default_volume() -> f32 {
    1.0
}

pub(super) fn default_scroll_speed() -> f32 {
    1.2
}

pub(super) fn default_wheel_zoom_factor() -> f32 {
    0.96
}

pub(super) fn default_keyboard_zoom_factor() -> f32 {
    0.9
}

pub(super) fn default_anti_clip_fade_ms() -> f32 {
    2.0
}

pub(super) fn default_bpm_value() -> f32 {
    142.0
}
