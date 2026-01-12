use crate::sample_sources::SourceId;
use std::path::PathBuf;

/// Sidebar list of sample sources.
#[derive(Clone, Debug, Default)]
pub struct SourcePanelState {
    /// Render rows for configured sources.
    pub rows: Vec<SourceRowView>,
    /// Currently selected row index.
    pub selected: Option<usize>,
    /// Row index with an open context menu.
    pub menu_row: Option<usize>,
    /// Row index to scroll into view.
    pub scroll_to: Option<usize>,
    /// Folder browser sub-state.
    pub folders: FolderBrowserUiState,
    /// Drop target sub-state.
    pub drop_targets: DropTargetsUiState,
}

/// Display data for a single source row.
#[derive(Clone, Debug)]
pub struct SourceRowView {
    /// Source identifier.
    pub id: SourceId,
    /// Display name.
    pub name: String,
    /// Display path.
    pub path: String,
    /// Whether the source is missing on disk.
    pub missing: bool,
}

/// UI state for browsing folders within the active source.
#[derive(Clone, Debug, Default)]
pub struct FolderBrowserUiState {
    /// Render rows for the folder tree.
    pub rows: Vec<FolderRowView>,
    /// Currently focused row index.
    pub focused: Option<usize>,
    /// Row index to scroll into view.
    pub scroll_to: Option<usize>,
    /// Previously focused path for restore.
    pub last_focused_path: Option<PathBuf>,
    /// Active search query.
    pub search_query: String,
    /// Whether search focus is requested.
    pub search_focus_requested: bool,
    /// Whether rename focus is requested.
    pub rename_focus_requested: bool,
    /// Pending folder action prompt.
    pub pending_action: Option<FolderActionPrompt>,
    /// Inline folder creation state.
    pub new_folder: Option<InlineFolderCreation>,
}

/// Render-friendly folder row.
#[derive(Clone, Debug)]
pub struct FolderRowView {
    /// Full path for the folder.
    pub path: PathBuf,
    /// Display name.
    pub name: String,
    /// Depth in the tree.
    pub depth: usize,
    /// Whether the folder has children.
    pub has_children: bool,
    /// Whether the folder is expanded.
    pub expanded: bool,
    /// Whether the folder is selected.
    pub selected: bool,
    /// Whether the folder is negated in filters.
    pub negated: bool,
    /// Optional hotkey number.
    pub hotkey: Option<u8>,
    /// Whether this row represents the root.
    pub is_root: bool,
}

/// Pending inline action for the folder browser.
#[derive(Clone, Debug)]
pub enum FolderActionPrompt {
    /// Rename the target folder.
    Rename {
        /// Folder path to rename.
        target: PathBuf,
        /// New folder name.
        name: String,
    },
}

/// Inline editor state for a pending folder creation.
#[derive(Clone, Debug)]
pub struct InlineFolderCreation {
    /// Parent folder path.
    pub parent: PathBuf,
    /// New folder name.
    pub name: String,
    /// Whether the input should be focused.
    pub focus_requested: bool,
}

/// Sidebar list of configured drop targets.
#[derive(Clone, Debug, Default)]
pub struct DropTargetsUiState {
    /// Render rows for drop targets.
    pub rows: Vec<DropTargetRowView>,
    /// Currently selected row index.
    pub selected: Option<usize>,
    /// Row index with an open context menu.
    pub menu_row: Option<usize>,
    /// Row index to scroll into view.
    pub scroll_to: Option<usize>,
    /// User-defined height for the drop targets section, in points.
    pub height_override: Option<f32>,
    /// Cached height at the start of a resize drag for stable deltas.
    pub resize_origin_height: Option<f32>,
}

/// Display data for a single drop target row.
#[derive(Clone, Debug)]
pub struct DropTargetRowView {
    /// Drop target path.
    pub path: PathBuf,
    /// Display name.
    pub name: String,
    /// Whether the drop target path is missing.
    pub missing: bool,
    /// Optional drop target color.
    pub color: Option<crate::sample_sources::config::DropTargetColor>,
}
