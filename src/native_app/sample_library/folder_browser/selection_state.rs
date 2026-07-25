use std::collections::HashSet;

use wavecrate::sample_sources::SampleCollection;

mod collections;
mod files;
mod folders;
mod reconciliation;

#[derive(Clone, Debug)]
pub(super) struct BrowserSelectionState {
    pub(super) selected_folder: String,
    pub(super) selected_folder_ids: HashSet<String>,
    pub(super) selected_folder_ids_explicit: bool,
    pub(super) folder_selection_anchor: Option<String>,
    pub(super) selected_file: Option<String>,
    pub(super) selected_file_ids: HashSet<String>,
    pub(super) selected_file_ids_explicit: bool,
    pub(super) selected_file_ids_keyboard_range: bool,
    pub(super) selected_collection: Option<SampleCollection>,
    pub(super) folder_before_collection: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct BrowserSelectionSnapshot {
    pub(super) selected_folder: String,
    pub(super) selected_folder_ids: HashSet<String>,
    pub(super) selected_folder_ids_explicit: bool,
    pub(super) folder_selection_anchor: Option<String>,
    pub(super) selected_file: Option<String>,
    pub(super) selected_file_ids: HashSet<String>,
    pub(super) selected_file_ids_explicit: bool,
    pub(super) selected_file_ids_keyboard_range: bool,
    pub(super) selected_collection: Option<SampleCollection>,
    pub(super) folder_before_collection: Option<String>,
}

impl BrowserSelectionState {
    pub(super) fn new(selected_folder: String) -> Self {
        Self {
            selected_folder: selected_folder.clone(),
            selected_folder_ids: [selected_folder].into_iter().collect(),
            selected_folder_ids_explicit: false,
            folder_selection_anchor: None,
            selected_file: None,
            selected_file_ids: HashSet::new(),
            selected_file_ids_explicit: false,
            selected_file_ids_keyboard_range: false,
            selected_collection: None,
            folder_before_collection: None,
        }
    }
}
