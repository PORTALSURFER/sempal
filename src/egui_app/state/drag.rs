use super::browser::TriageFlagColumn;
use crate::sample_sources::{CollectionId, SourceId};
use crate::selection::SelectionRange;
use egui::Pos2;
use std::path::PathBuf;

/// Active drag payload carried across UI panels.
#[derive(Clone, Debug, PartialEq)]
pub enum DragPayload {
    Sample {
        source_id: SourceId,
        relative_path: PathBuf,
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
    pub hovering_folder: Option<PathBuf>,
    pub hovering_folder_panel: bool,
    pub last_hovering_folder: Option<PathBuf>,
    pub external_started: bool,
}
