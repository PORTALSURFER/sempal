//! Shared state types for the egui UI.
//! Temporary while the egui UI is being wired; types will be exercised by the renderer next.

mod audio;
mod browser;
mod collections;
mod controls;
mod drag;
mod feedback_issue;
mod focus;
mod hotkeys;
mod loop_crossfade;
mod map;
mod progress;
mod sources;
mod status;
mod update;
mod waveform;

pub use audio::*;
pub use browser::*;
pub use collections::*;
pub use controls::*;
pub use drag::*;
pub use feedback_issue::*;
pub use focus::*;
pub use hotkeys::*;
pub use loop_crossfade::*;
pub use map::*;
pub use progress::*;
pub use sources::*;
pub use status::*;
pub use update::*;
pub use waveform::*;

use std::path::PathBuf;

/// Top-level UI model consumed by the egui renderer.
#[derive(Clone, Debug)]
pub struct UiState {
    /// Status bar and status log state.
    pub status: StatusBarState,
    /// Source list and folder browser state.
    pub sources: SourcePanelState,
    /// Sample browser state.
    pub browser: SampleBrowserState,
    /// Waveform rendering and selection state.
    pub waveform: WaveformState,
    /// Drag-and-drop state.
    pub drag: DragState,
    /// Collections panel state.
    pub collections: CollectionsState,
    /// Overlay for long-running tasks.
    pub progress: ProgressOverlayState,
    /// Tracks which UI region currently owns keyboard focus.
    pub focus: UiFocusState,
    /// UI state for contextual hotkey affordances.
    pub hotkeys: HotkeyUiState,
    /// Feedback prompt state for filing GitHub issues.
    pub feedback_issue: FeedbackIssueUiState,
    /// Audio device/options UI state.
    pub audio: AudioOptionsState,
    /// 2D map explorer state.
    pub map: MapUiState,
    /// Interaction and navigation tuning options.
    pub controls: InteractionOptionsState,
    /// Pending loop crossfade prompt state.
    pub loop_crossfade_prompt: Option<LoopCrossfadePrompt>,
    /// Master output volume (0.0-1.0).
    pub volume: f32,
    /// Release update status / notification state.
    pub update: UpdateUiState,
    /// Currently loaded wav path, if any.
    pub loaded_wav: Option<PathBuf>,
    /// Optional trash folder path configured by the user.
    pub trash_folder: Option<PathBuf>,
    /// Optional global export root used for automatic collection exports.
    pub collection_export_root: Option<PathBuf>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            status: StatusBarState::idle(),
            sources: SourcePanelState::default(),
            browser: SampleBrowserState::default(),
            waveform: WaveformState::default(),
            drag: DragState::default(),
            collections: CollectionsState::default(),
            progress: ProgressOverlayState::default(),
            focus: UiFocusState::default(),
            hotkeys: HotkeyUiState::default(),
            feedback_issue: FeedbackIssueUiState::default(),
            audio: AudioOptionsState::default(),
            map: MapUiState::default(),
            controls: InteractionOptionsState::default(),
            loop_crossfade_prompt: None,
            volume: 1.0,
            update: UpdateUiState::default(),
            loaded_wav: None,
            trash_folder: None,
            collection_export_root: None,
        }
    }
}
