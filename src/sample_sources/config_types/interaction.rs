use serde::{Deserialize, Serialize};

use crate::waveform::WaveformChannelView;

use super::super::config_defaults::{
    default_anti_clip_fade_ms, default_bpm_value, default_false, default_keyboard_zoom_factor,
    default_scroll_speed, default_true, default_wheel_zoom_factor,
};

/// Interaction tuning for waveform navigation.
///
/// Config keys: `invert_waveform_scroll`, `waveform_scroll_speed`,
/// `wheel_zoom_factor`, `keyboard_zoom_factor`, `anti_clip_fade_enabled`,
/// `anti_clip_fade_ms`, `destructive_yolo_mode`, `waveform_channel_view`,
/// `bpm_snap_enabled`, `bpm_value`, `transient_markers_enabled`, `transient_snap_enabled`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionOptions {
    #[serde(default = "default_true")]
    pub invert_waveform_scroll: bool,
    #[serde(default = "default_scroll_speed")]
    pub waveform_scroll_speed: f32,
    #[serde(default = "default_wheel_zoom_factor")]
    pub wheel_zoom_factor: f32,
    #[serde(default = "default_keyboard_zoom_factor")]
    pub keyboard_zoom_factor: f32,
    #[serde(default = "default_true")]
    pub anti_clip_fade_enabled: bool,
    #[serde(default = "default_anti_clip_fade_ms")]
    pub anti_clip_fade_ms: f32,
    #[serde(default)]
    pub destructive_yolo_mode: bool,
    #[serde(default)]
    pub waveform_channel_view: WaveformChannelView,
    #[serde(default = "default_false")]
    pub bpm_snap_enabled: bool,
    #[serde(default = "default_bpm_value")]
    pub bpm_value: f32,
    #[serde(default = "default_false")]
    pub transient_snap_enabled: bool,
    #[serde(default = "default_true")]
    pub transient_markers_enabled: bool,
}

impl Default for InteractionOptions {
    fn default() -> Self {
        Self {
            invert_waveform_scroll: true,
            waveform_scroll_speed: default_scroll_speed(),
            wheel_zoom_factor: default_wheel_zoom_factor(),
            keyboard_zoom_factor: default_keyboard_zoom_factor(),
            anti_clip_fade_enabled: true,
            anti_clip_fade_ms: default_anti_clip_fade_ms(),
            destructive_yolo_mode: false,
            waveform_channel_view: WaveformChannelView::Mono,
            bpm_snap_enabled: default_false(),
            bpm_value: default_bpm_value(),
            transient_snap_enabled: default_false(),
            transient_markers_enabled: default_true(),
        }
    }
}
