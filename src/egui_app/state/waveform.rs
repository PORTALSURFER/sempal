use super::controls::DestructiveEditPrompt;
use crate::selection::SelectionRange;
use crate::waveform::WaveformChannelView;
use egui;
use std::collections::VecDeque;
use std::path::PathBuf;

/// Cached waveform image and playback overlays.
#[derive(Clone, Debug)]
pub struct WaveformState {
    pub image: Option<WaveformImage>,
    pub playhead: PlayheadState,
    /// Last play start position chosen by the user (normalized 0-1).
    pub last_start_marker: Option<f32>,
    /// Persistent navigation cursor (normalized 0-1) used by keyboard navigation.
    pub cursor: Option<f32>,
    pub selection: Option<SelectionRange>,
    pub selection_duration: Option<String>,
    pub hover_time_label: Option<String>,
    pub channel_view: WaveformChannelView,
    /// Current visible viewport within the waveform (0.0-1.0 normalized).
    pub view: WaveformView,
    pub loop_enabled: bool,
    pub notice: Option<String>,
    /// Optional path for the sample currently loading to drive UI affordances.
    pub loading: Option<PathBuf>,
    /// Pending confirmation dialog for destructive edits.
    pub pending_destructive: Option<DestructiveEditPrompt>,
    /// Last moment the waveform cursor was moved via mouse hover.
    pub cursor_last_hover_at: Option<std::time::Instant>,
    /// Last moment the waveform cursor was moved via keyboard/navigation.
    pub cursor_last_navigation_at: Option<std::time::Instant>,
    /// Last pointer position seen over the waveform (screen space).
    pub hover_pointer_pos: Option<egui::Pos2>,
    /// Timestamp of the last time the pointer moved over the waveform.
    pub hover_pointer_last_moved_at: Option<std::time::Instant>,
}

impl Default for WaveformState {
    fn default() -> Self {
        Self {
            image: None,
            playhead: PlayheadState::default(),
            last_start_marker: None,
            cursor: None,
            selection: None,
            selection_duration: None,
            hover_time_label: None,
            channel_view: WaveformChannelView::Mono,
            view: WaveformView::default(),
            loop_enabled: false,
            notice: None,
            loading: None,
            pending_destructive: None,
            cursor_last_hover_at: None,
            cursor_last_navigation_at: None,
            hover_pointer_pos: None,
            hover_pointer_last_moved_at: None,
        }
    }
}

/// Raw pixels ready to upload to an egui texture.
#[derive(Clone, Debug)]
pub struct WaveformImage {
    pub image: egui::ColorImage,
    pub view_start: f32,
    pub view_end: f32,
}

/// Normalized bounds describing the visible region of the waveform.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaveformView {
    pub start: f32,
    pub end: f32,
}

impl WaveformView {
    /// Clamp the view to a valid range while keeping the width positive.
    pub fn clamp(mut self) -> Self {
        let width = (self.end - self.start).clamp(0.001, 1.0);
        let start = self.start.clamp(0.0, 1.0 - width);
        self.start = start;
        self.end = (start + width).min(1.0);
        self
    }

    /// Width of the viewport.
    pub fn width(&self) -> f32 {
        (self.end - self.start).max(0.001)
    }
}

impl Default for WaveformView {
    fn default() -> Self {
        Self {
            start: 0.0,
            end: 1.0,
        }
    }
}

/// Current playhead position/visibility.
#[derive(Clone, Debug)]
pub struct PlayheadState {
    pub position: f32,
    pub visible: bool,
    /// Normalized end of the currently playing span, when any.
    pub active_span_end: Option<f32>,
    /// Recent playhead positions used to render a fading trail while playing.
    pub trail: VecDeque<PlayheadTrailSample>,
    /// Previous trails that are fading out after a discontinuity (seek/loop/stop).
    pub fading_trails: Vec<FadingPlayheadTrail>,
}

impl Default for PlayheadState {
    fn default() -> Self {
        Self {
            position: 0.0,
            visible: false,
            active_span_end: None,
            trail: VecDeque::new(),
            fading_trails: Vec::new(),
        }
    }
}

/// Cached samples for a playhead trail that is fading out.
#[derive(Clone, Debug)]
pub struct FadingPlayheadTrail {
    pub started_at: f64,
    pub samples: VecDeque<PlayheadTrailSample>,
}

/// Single playhead position sample used for rendering a fading trail.
#[derive(Clone, Copy, Debug)]
pub struct PlayheadTrailSample {
    /// Normalized playhead position (0.0-1.0).
    pub position: f32,
    /// Timestamp from `egui::InputState::time` when captured.
    pub time: f64,
}
