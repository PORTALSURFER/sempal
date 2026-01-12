use crate::waveform::WaveformChannelView;

/// Interaction tuning surfaced in the UI.
#[derive(Clone, Debug)]
pub struct InteractionOptionsState {
    /// Invert waveform scroll direction.
    pub invert_waveform_scroll: bool,
    /// Scroll speed multiplier for waveform navigation.
    pub waveform_scroll_speed: f32,
    /// Wheel zoom factor used by the UI.
    pub wheel_zoom_factor: f32,
    /// Keyboard zoom factor used by the UI.
    pub keyboard_zoom_factor: f32,
    /// Whether anti-clip fades are enabled.
    pub anti_clip_fade_enabled: bool,
    /// Anti-clip fade duration in milliseconds.
    pub anti_clip_fade_ms: f32,
    /// Allow destructive edits without confirmation.
    pub destructive_yolo_mode: bool,
    /// Default waveform channel view.
    pub waveform_channel_view: WaveformChannelView,
    /// Whether input monitoring is enabled.
    pub input_monitoring_enabled: bool,
    /// Advance selection after rating a sample.
    pub advance_after_rating: bool,
}

impl Default for InteractionOptionsState {
    fn default() -> Self {
        Self {
            invert_waveform_scroll: true,
            waveform_scroll_speed: 1.2,
            wheel_zoom_factor: 0.96,
            keyboard_zoom_factor: 0.9,
            anti_clip_fade_enabled: true,
            anti_clip_fade_ms: 2.0,
            destructive_yolo_mode: false,
            waveform_channel_view: WaveformChannelView::Mono,
            input_monitoring_enabled: true,
            advance_after_rating: true,
        }
    }
}

/// Destructive selection edits that overwrite audio on disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DestructiveSelectionEdit {
    /// Crop the selection and discard the rest.
    CropSelection,
    /// Trim everything outside the selection.
    TrimSelection,
    /// Reverse the selected audio.
    ReverseSelection,
    /// Apply a left-to-right fade.
    FadeLeftToRight,
    /// Apply a right-to-left fade.
    FadeRightToLeft,
    /// Apply short fade-in/out ramps at the selection edges to reduce clicks.
    ShortEdgeFades,
    /// Mute the selection.
    MuteSelection,
    /// Normalize the selection.
    NormalizeSelection,
    /// Attempt to remove clicks in the selection.
    ClickRemoval,
}

/// Confirmation prompt content for destructive edits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DestructiveEditPrompt {
    /// Edit type that will be applied.
    pub edit: DestructiveSelectionEdit,
    /// Prompt title text.
    pub title: String,
    /// Prompt body text.
    pub message: String,
}
