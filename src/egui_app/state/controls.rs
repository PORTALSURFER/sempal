use crate::waveform::WaveformChannelView;

/// Interaction tuning surfaced in the UI.
#[derive(Clone, Debug)]
pub struct InteractionOptionsState {
    pub invert_waveform_scroll: bool,
    pub waveform_scroll_speed: f32,
    pub wheel_zoom_factor: f32,
    pub keyboard_zoom_factor: f32,
    pub anti_clip_fade_enabled: bool,
    pub anti_clip_fade_ms: f32,
    pub destructive_yolo_mode: bool,
    pub waveform_channel_view: WaveformChannelView,
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
        }
    }
}

/// Destructive selection edits that overwrite audio on disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DestructiveSelectionEdit {
    CropSelection,
    TrimSelection,
    ReverseSelection,
    FadeLeftToRight,
    FadeRightToLeft,
    MuteSelection,
    NormalizeSelection,
    ClickRemoval,
}

/// Confirmation prompt content for destructive edits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DestructiveEditPrompt {
    pub edit: DestructiveSelectionEdit,
    pub title: String,
    pub message: String,
}
