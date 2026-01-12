use crate::sample_sources::{CollectionId, Rating, SourceId};
use std::path::PathBuf;

/// Collections sidebar and sample list state.
#[derive(Clone, Debug)]
pub struct CollectionsState {
    /// Whether collections are enabled in the UI.
    pub enabled: bool,
    /// Collection rows for rendering.
    pub rows: Vec<CollectionRowView>,
    /// Selected collection index.
    pub selected: Option<usize>,
    /// Samples shown for the selected collection.
    pub samples: Vec<CollectionSampleView>,
    /// Whether the panel is ready to accept drops.
    pub drop_ready: bool,
    /// Whether a drop is currently active.
    pub drop_active: bool,
    /// Selected sample index within the collection.
    pub selected_sample: Option<usize>,
    /// Sample index to scroll into view.
    pub scroll_to_sample: Option<usize>,
    /// Paths currently included in the multi-selection set.
    pub selected_paths: Vec<PathBuf>,
    /// Anchor row for range selection (shift + click).
    pub selection_anchor: Option<usize>,
    /// Last focused collection for restoring focus.
    pub last_focused_collection: Option<CollectionId>,
    /// Last focused sample path for restoring focus.
    pub last_focused_path: Option<PathBuf>,
    /// Pending inline action for the collections list.
    pub pending_action: Option<CollectionActionPrompt>,
    /// Whether to focus the rename editor.
    pub rename_focus_requested: bool,
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
            scroll_to_sample: None,
            selected_paths: Vec::new(),
            selection_anchor: None,
            last_focused_collection: None,
            last_focused_path: None,
            pending_action: None,
            rename_focus_requested: false,
        }
    }
}

/// Display data for a collection row.
#[derive(Clone, Debug)]
pub struct CollectionRowView {
    /// Collection identifier.
    pub id: CollectionId,
    /// Display name.
    pub name: String,
    /// Whether this row is selected.
    pub selected: bool,
    /// Number of samples in the collection.
    pub count: usize,
    /// Optional export path for the collection.
    pub export_path: Option<PathBuf>,
    /// Optional number hotkey (1-9) bound to this collection.
    pub hotkey: Option<u8>,
    /// Whether the export path is missing.
    pub missing: bool,
}

/// Pending inline action for the collections list.
#[derive(Clone, Debug)]
pub enum CollectionActionPrompt {
    /// Rename the selected collection.
    Rename {
        /// Collection to rename.
        target: CollectionId,
        /// New name.
        name: String,
    },
}

/// Display data for a sample inside a collection.
#[derive(Clone, Debug)]
pub struct CollectionSampleView {
    /// Source id that owns the sample.
    pub source_id: SourceId,
    /// Source display name.
    pub source: String,
    /// Sample path relative to the source root.
    pub path: PathBuf,
    /// Display label for the sample.
    pub label: String,
    /// Current rating/tag for the sample.
    pub tag: Rating,
    /// Whether the sample is missing on disk.
    pub missing: bool,
    /// Last playback timestamp in epoch seconds.
    pub last_played_at: Option<i64>,
}
