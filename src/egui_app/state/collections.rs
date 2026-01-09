use crate::sample_sources::{CollectionId, Rating, SourceId};
use std::path::PathBuf;

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
    pub scroll_to_sample: Option<usize>,
    /// Paths currently included in the multi-selection set.
    pub selected_paths: Vec<PathBuf>,
    /// Anchor row for range selection (shift + click).
    pub selection_anchor: Option<usize>,
    pub last_focused_collection: Option<CollectionId>,
    pub last_focused_path: Option<PathBuf>,
    pub pending_action: Option<CollectionActionPrompt>,
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
    pub id: CollectionId,
    pub name: String,
    pub selected: bool,
    pub count: usize,
    pub export_path: Option<PathBuf>,
    /// Optional number hotkey (1-9) bound to this collection.
    pub hotkey: Option<u8>,
    pub missing: bool,
}

/// Pending inline action for the collections list.
#[derive(Clone, Debug)]
pub enum CollectionActionPrompt {
    Rename { target: CollectionId, name: String },
}

/// Display data for a sample inside a collection.
#[derive(Clone, Debug)]
pub struct CollectionSampleView {
    pub source_id: SourceId,
    pub source: String,
    pub path: PathBuf,
    pub label: String,
    pub tag: Rating,
    pub missing: bool,
    pub last_played_at: Option<i64>,
}
