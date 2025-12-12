//! Shared state types for the egui UI.
//! Temporary while the egui UI is being wired; types will be exercised by the renderer next.

mod audio;
mod browser;
mod collections;
mod controls;
mod drag;
mod focus;
mod hotkeys;
mod progress;
mod sources;
mod status;
mod waveform;

pub use audio::*;
pub use browser::*;
pub use collections::*;
pub use controls::*;
pub use drag::*;
pub use focus::*;
pub use hotkeys::*;
pub use progress::*;
pub use sources::*;
pub use status::*;
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
    /// Audio device/options UI state.
    pub audio: AudioOptionsState,
    /// Interaction and navigation tuning options.
    pub controls: InteractionOptionsState,
    /// Master output volume (0.0-1.0).
    pub volume: f32,
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
            audio: AudioOptionsState::default(),
            controls: InteractionOptionsState::default(),
            volume: 1.0,
            loaded_wav: None,
            trash_folder: None,
            collection_export_root: None,
        }
    }
}
