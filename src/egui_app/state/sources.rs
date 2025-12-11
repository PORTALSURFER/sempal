use crate::sample_sources::SourceId;
use std::path::PathBuf;

/// Sidebar list of sample sources.
#[derive(Clone, Debug, Default)]
pub struct SourcePanelState {
    pub rows: Vec<SourceRowView>,
    pub selected: Option<usize>,
    pub menu_row: Option<usize>,
    pub scroll_to: Option<usize>,
    pub folders: FolderBrowserUiState,
}

/// Display data for a single source row.
#[derive(Clone, Debug)]
pub struct SourceRowView {
    pub id: SourceId,
    pub name: String,
    pub path: String,
    pub missing: bool,
}

/// UI state for browsing folders within the active source.
#[derive(Clone, Debug, Default)]
pub struct FolderBrowserUiState {
    pub rows: Vec<FolderRowView>,
    pub focused: Option<usize>,
    pub scroll_to: Option<usize>,
    pub search_query: String,
    pub search_focus_requested: bool,
    pub rename_focus_requested: bool,
    pub pending_action: Option<FolderActionPrompt>,
    pub new_folder: Option<InlineFolderCreation>,
}

/// Render-friendly folder row.
#[derive(Clone, Debug)]
pub struct FolderRowView {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub has_children: bool,
    pub expanded: bool,
    pub selected: bool,
    pub is_root: bool,
}

/// Pending inline action for the folder browser.
#[derive(Clone, Debug)]
pub enum FolderActionPrompt {
    Rename { target: PathBuf, name: String },
}

/// Inline editor state for a pending folder creation.
#[derive(Clone, Debug)]
pub struct InlineFolderCreation {
    pub parent: PathBuf,
    pub name: String,
    pub focus_requested: bool,
}
