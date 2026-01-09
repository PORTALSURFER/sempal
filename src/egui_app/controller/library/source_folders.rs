use super::*;
use std::path::Path;

mod actions;
mod entry_updates;
mod selection;
mod tree;

pub(crate) use selection::folder_filter_accepts;
pub(crate) use tree::FolderBrowserModel;

// Folder entry/db/cache update helpers moved to `entry_updates` submodule.

impl EguiController {
    /// Focus a folder path inside the current source, rebuilding the folder browser first.
    pub(crate) fn focus_drop_target_folder(&mut self, path: &Path) {
        self.refresh_folder_browser();
        self.focus_folder_by_path(path);
    }
}
