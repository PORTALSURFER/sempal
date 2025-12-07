#![allow(dead_code)]
//! Shared state types for the egui UI.
// Temporary while the egui UI is being wired; types will be exercised by the renderer next.

use crate::egui_app::ui::style;
use crate::sample_sources::{CollectionId, SampleTag, SourceId};
use crate::selection::SelectionRange;
use egui::{Color32, Pos2};
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
    /// Master output volume (0.0-1.0).
    pub volume: f32,
    pub loaded_wav: Option<PathBuf>,
    /// Optional trash folder path configured by the user.
    pub trash_folder: Option<PathBuf>,
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
            volume: 1.0,
            loaded_wav: None,
            trash_folder: None,
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
            badge_color: style::status_badge_color(style::StatusTone::Idle),
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
    pub image: egui::ColorImage,
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

/// Sample browser state for wav entries with filterable rows.
#[derive(Clone, Debug)]
pub struct SampleBrowserState {
    /// Absolute indices per tag for keyboard navigation and tagging.
    pub trash: Vec<usize>,
    pub neutral: Vec<usize>,
    pub keep: Vec<usize>,
    /// Visible rows after applying the active filter.
    pub visible: Vec<usize>,
    /// Focused row used for playback/navigation (mirrors previously “selected”).
    pub selected: Option<SampleBrowserIndex>,
    pub loaded: Option<SampleBrowserIndex>,
    /// Visible row indices for selection/autoscroll (filtered list).
    pub selected_visible: Option<usize>,
    pub loaded_visible: Option<usize>,
    /// Visible row anchor used for range selection (shift + click/arrow).
    pub selection_anchor_visible: Option<usize>,
    /// Paths currently included in the multi-selection set.
    pub selected_paths: Vec<PathBuf>,
    pub autoscroll: bool,
    pub filter: TriageFlagFilter,
}

impl Default for SampleBrowserState {
    fn default() -> Self {
        Self {
            trash: Vec::new(),
            neutral: Vec::new(),
            keep: Vec::new(),
            visible: Vec::new(),
            selected: None,
            loaded: None,
            selected_visible: None,
            loaded_visible: None,
            selection_anchor_visible: None,
            selected_paths: Vec::new(),
            autoscroll: false,
            filter: TriageFlagFilter::All,
        }
    }
}

/// Identifies a row inside one of the triage flag columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleBrowserIndex {
    pub column: TriageFlagColumn,
    pub row: usize,
}

/// Wav triage flag columns: trash, neutral, keep.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriageFlagColumn {
    Trash,
    Neutral,
    Keep,
}

/// Filter options for the single-column sample browser view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriageFlagFilter {
    All,
    Keep,
    Trash,
    Untagged,
}

/// Active drag payload carried across UI panels.
#[derive(Clone, Debug, PartialEq)]
pub enum DragPayload {
    Sample {
        path: PathBuf,
    },
    Selection {
        source_id: SourceId,
        relative_path: PathBuf,
        bounds: SelectionRange,
    },
}

/// Drag/hover state shared between the sample browser and collections.
#[derive(Clone, Debug, Default)]
pub struct DragState {
    pub payload: Option<DragPayload>,
    pub label: String,
    pub position: Option<Pos2>,
    pub hovering_collection: Option<CollectionId>,
    pub hovering_drop_zone: bool,
    pub hovering_browser: Option<TriageFlagColumn>,
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
    pub selected_sample: Option<usize>,
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
            selected_sample: None,
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
    pub export_path: Option<PathBuf>,
}

/// Display data for a sample inside a collection.
#[derive(Clone, Debug)]
pub struct CollectionSampleView {
    pub source_id: SourceId,
    pub source: String,
    pub path: PathBuf,
    pub label: String,
    pub tag: SampleTag,
}
