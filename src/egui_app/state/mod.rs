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
mod progress;
mod sources;
mod status;
mod tf_labels;
mod training;
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
pub use progress::*;
pub use sources::*;
pub use status::*;
pub use tf_labels::*;
pub use training::*;
pub use update::*;
pub use waveform::*;

use std::path::PathBuf;

/// Top-level UI model consumed by the egui renderer.
#[derive(Clone, Debug)]
pub struct UiState {
    pub status: StatusBarState,
    pub sources: SourcePanelState,
    pub browser: SampleBrowserState,
    pub waveform: WaveformState,
    pub drag: DragState,
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
    /// Model training and weak labeling controls.
    pub training: TrainingUiState,
    /// Training-free label UI state.
    pub tf_labels: TfLabelsUiState,
    /// Interaction and navigation tuning options.
    pub controls: InteractionOptionsState,
    /// Master output volume (0.0-1.0).
    pub volume: f32,
    /// Release update status / notification state.
    pub update: UpdateUiState,
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
            training: TrainingUiState::default(),
            tf_labels: TfLabelsUiState::default(),
            controls: InteractionOptionsState::default(),
            volume: 1.0,
            update: UpdateUiState::default(),
            loaded_wav: None,
            trash_folder: None,
            collection_export_root: None,
        }
    }
}
