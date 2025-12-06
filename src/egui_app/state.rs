use crate::sample_sources::{CollectionId, SampleTag, SourceId};
use crate::selection::SelectionRange;
use egui::{Color32, Pos2};
use std::path::PathBuf;

/// Top-level UI model consumed by the egui renderer.
#[derive(Clone, Debug)]
pub struct UiState {
    pub status: StatusBarState,
    pub sources: SourcePanelState,
    pub triage: TriageState,
    pub waveform: WaveformState,
    pub drag: DragState,
    pub collections: CollectionsState,
    pub loaded_wav: Option<PathBuf>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            status: StatusBarState::idle(),
            sources: SourcePanelState::default(),
            triage: TriageState::default(),
            waveform: WaveformState::default(),
            drag: DragState::default(),
            collections: CollectionsState::default(),
            loaded_wav: None,
        }
    }
}

/// Status badge + text shown in the footer.
#[derive(Clone, Debug, PartialEq)]
pub struct StatusBarState {
    pub text: String,
    pub badge_label: String,
    pub badge_color: Color32,
}

impl StatusBarState {
    pub fn idle() -> Self {
        Self {
            text: "Add a sample source to get started".into(),
            badge_label: "Idle".into(),
            badge_color: Color32::from_rgb(42, 42, 42),
        }
    }
}

/// Sidebar list of sample sources.
#[derive(Clone, Debug, Default)]
pub struct SourcePanelState {
    pub rows: Vec<SourceRowView>,
    pub selected: Option<usize>,
    pub menu_row: Option<usize>,
    pub scroll_to: Option<usize>,
}

/// Display data for a single source row.
#[derive(Clone, Debug)]
pub struct SourceRowView {
    pub id: SourceId,
    pub name: String,
    pub path: String,
}

/// Cached waveform image and playback overlays.
#[derive(Clone, Debug)]
pub struct WaveformState {
    pub image: Option<WaveformImage>,
    pub playhead: PlayheadState,
    pub selection: Option<SelectionRange>,
    pub loop_enabled: bool,
}

impl Default for WaveformState {
    fn default() -> Self {
        Self {
            image: None,
            playhead: PlayheadState::default(),
            selection: None,
            loop_enabled: false,
        }
    }
}

/// Raw pixels ready to upload to an egui texture.
#[derive(Clone, Debug)]
pub struct WaveformImage {
    pub pixels: egui::ColorImage,
}

/// Current playhead position/visibility.
#[derive(Clone, Debug)]
pub struct PlayheadState {
    pub position: f32,
    pub visible: bool,
}

impl Default for PlayheadState {
    fn default() -> Self {
        Self {
            position: 0.0,
            visible: false,
        }
    }
}

/// Three-column triage state for wav entries.
#[derive(Clone, Debug, Default)]
pub struct TriageState {
    pub trash: Vec<WavRowView>,
    pub neutral: Vec<WavRowView>,
    pub keep: Vec<WavRowView>,
    pub selected: Option<TriageIndex>,
    pub loaded: Option<TriageIndex>,
}

/// Identifies a row inside one of the triage columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TriageIndex {
    pub column: TriageColumn,
    pub row: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriageColumn {
    Trash,
    Neutral,
    Keep,
}

/// Display data for a single wav row.
#[derive(Clone, Debug)]
pub struct WavRowView {
    pub path: PathBuf,
    pub name: String,
    pub tag: SampleTag,
    pub selected: bool,
    pub loaded: bool,
}

/// Drag/hover state shared between triage lists and collections.
#[derive(Clone, Debug, Default)]
pub struct DragState {
    pub active_path: Option<PathBuf>,
    pub label: String,
    pub position: Option<Pos2>,
    pub hovering_collection: Option<CollectionId>,
}

/// Collections sidebar and sample list state.
#[derive(Clone, Debug)]
pub struct CollectionsState {
    pub enabled: bool,
    pub rows: Vec<CollectionRowView>,
    pub selected: Option<usize>,
    pub samples: Vec<CollectionSampleView>,
    pub drop_ready: bool,
    pub drop_active: bool,
}

impl Default for CollectionsState {
    fn default() -> Self {
        Self {
            enabled: true,
            rows: Vec::new(),
            selected: None,
            samples: Vec::new(),
            drop_ready: false,
            drop_active: false,
        }
    }
}

/// Display data for a collection row.
#[derive(Clone, Debug)]
pub struct CollectionRowView {
    pub id: CollectionId,
    pub name: String,
    pub selected: bool,
    pub count: usize,
}

/// Display data for a sample inside a collection.
#[derive(Clone, Debug)]
pub struct CollectionSampleView {
    pub source: String,
    pub path: String,
}
