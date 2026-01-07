use super::ops;
use super::*;
use std::fs;
use std::path::{Path, PathBuf};

impl EguiController {
    pub(crate) fn delete_focused_folder(&mut self) {
        let Some(target) = self.focused_folder_path() else {
            self.set_status("Focus a folder to delete it", StatusTone::Info);
            return;
        };
        if target.as_os_str().is_empty() {
            self.set_status("Root folder cannot be deleted", StatusTone::Info);
            return;
        }
        match self.remove_folder(&target) {
            Ok(()) => self.set_status(
                format!("Deleted folder {}", target.display()),
                StatusTone::Info,
            ),
            Err(err) => self.set_status(err, StatusTone::Error),
        }
    }

    pub(crate) fn rename_folder(&mut self, target: &Path, new_name: &str) -> Result<(), String> {
        let new_relative = ops::rename_target(target, new_name)?;
        let source = self
            .current_source()
            .ok_or_else(|| "Select a source first".to_string())?;
        if target == new_relative {
            return Ok(());
        }
        let absolute_old = source.root.join(target);
        let absolute_new = source.root.join(&new_relative);
        if !absolute_old.exists() {
            return Err(format!("Folder not found: {}", target.display()));
        }
        if absolute_new.exists() {
            return Err(format!("Folder already exists: {}", new_relative.display()));
        }
        let affected = self.folder_entries(target);
        fs::rename(&absolute_old, &absolute_new)
            .map_err(|err| format!("Failed to rename folder: {err}"))?;
        self.rewrite_entries_for_folder(&source, target, &new_relative, &affected)?;
        self.remap_folder_state(target, &new_relative);
        self.remap_manual_folders(target, &new_relative);
        self.refresh_folder_browser();
        self.set_status(
            format!("Renamed folder to {}", new_relative.display()),
            StatusTone::Info,
        );
        Ok(())
    }

    pub(crate) fn move_folder_to_parent(
        &mut self,
        folder: &Path,
        target_folder: &Path,
    ) -> Result<PathBuf, String> {
        let new_relative = ops::move_target(folder, target_folder)?;
        let source = self
            .current_source()
            .ok_or_else(|| "Select a source first".to_string())?;
        let absolute_old = source.root.join(folder);
        if !absolute_old.is_dir() {
            return Err(format!("Folder not found: {}", folder.display()));
        }
        if !target_folder.as_os_str().is_empty() {
            let destination_dir = source.root.join(target_folder);
            if !destination_dir.is_dir() {
                return Err(format!("Folder not found: {}", target_folder.display()));
            }
        }
        let absolute_new = source.root.join(&new_relative);
        if absolute_new.exists() {
            return Err(format!("Folder already exists: {}", new_relative.display()));
        }
        let affected = self.folder_entries(folder);
        fs::rename(&absolute_old, &absolute_new)
            .map_err(|err| format!("Failed to move folder: {err}"))?;
        if let Err(err) = self.rewrite_entries_for_folder(&source, folder, &new_relative, &affected)
        {
            let _ = fs::rename(&absolute_new, &absolute_old);
            return Err(err);
        }
        self.remap_folder_state(folder, &new_relative);
        self.remap_manual_folders(folder, &new_relative);
        self.refresh_folder_browser();
        self.focus_folder_by_path(&new_relative);
        Ok(new_relative)
    }

    fn remove_folder(&mut self, target: &Path) -> Result<(), String> {
        let source = self
            .current_source()
            .ok_or_else(|| "Select a source first".to_string())?;
        let absolute = source.root.join(target);
        if !absolute.exists() {
            return Err(format!("Folder not found: {}", target.display()));
        }
        let next_focus = self.next_folder_focus_after_delete(target);
        if !self.confirm_folder_delete(target) {
            return Ok(());
        }
        let entries = self.folder_entries(target);
        fs::remove_dir_all(&absolute).map_err(|err| format!("Failed to delete folder: {err}"))?;
        let mut collections_changed = false;
        if !entries.is_empty() {
            let db = self
                .database_for(&source)
                .map_err(|err| format!("Database unavailable: {err}"))?;
            let mut batch = db
                .write_batch()
                .map_err(|err| format!("Failed to start database update: {err}"))?;
            for entry in &entries {
                batch
                    .remove_file(&entry.relative_path)
                    .map_err(|err| format!("Failed to drop database row: {err}"))?;
            }
            batch
                .commit()
                .map_err(|err| format!("Failed to save folder delete: {err}"))?;
        }
        for entry in entries {
            self.prune_cached_sample(&source, &entry.relative_path);
            if self.remove_sample_from_collections(&source.id, &entry.relative_path) {
                collections_changed = true;
            }
        }
        if collections_changed {
            self.persist_config("Failed to save collection after delete")?;
        }
        self.update_manual_folders(|set| {
            set.retain(|path| !path.starts_with(target));
        });
        self.prune_folder_state(target);
        self.refresh_folder_browser();
        if let Some(path) = next_focus {
            self.focus_folder_by_path(&path);
        } else {
            self.ui.sources.folders.focused = None;
            self.ui.sources.folders.scroll_to = None;
        }
        self.ui.sources.folders.pending_action = None;
        self.ui.sources.folders.new_folder = None;
        Ok(())
    }

    fn next_folder_focus_after_delete(&self, target: &Path) -> Option<PathBuf> {
        let rows = &self.ui.sources.folders.rows;
        let target_index = rows.iter().position(|row| row.path == target)?;
        let mut after = rows
            .iter()
            .skip(target_index + 1)
            .filter(|row| !row.path.starts_with(target));
        if let Some(row) = after.next() {
            return Some(row.path.clone());
        }
        rows.iter()
            .take(target_index)
            .rev()
            .find(|row| !row.path.starts_with(target))
            .map(|row| row.path.clone())
    }

    fn remap_folder_state(&mut self, old: &Path, new: &Path) {
        let Some(model) = self.current_folder_model_mut() else {
            return;
        };
        ops::remap_path_set(&mut model.selected, old, new);
        ops::remap_path_set(&mut model.negated, old, new);
        ops::remap_path_set(&mut model.expanded, old, new);
        ops::remap_path_set(&mut model.available, old, new);
        ops::remap_path_map(&mut model.hotkeys, old, new);
        model.focused = ops::remap_path_option(model.focused.take(), old, new);
        model.selection_anchor = ops::remap_path_option(model.selection_anchor.take(), old, new);
        self.ui.sources.folders.last_focused_path =
            ops::remap_path_option(self.ui.sources.folders.last_focused_path.take(), old, new);
    }

    fn prune_folder_state(&mut self, target: &Path) {
        let Some(model) = self.current_folder_model_mut() else {
            return;
        };
        model.selected.retain(|path| !path.starts_with(target));
        model.negated.retain(|path| !path.starts_with(target));
        model.expanded.retain(|path| !path.starts_with(target));
        model.available.retain(|path| !path.starts_with(target));
        model.hotkeys.retain(|_, path| !path.starts_with(target));
        if model
            .focused
            .as_ref()
            .is_some_and(|path| path.starts_with(target))
        {
            model.focused = None;
        }
        if model
            .selection_anchor
            .as_ref()
            .is_some_and(|path| path.starts_with(target))
        {
            model.selection_anchor = None;
        }
        if self
            .ui
            .sources
            .folders
            .last_focused_path
            .as_ref()
            .is_some_and(|path| path.starts_with(target))
        {
            self.ui.sources.folders.last_focused_path = None;
        }
    }
}
