use super::*;
use crate::egui_app::state::{DragSample, FocusContext, FolderActionPrompt, InlineFolderCreation};
use rfd::{MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
use std::fs;
use std::path::{Path, PathBuf};

fn normalize_folder_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Folder name cannot be empty".into());
    }
    if trimmed == "." || trimmed == ".." {
        return Err("Folder name is invalid".into());
    }
    if trimmed.contains(['/', '\\']) {
        return Err("Folder name cannot contain path separators".into());
    }
    Ok(trimmed.to_string())
}

fn folder_with_name(target: &Path, name: &str) -> PathBuf {
    target.parent().map_or_else(
        || PathBuf::from(name),
        |parent| {
            if parent.as_os_str().is_empty() {
                PathBuf::from(name)
            } else {
                parent.join(name)
            }
        },
    )
}

impl EguiController {
    pub(crate) fn open_folder_in_file_explorer(&mut self, relative_folder: &Path) {
        let Some(source) = self.current_source() else {
            self.set_status("Select a source first", StatusTone::Info);
            return;
        };
        let absolute = source.root.join(relative_folder);
        if !absolute.exists() {
            self.set_status(
                format!("Folder missing: {}", absolute.display()),
                StatusTone::Warning,
            );
            return;
        }
        if !absolute.is_dir() {
            self.set_status(
                format!("Not a folder: {}", absolute.display()),
                StatusTone::Warning,
            );
            return;
        }
        if let Err(err) = super::super::os_explorer::open_folder_in_file_explorer(&absolute) {
            self.set_status(err, StatusTone::Error);
        }
    }

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

    pub(crate) fn start_folder_rename(&mut self) {
        let Some(target) = self.focused_folder_path() else {
            self.set_status("Focus a folder to rename it", StatusTone::Info);
            return;
        };
        if target.as_os_str().is_empty() {
            self.set_status("Root folder cannot be renamed", StatusTone::Info);
            return;
        }
        let default = target
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string)
            .unwrap_or_else(|| target.to_string_lossy().into_owned());
        self.focus_folder_context();
        self.cancel_new_folder_creation();
        self.ui.sources.folders.pending_action = Some(FolderActionPrompt::Rename {
            target,
            name: default,
        });
        self.ui.sources.folders.rename_focus_requested = true;
    }

    pub(crate) fn cancel_folder_rename(&mut self) {
        if matches!(
            self.ui.sources.folders.pending_action,
            Some(FolderActionPrompt::Rename { .. })
        ) {
            self.ui.sources.folders.pending_action = None;
            self.ui.sources.folders.rename_focus_requested = false;
        }
    }

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

    pub(crate) fn rename_folder(&mut self, target: &Path, new_name: &str) -> Result<(), String> {
        let name = normalize_folder_name(new_name)?;
        let source = self
            .current_source()
            .ok_or_else(|| "Select a source first".to_string())?;
        let new_relative = folder_with_name(target, &name);
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
        if folder.as_os_str().is_empty() {
            return Err("Root folder cannot be moved".into());
        }
        if target_folder.starts_with(folder) {
            return Err("Cannot move a folder into itself".into());
        }
        let source = self
            .current_source()
            .ok_or_else(|| "Select a source first".to_string())?;
        let name = folder
            .file_name()
            .ok_or_else(|| "Folder name unavailable for move".to_string())?;
        let new_relative = if target_folder.as_os_str().is_empty() {
            PathBuf::from(name)
        } else {
            target_folder.join(name)
        };
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
            return Err(format!(
                "Folder already exists: {}",
                new_relative.display()
            ));
        }
        let affected = self.folder_entries(folder);
        fs::rename(&absolute_old, &absolute_new)
            .map_err(|err| format!("Failed to move folder: {err}"))?;
        if let Err(err) = self.rewrite_entries_for_folder(&source, folder, &new_relative, &affected)
        {
            let _ = fs::rename(&absolute_new, &absolute_old);
            return Err(err);
        }
        self.remap_manual_folders(folder, &new_relative);
        self.refresh_folder_browser();
        self.focus_folder_by_path(&new_relative);
        Ok(new_relative)
    }

    pub(crate) fn create_folder(&mut self, parent: &Path, name: &str) -> Result<(), String> {
        let folder_name = normalize_folder_name(name)?;
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

    fn confirm_folder_delete(&self, target: &Path) -> bool {
        if cfg!(test) {
            return true;
        }
        let message = format!(
            "Delete {} and all files inside it? This cannot be undone.",
            target.display()
        );
        matches!(
            MessageDialog::new()
                .set_title("Delete folder")
                .set_description(message)
                .set_level(MessageLevel::Warning)
                .set_buttons(MessageButtons::YesNo)
                .show(),
            MessageDialogResult::Yes
        )
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

    pub(crate) fn bind_folder_hotkey(&mut self, folder: &Path, hotkey: Option<u8>) {
        let Some(source) = self.current_source() else {
            self.set_status("Select a source first", StatusTone::Info);
            return;
        };
        let slot = match normalize_folder_hotkey(hotkey) {
            Ok(slot) => slot,
            Err(err) => {
                self.set_status(err, StatusTone::Error);
                return;
            }
        };
        if !folder.as_os_str().is_empty() && !source.root.join(folder).is_dir() {
            self.set_status(
                format!("Folder missing: {}", folder.display()),
                StatusTone::Error,
            );
            return;
        }
        let (snapshot, name) = match self.apply_folder_hotkey_binding(folder, slot) {
            Ok(state) => state,
            Err(err) => {
                self.set_status(err, StatusTone::Error);
                return;
            }
        };
        self.build_folder_rows(&snapshot);
        match slot {
            Some(slot) => self.set_status(
                format!("Bound hotkey {slot} to '{name}'"),
                StatusTone::Info,
            ),
            None => self.set_status(format!("Cleared hotkey for '{name}'"), StatusTone::Info),
        }
    }

    pub(crate) fn apply_folder_hotkey(&mut self, hotkey: u8, focus: FocusContext) -> bool {
        let Some(target) = self.resolve_folder_hotkey_target(hotkey, focus) else {
            return false;
        };
        match target {
            FolderHotkeyTarget::Missing => true,
            FolderHotkeyTarget::Ready { source, folder } => {
                self.run_folder_hotkey_move(&source, &folder);
                true
            }
        }
    }

    fn apply_folder_hotkey_binding(
        &mut self,
        folder: &Path,
        slot: Option<u8>,
    ) -> Result<(FolderBrowserModel, String), String> {
        let name = if folder.as_os_str().is_empty() {
            ".".to_string()
        } else {
            folder.to_string_lossy().into_owned()
        };
        let Some(model) = self.current_folder_model_mut() else {
            return Err("Select a source first".into());
        };
        model.hotkeys.retain(|key, path| *key != slot.unwrap_or(255) && path != folder);
        if let Some(slot) = slot {
            model.hotkeys.insert(slot, folder.to_path_buf());
        }
        Ok((model.clone(), name))
    }

    fn folder_for_hotkey(&self, hotkey: u8) -> Option<PathBuf> {
        self.current_folder_model()
            .and_then(|model| model.hotkeys.get(&hotkey).cloned())
    }

    fn resolve_folder_hotkey_target(
        &mut self,
        hotkey: u8,
        focus: FocusContext,
    ) -> Option<FolderHotkeyTarget> {
        if !matches!(focus, FocusContext::SampleBrowser) {
            return None;
        }
        let source = self.current_source()?;
        let folder = self.folder_for_hotkey(hotkey)?;
        if !folder.as_os_str().is_empty() && !source.root.join(&folder).is_dir() {
            self.set_status(
                format!("Folder missing: {}", folder.display()),
                StatusTone::Error,
            );
            return Some(FolderHotkeyTarget::Missing);
        }
        Some(FolderHotkeyTarget::Ready { source, folder })
    }

    fn browser_selection_rows_for_folder_move(&mut self) -> Vec<usize> {
        let mut rows: Vec<usize> = self
            .ui
            .browser
            .selected_paths
            .iter()
            .filter_map(|path| self.visible_row_for_path(path))
            .collect();
        if rows.is_empty() {
            if let Some(row) = self.focused_browser_row() {
                rows.push(row);
            }
        }
        rows.sort_unstable();
        rows.dedup();
        rows
    }

    fn samples_for_folder_move(
        &mut self,
        source: &SampleSource,
        rows: &[usize],
    ) -> Vec<DragSample> {
        rows.iter()
            .filter_map(|row| {
                let entry_index = *self.ui.browser.visible.get(*row)?;
                let entry = self.wav_entry(entry_index)?;
                Some(DragSample {
                    source_id: source.id.clone(),
                    relative_path: entry.relative_path.clone(),
                })
            })
            .collect()
    }

    fn next_focus_path_after_folder_move(&mut self, rows: &[usize]) -> Option<PathBuf> {
        if rows.is_empty() || self.ui.browser.visible.is_empty() {
            return None;
        }
        let mut sorted = rows.to_vec();
        sorted.sort_unstable();
        let highest = sorted.last().copied()?;
        let first = sorted.first().copied().unwrap_or(highest);
        let after = highest
            .checked_add(1)
            .and_then(|idx| self.ui.browser.visible.get(idx))
            .and_then(|entry_idx| self.wav_entry(*entry_idx))
            .map(|entry| entry.relative_path.clone());
        after.or_else(|| {
            first
                .checked_sub(1)
                .and_then(|idx| self.ui.browser.visible.get(idx))
                .and_then(|entry_idx| self.wav_entry(*entry_idx))
                .map(|entry| entry.relative_path.clone())
        })
    }

    fn apply_folder_move_focus(&mut self, next_focus: Option<PathBuf>) {
        let Some(path) = next_focus else {
            return;
        };
        if let Some(row) = self.visible_row_for_path(&path) {
            self.focus_browser_row_only(row);
        } else if self.wav_index_for_path(&path).is_some() {
            self.select_wav_by_path_with_rebuild(&path, true);
        }
    }

    fn run_folder_hotkey_move(&mut self, source: &SampleSource, folder: &Path) {
        let rows = self.browser_selection_rows_for_folder_move();
        if rows.is_empty() {
            self.set_status("Select samples to move to a folder", StatusTone::Info);
            return;
        }
        let samples = self.samples_for_folder_move(source, &rows);
        if samples.is_empty() {
            self.set_status("No samples available for folder move", StatusTone::Warning);
            return;
        }
        let next_focus = self.next_focus_path_after_folder_move(&rows);
        self.move_samples_to_folder(samples, folder.to_path_buf());
        self.clear_browser_selection();
        self.apply_folder_move_focus(next_focus);
    }
}

enum FolderHotkeyTarget {
    Missing,
    Ready { source: SampleSource, folder: PathBuf },
}

fn normalize_folder_hotkey(hotkey: Option<u8>) -> Result<Option<u8>, String> {
    match hotkey {
        None => Ok(None),
        Some(slot) if slot <= 9 => Ok(Some(slot)),
        Some(_) => Err("Folder hotkey must be between 0 and 9".into()),
    }
}
