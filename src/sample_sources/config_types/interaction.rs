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
/// `bpm_snap_enabled`, `bpm_lock_enabled`, `bpm_stretch_enabled`, `bpm_value`,
/// `transient_markers_enabled`, `transient_snap_enabled`, `input_monitoring_enabled`,
/// `normalized_audition_enabled`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionOptions {
    /// Invert mouse wheel direction for waveform scrolling.
    #[serde(default = "default_true")]
    pub invert_waveform_scroll: bool,
    /// Scroll speed multiplier for waveform navigation.
    #[serde(default = "default_scroll_speed")]
    pub waveform_scroll_speed: f32,
    /// Zoom factor for mouse wheel zoom.
    #[serde(default = "default_wheel_zoom_factor")]
    pub wheel_zoom_factor: f32,
    /// Zoom factor for keyboard shortcuts.
    #[serde(default = "default_keyboard_zoom_factor")]
    pub keyboard_zoom_factor: f32,
    /// Whether to apply anti-clip fades at playback edges.
    #[serde(default = "default_true")]
    pub anti_clip_fade_enabled: bool,
    /// Anti-clip fade duration in milliseconds.
    #[serde(default = "default_anti_clip_fade_ms")]
    pub anti_clip_fade_ms: f32,
    /// Allow destructive edits without confirmation.
    #[serde(default)]
    pub destructive_yolo_mode: bool,
    /// Default waveform channel visualization mode.
    #[serde(default)]
    pub waveform_channel_view: WaveformChannelView,
    /// Enable BPM snapping for selections and cursor moves.
    #[serde(default = "default_false")]
    pub bpm_snap_enabled: bool,
    /// Lock BPM input to the detected value.
    #[serde(default = "default_false")]
    pub bpm_lock_enabled: bool,
    /// Enable BPM-based time stretching for playback.
    #[serde(default = "default_false")]
    pub bpm_stretch_enabled: bool,
    /// BPM value used for snapping and stretching.
    #[serde(default = "default_bpm_value")]
    pub bpm_value: f32,
    /// Snap selections to detected transient markers.
    #[serde(default = "default_false")]
    pub transient_snap_enabled: bool,
    /// Render transient markers in the waveform UI.
    #[serde(default = "default_true")]
    pub transient_markers_enabled: bool,
    /// Enable live input monitoring during recording.
    #[serde(default = "default_true")]
    pub input_monitoring_enabled: bool,
    /// Normalize audition playback levels.
    #[serde(default = "default_false")]
    pub normalized_audition_enabled: bool,
    /// Advance selection after rating a sample.
    #[serde(default = "default_true")]
    pub advance_after_rating: bool,
    /// Show hover hints in the UI.
    #[serde(default = "default_true")]
    pub hover_hints_enabled: bool,
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
            bpm_lock_enabled: default_false(),
            bpm_stretch_enabled: default_false(),
            bpm_value: default_bpm_value(),
            transient_snap_enabled: default_false(),
            transient_markers_enabled: default_true(),
            input_monitoring_enabled: default_true(),
            normalized_audition_enabled: default_false(),
            advance_after_rating: true,
            hover_hints_enabled: true,
        }
    }
}
