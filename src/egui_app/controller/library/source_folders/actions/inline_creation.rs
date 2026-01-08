use super::ops;
use super::*;
use crate::egui_app::state::InlineFolderCreation;
use std::fs;
use std::path::{Path, PathBuf};

impl EguiController {
    pub(crate) fn start_new_folder(&mut self) {
        if self.current_source().is_none() {
            self.set_status("Add a source before creating folders", StatusTone::Info);
            return;
        }
        let parent = self.focused_folder_path().unwrap_or_default();
        self.begin_inline_folder_creation(parent);
    }

    pub(crate) fn start_new_folder_at_root(&mut self) {
        if self.current_source().is_none() {
            self.set_status("Add a source before creating folders", StatusTone::Info);
            return;
        }
        self.begin_inline_folder_creation(PathBuf::new());
    }

    fn begin_inline_folder_creation(&mut self, parent: PathBuf) {
        self.focus_folder_context();
        self.cancel_folder_rename();
        self.cancel_new_folder_creation();
        if !self.ui.sources.folders.search_query.trim().is_empty() {
            self.set_folder_search(String::new());
        }
        self.ensure_folder_expanded_for_creation(&parent);
        self.ui.sources.folders.new_folder = Some(InlineFolderCreation {
            parent: parent.clone(),
            name: String::new(),
            focus_requested: true,
        });
        let focus_index = if parent.as_os_str().is_empty() {
            Some(0)
        } else {
            self.ui
                .sources
                .folders
                .rows
                .iter()
                .position(|row| row.path == parent)
        };
        if let Some(index) = focus_index {
            self.ui.sources.folders.focused = Some(index);
            self.ui.sources.folders.scroll_to = Some(index);
        }
    }

    pub(crate) fn cancel_new_folder_creation(&mut self) {
        self.ui.sources.folders.new_folder = None;
    }

    fn ensure_folder_expanded_for_creation(&mut self, parent: &Path) {
        if parent.as_os_str().is_empty() {
            return;
        }
        let Some(model) = self.current_folder_model_mut() else {
            return;
        };
        if model.expanded.insert(parent.to_path_buf()) {
            let snapshot = model.clone();
            self.build_folder_rows(&snapshot);
        }
    }

    pub(crate) fn create_folder(&mut self, parent: &Path, name: &str) -> Result<(), String> {
        let folder_name = ops::normalize_folder_name(name)?;
        let source = self
            .current_source()
            .ok_or_else(|| "Select a source first".to_string())?;
        let relative = if parent.as_os_str().is_empty() {
            PathBuf::from(&folder_name)
        } else {
            parent.join(&folder_name)
        };
        let destination = source.root.join(&relative);
        if destination.exists() {
            return Err(format!("Folder already exists: {}", relative.display()));
        }
        fs::create_dir_all(&destination)
            .map_err(|err| format!("Failed to create folder: {err}"))?;
        self.update_manual_folders(|set| {
            set.insert(relative.clone());
        });
        self.refresh_folder_browser();
        self.focus_folder_by_path(&relative);
        self.set_status(
            format!("Created folder {}", relative.display()),
            StatusTone::Info,
        );
        Ok(())
    }
}
