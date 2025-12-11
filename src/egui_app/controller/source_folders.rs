use super::*;
use crate::egui_app::state::{FolderActionPrompt, FolderRowView};
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;
use rfd::{MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
enum FolderSelectMode {
    Replace,
    Toggle,
}

#[derive(Clone, Default)]
pub(super) struct FolderBrowserModel {
    selected: BTreeSet<PathBuf>,
    expanded: BTreeSet<PathBuf>,
    focused: Option<PathBuf>,
    available: BTreeSet<PathBuf>,
    selection_anchor: Option<PathBuf>,
    manual_folders: BTreeSet<PathBuf>,
    search_query: String,
}

impl FolderBrowserModel {
    fn clear_focus_if_missing(&mut self) {
        if let Some(focused) = self.focused.clone() && !self.available.contains(&focused) {
            self.focused = None;
        }
    }

    fn clear_anchor_if_missing(&mut self) {
        if let Some(anchor) = self.selection_anchor.clone() && !self.available.contains(&anchor) {
            self.selection_anchor = None;
        }
    }
}

impl EguiController {
    pub(super) fn refresh_folder_browser(&mut self) {
        let Some(source_id) = self.selected_source.clone() else {
            self.ui.sources.folders = FolderBrowserUiState::default();
            return;
        };
        let Some(source) = self.current_source() else {
            self.ui.sources.folders = FolderBrowserUiState::default();
            return;
        };
        let available = self.collect_folders();
        let snapshot = {
            let model = self
                .folder_browsers
                .entry(source_id.clone())
                .or_default();
            model
                .manual_folders
                .retain(|path| source.root.join(path).is_dir());
            model.available = available;
            for path in model.manual_folders.iter().cloned() {
                model.available.insert(path);
            }
            model.selected.retain(|path| model.available.contains(path));
            model.expanded.retain(|path| model.available.contains(path));
            if model.expanded.is_empty() {
                for dir in model
                    .available
                    .iter()
                    .filter(|path| path.parent().is_none())
                {
                    model.expanded.insert(dir.clone());
                }
            }
            model.clear_focus_if_missing();
            model.clear_anchor_if_missing();
            for path in model.selected.iter() {
                let mut cursor = path.as_path();
                while let Some(parent) = cursor.parent() {
                    model.expanded.insert(parent.to_path_buf());
                    cursor = parent;
                }
            }
            model.clone()
        };
        self.ui.sources.folders.search_query = snapshot.search_query.clone();
        self.build_folder_rows(&snapshot);
    }

    pub(crate) fn replace_folder_selection(&mut self, row_index: usize) {
        self.apply_folder_selection(row_index, FolderSelectMode::Replace);
    }

    pub(crate) fn select_folder_range(&mut self, row_index: usize) {
        let rows = self.ui.sources.folders.rows.clone();
        if rows.is_empty() {
            return;
        }
        let Some(anchor_idx) = self.folder_anchor_index(&rows) else {
            self.replace_folder_selection(row_index);
            return;
        };
        let anchor_idx = anchor_idx.min(rows.len().saturating_sub(1));
        let row_index = row_index.min(rows.len().saturating_sub(1));
        let start = anchor_idx.min(row_index);
        let end = anchor_idx.max(row_index);
        let selection: Vec<(PathBuf, bool)> = rows[start..=end]
            .iter()
            .map(|row| (row.path.clone(), row.has_children))
            .collect();
        let (snapshot, selection_changed) = {
            let Some(model) = self.current_folder_model_mut() else {
                return;
            };
            model.selected.clear();
            for (path, has_children) in &selection {
                insert_folder(&mut model.selected, path, *has_children);
            }
            model.selection_anchor = Some(rows[anchor_idx].path.clone());
            model.focused = Some(rows[row_index].path.clone());
            (model.clone(), true)
        };
        self.ui.sources.folders.focused = Some(row_index);
        self.ui.sources.folders.scroll_to = Some(row_index);
        self.focus_folder_context();
        self.build_folder_rows(&snapshot);
        if selection_changed {
            self.rebuild_browser_lists();
        }
    }

    fn focused_folder_path(&self) -> Option<PathBuf> {
        let row = self.ui.sources.folders.focused?;
        self.ui
            .sources
            .folders
            .rows
            .get(row)
            .map(|row| row.path.clone())
    }

    pub(crate) fn toggle_folder_row_selection(&mut self, row_index: usize) {
        self.apply_folder_selection(row_index, FolderSelectMode::Toggle);
    }

    pub(super) fn toggle_focused_folder_selection(&mut self) {
        let Some(row) = self.ui.sources.folders.focused else {
            return;
        };
        self.toggle_folder_row_selection(row);
    }

    pub(crate) fn expand_focused_folder(&mut self) {
        let Some(row) = self.ui.sources.folders.focused else {
            return;
        };
        let Some(view) = self.ui.sources.folders.rows.get(row) else {
            return;
        };
        if view.has_children && !view.expanded {
            self.toggle_folder_expanded(row);
        }
    }

    pub(crate) fn collapse_focused_folder(&mut self) {
        let Some(row) = self.ui.sources.folders.focused else {
            return;
        };
        let Some(view) = self.ui.sources.folders.rows.get(row) else {
            return;
        };
        if view.has_children && view.expanded {
            self.toggle_folder_expanded(row);
            return;
        }
        if let Some(parent) = view.path.parent()
            && !parent.as_os_str().is_empty()
            && let Some(parent_index) = self
                .ui
                .sources
                .folders
                .rows
                .iter()
                .position(|row| row.path == parent)
        {
            self.focus_folder_row(parent_index);
        }
    }

    pub(crate) fn clear_folder_selection(&mut self) {
        let focused_path = self.ui.sources.folders.focused.and_then(|idx| {
            self.ui
                .sources
                .folders
                .rows
                .get(idx)
                .map(|row| row.path.clone())
        });
        let snapshot = {
            let Some(model) = self.current_folder_model_mut() else {
                return;
            };
            if model.selected.is_empty() {
                return;
            }
            model.selected.clear();
            if let Some(focused) = focused_path.clone() {
                model.focused = Some(focused.clone());
                model.selection_anchor = Some(focused);
            }
            model.clone()
        };
        // Preserve focus on the last focused row even after clearing selection.
        self.ui.sources.folders.scroll_to = self.ui.sources.folders.focused;
        self.build_folder_rows(&snapshot);
        self.rebuild_browser_lists();
    }

    pub(super) fn drop_folder_focus(&mut self) {
        self.ui.sources.folders.focused = None;
        self.ui.sources.folders.scroll_to = None;
        let Some(model) = self.current_folder_model_mut() else {
            return;
        };
        if model.focused.take().is_none() {
            return;
        }
        let snapshot = model.clone();
        self.build_folder_rows(&snapshot);
    }

    pub(crate) fn toggle_folder_expanded(&mut self, row_index: usize) {
        let Some(path) = self
            .ui
            .sources
            .folders
            .rows
            .get(row_index)
            .map(|row| row.path.clone())
        else {
            return;
        };
        let snapshot = {
            let Some(model) = self.current_folder_model_mut() else {
                return;
            };
            if !model.available.contains(&path) {
                return;
            }
            if !model.expanded.remove(&path) {
                model.expanded.insert(path.clone());
            }
            model.focused = Some(path);
            model.clone()
        };
        self.ui.sources.folders.focused = Some(row_index);
        self.ui.sources.folders.scroll_to = Some(row_index);
        self.focus_folder_context();
        self.build_folder_rows(&snapshot);
    }

    pub(crate) fn focus_folder_row(&mut self, row_index: usize) {
        let Some(path) = self
            .ui
            .sources
            .folders
            .rows
            .get(row_index)
            .map(|row| row.path.clone())
        else {
            return;
        };
        let snapshot = {
            let Some(model) = self.current_folder_model_mut() else {
                return;
            };
            if !model.available.contains(&path) {
                return;
            }
            model.focused = Some(path);
            model.clone()
        };
        self.ui.sources.folders.focused = Some(row_index);
        self.ui.sources.folders.scroll_to = Some(row_index);
        self.focus_folder_context();
        self.build_folder_rows(&snapshot);
    }

    #[allow(dead_code)]
    pub(crate) fn nudge_folder_focus(&mut self, offset: isize) {
        self.nudge_folder_selection(offset, false);
    }

    pub(crate) fn nudge_folder_selection(&mut self, offset: isize, extend: bool) {
        let Some(current) = self.ui.sources.folders.focused else {
            if !self.ui.sources.folders.rows.is_empty() {
                self.focus_folder_row(0);
            }
            return;
        };
        let len = self.ui.sources.folders.rows.len() as isize;
        if len == 0 {
            return;
        }
        let target = (current as isize + offset).clamp(0, len - 1) as usize;
        if extend {
            // Include the currently focused row plus the target step.
            self.add_folder_to_selection(current);
            self.add_folder_to_selection(target);
        } else {
            self.focus_folder_row(target);
        }
    }

    pub(crate) fn add_folder_to_selection(&mut self, row_index: usize) {
        let Some((path, has_children)) = self
            .ui
            .sources
            .folders
            .rows
            .get(row_index)
            .map(|row| (row.path.clone(), row.has_children))
        else {
            return;
        };
        let (snapshot, selection_changed) = {
            let Some(model) = self.current_folder_model_mut() else {
                return;
            };
            if !model.available.contains(&path) {
                return;
            }
            let before = model.selected.clone();
            insert_folder(&mut model.selected, &path, has_children);
            if model.selection_anchor.is_none() {
                model.selection_anchor = Some(path.clone());
            }
            model.focused = Some(path.clone());
            let changed = before != model.selected;
            (model.clone(), changed)
        };
        self.ui.sources.folders.focused = Some(row_index);
        self.ui.sources.folders.scroll_to = Some(row_index);
        self.focus_folder_context();
        self.build_folder_rows(&snapshot);
        if selection_changed {
            self.rebuild_browser_lists();
        }
    }

    pub(super) fn folder_selection_for_filter(&self) -> Option<&BTreeSet<PathBuf>> {
        let id = self.selected_source.as_ref()?;
        self.folder_browsers.get(id).map(|model| &model.selected)
    }

    pub(super) fn clear_folder_state_for(&mut self, source_id: &SourceId) {
        self.folder_browsers.remove(source_id);
    }

    pub(super) fn folder_filter_accepts(&self, relative_path: &Path) -> bool {
        let Some(selection) = self.folder_selection_for_filter() else {
            return true;
        };
        if selection.is_empty() {
            return true;
        }
        selection
            .iter()
            .any(|folder| relative_path.starts_with(folder))
    }

    pub(crate) fn selected_folder_paths(&self) -> Vec<PathBuf> {
        let Some(id) = self.selected_source.as_ref() else {
            return Vec::new();
        };
        self.folder_browsers
            .get(id)
            .map(|model| model.selected.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub(crate) fn delete_focused_folder(&mut self) {
        let Some(target) = self.focused_folder_path() else {
            self.set_status("Focus a folder to delete it", StatusTone::Info);
            return;
        };
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
        let default = target
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string)
            .unwrap_or_else(|| target.to_string_lossy().into_owned());
        self.focus_folder_context();
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
        let parent = self.focused_folder_path().unwrap_or_default();
        self.focus_folder_context();
        self.ui.sources.folders.pending_action = Some(FolderActionPrompt::Create {
            parent,
            name: String::new(),
        });
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

    pub(crate) fn set_folder_search(&mut self, query: String) {
        if self.selected_source.is_none() {
            self.ui.sources.folders.search_query = query;
            return;
        }
        let snapshot = {
            let Some(model) = self.current_folder_model_mut() else {
                self.ui.sources.folders.search_query = query;
                return;
            };
            if model.search_query == query {
                return;
            }
            model.search_query = query.clone();
            model.clone()
        };
        self.ui.sources.folders.search_query = query;
        self.build_folder_rows(&snapshot);
    }

    pub(crate) fn focus_folder_search(&mut self) {
        self.ui.sources.folders.search_focus_requested = true;
        self.focus_folder_context();
    }

    fn apply_folder_selection(&mut self, row_index: usize, mode: FolderSelectMode) {
        let Some((path, has_children)) = self
            .ui
            .sources
            .folders
            .rows
            .get(row_index)
            .map(|row| (row.path.clone(), row.has_children))
        else {
            return;
        };
        let (snapshot, selection_changed) = {
            let Some(model) = self.current_folder_model_mut() else {
                return;
            };
            if !model.available.contains(&path) {
                return;
            }
            let before = model.selected.clone();
            match mode {
                FolderSelectMode::Replace => {
                    model.selected.clear();
                    insert_folder(&mut model.selected, &path, has_children);
                    model.selection_anchor = Some(path.clone());
                }
                FolderSelectMode::Toggle => {
                    if model.selected.contains(&path) {
                        model.selected.remove(&path);
                        if model.selection_anchor.as_ref() == Some(&path) {
                            model.selection_anchor = None;
                        }
                    } else {
                        insert_folder(&mut model.selected, &path, has_children);
                        if model.selection_anchor.is_none() {
                            model.selection_anchor = Some(path.clone());
                        }
                    }
                }
            }
            if model.selected.is_empty() {
                model.selection_anchor = None;
            }
            let changed = before != model.selected;
            if changed {
                model.focused = Some(path.clone());
            }
            (model.clone(), changed)
        };
        self.ui.sources.folders.focused = Some(row_index);
        self.ui.sources.folders.scroll_to = Some(row_index);
        self.focus_folder_context();
        self.build_folder_rows(&snapshot);
        if selection_changed {
            self.rebuild_browser_lists();
        }
    }

    fn current_folder_model_mut(&mut self) -> Option<&mut FolderBrowserModel> {
        let id = self.selected_source.clone()?;
        Some(
            self.folder_browsers
                .entry(id)
                .or_default(),
        )
    }

    fn build_folder_rows(&mut self, model: &FolderBrowserModel) {
        self.ui.sources.folders.search_query = model.search_query.clone();
        let tree = self.build_folder_tree(&model.available);
        let searching = !model.search_query.trim().is_empty();
        let mut rows = Vec::new();
        let expanded = if searching {
            model.available.clone()
        } else {
            model.expanded.clone()
        };
        Self::flatten_folder_tree(Path::new(""), 0, &tree, model, &expanded, &mut rows);
        if searching {
            rows = self.filter_folder_rows(rows, &model.search_query);
        }
        let focused = model
            .focused
            .as_ref()
            .and_then(|path| rows.iter().position(|row| &row.path == path));
        self.ui.sources.folders.rows = rows;
        self.ui.sources.folders.focused = focused;
        self.ui.sources.folders.scroll_to = focused;
    }

    fn folder_anchor_index(&self, rows: &[FolderRowView]) -> Option<usize> {
        let anchor_path = self.current_folder_anchor_path().or_else(|| {
            self.ui
                .sources
                .folders
                .focused
                .and_then(|idx| rows.get(idx).map(|r| r.path.clone()))
        });
        anchor_path.and_then(|path| rows.iter().position(|row| row.path == path))
    }

    fn current_folder_anchor_path(&self) -> Option<PathBuf> {
        let id = self.selected_source.as_ref()?;
        self.folder_browsers
            .get(id)
            .and_then(|model| model.selection_anchor.clone())
    }

    fn collect_folders(&self) -> BTreeSet<PathBuf> {
        let mut folders = BTreeSet::new();
        for entry in &self.wav_entries {
            let mut current = entry.relative_path.parent();
            while let Some(path) = current {
                if !path.as_os_str().is_empty() {
                    folders.insert(path.to_path_buf());
                }
                current = path.parent();
            }
        }
        folders
    }

    fn build_folder_tree(&self, available: &BTreeSet<PathBuf>) -> BTreeMap<PathBuf, Vec<PathBuf>> {
        let mut tree: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
        for path in available {
            let parent = path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(PathBuf::new);
            tree.entry(parent).or_default().push(path.clone());
        }
        for children in tree.values_mut() {
            children.sort();
        }
        tree
    }

    fn filter_folder_rows(&self, rows: Vec<FolderRowView>, query: &str) -> Vec<FolderRowView> {
        let matcher = SkimMatcherV2::default();
        let mut scored = Vec::new();
        for row in rows {
            let label = row.path.to_string_lossy();
            if let Some(score) = matcher.fuzzy_match(label.as_ref(), query) {
                scored.push((row, score));
            }
        }
        scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.path.cmp(&b.0.path)));
        scored.into_iter().map(|(row, _)| row).collect()
    }

    fn flatten_folder_tree(
        parent: &Path,
        depth: usize,
        tree: &BTreeMap<PathBuf, Vec<PathBuf>>,
        model: &FolderBrowserModel,
        expanded: &BTreeSet<PathBuf>,
        rows: &mut Vec<FolderRowView>,
    ) {
        let Some(children) = tree.get(parent) else {
            return;
        };
        for child in children {
            let has_children = tree.contains_key(child);
            let is_expanded = expanded.contains(child);
            let selected = model.selected.contains(child);
            let name = child
                .file_name()
                .and_then(|n| n.to_str())
                .map(str::to_string)
                .unwrap_or_else(|| child.display().to_string());
            let row = FolderRowView {
                path: child.clone(),
                name,
                depth,
                has_children,
                expanded: is_expanded,
                selected,
            };
            rows.push(row);
            if has_children && is_expanded {
                Self::flatten_folder_tree(child, depth + 1, tree, model, expanded, rows);
            }
        }
    }

    fn folder_entries(&self, folder: &Path) -> Vec<WavEntry> {
        self.wav_entries
            .iter()
            .filter(|entry| entry.relative_path.starts_with(folder))
            .cloned()
            .collect()
    }

    fn rewrite_entries_for_folder(
        &mut self,
        source: &SampleSource,
        old_folder: &Path,
        new_folder: &Path,
        entries: &[WavEntry],
    ) -> Result<(), String> {
        if entries.is_empty() {
            return Ok(());
        }
        self.update_folder_db_entries(source, old_folder, new_folder, entries)?;
        self.update_folder_caches(source, old_folder, new_folder, entries)
    }

    fn update_folder_db_entries(
        &mut self,
        source: &SampleSource,
        old_folder: &Path,
        new_folder: &Path,
        entries: &[WavEntry],
    ) -> Result<(), String> {
        let db = self
            .database_for(source)
            .map_err(|err| format!("Database unavailable: {err}"))?;
        let mut batch = db
            .write_batch()
            .map_err(|err| format!("Failed to start database update: {err}"))?;
        for entry in entries {
            let suffix = entry
                .relative_path
                .strip_prefix(old_folder)
                .unwrap_or_else(|_| Path::new(""));
            let updated_path = new_folder.join(suffix);
            batch
                .remove_file(&entry.relative_path)
                .map_err(|err| format!("Failed to drop old entry: {err}"))?;
            batch
                .upsert_file(&updated_path, entry.file_size, entry.modified_ns)
                .map_err(|err| format!("Failed to register renamed file: {err}"))?;
            batch
                .set_tag(&updated_path, entry.tag)
                .map_err(|err| format!("Failed to copy tag: {err}"))?;
        }
        batch
            .commit()
            .map_err(|err| format!("Failed to save rename: {err}"))
    }

    fn update_folder_caches(
        &mut self,
        source: &SampleSource,
        old_folder: &Path,
        new_folder: &Path,
        entries: &[WavEntry],
    ) -> Result<(), String> {
        let mut collections_changed = false;
        let mut updates: Vec<(WavEntry, WavEntry)> = Vec::with_capacity(entries.len());
        for entry in entries {
            let suffix = entry
                .relative_path
                .strip_prefix(old_folder)
                .unwrap_or_else(|_| Path::new(""));
            let updated_path = new_folder.join(suffix);
            let mut new_entry = entry.clone();
            new_entry.relative_path = updated_path.clone();
            new_entry.missing = false;
            updates.push((entry.clone(), new_entry));
            if self.update_collections_for_rename(&source.id, &entry.relative_path, &updated_path) {
                collections_changed = true;
            }
        }
        self.apply_folder_entry_updates(source, &updates);
        if collections_changed {
            self.persist_config("Failed to save collection after folder rename")?;
        }
        Ok(())
    }

    fn apply_folder_entry_updates(
        &mut self,
        source: &SampleSource,
        updates: &[(WavEntry, WavEntry)],
    ) {
        if updates.is_empty() {
            return;
        }
        if let Some(cache) = self.wav_cache.get_mut(&source.id) {
            apply_entry_updates(cache, updates);
        }
        if self.selected_source.as_ref() == Some(&source.id) {
            apply_entry_updates(&mut self.wav_entries, updates);
            for (old_entry, new_entry) in updates {
                self.update_selection_paths(
                    source,
                    &old_entry.relative_path,
                    &new_entry.relative_path,
                );
                self.invalidate_cached_audio(&source.id, &old_entry.relative_path);
                self.invalidate_cached_audio(&source.id, &new_entry.relative_path);
            }
            self.rebuild_wav_lookup();
            self.rebuild_browser_lists();
            self.label_cache
                .insert(source.id.clone(), self.build_label_cache(&self.wav_entries));
        } else {
            self.label_cache.remove(&source.id);
        }
        self.rebuild_missing_lookup_for_source(&source.id);
    }

    fn update_manual_folders<F>(&mut self, mut update: F)
    where
        F: FnMut(&mut BTreeSet<PathBuf>),
    {
        let Some(model) = self.current_folder_model_mut() else {
            return;
        };
        update(&mut model.manual_folders);
    }

    fn remap_manual_folders(&mut self, old: &Path, new: &Path) {
        self.update_manual_folders(|set| {
            let descendants: Vec<PathBuf> = set
                .iter()
                .filter(|path| path.starts_with(old))
                .cloned()
                .collect();
            set.retain(|path| !path.starts_with(old));
            for path in descendants {
                let suffix = path.strip_prefix(old).unwrap_or_else(|_| Path::new(""));
                set.insert(new.join(suffix));
            }
        });
    }

    fn focus_folder_by_path(&mut self, path: &Path) {
        let Some(model) = self.current_folder_model_mut() else {
            return;
        };
        if !model.available.contains(path) {
            return;
        }
        model.focused = Some(path.to_path_buf());
        model.selection_anchor = Some(path.to_path_buf());
        model.selected.clear();
        model.selected.insert(path.to_path_buf());
        let snapshot = model.clone();
        self.build_folder_rows(&snapshot);
        self.rebuild_browser_lists();
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
}

fn ancestors(path: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    let mut current = path.parent();
    while let Some(parent) = current {
        if parent.as_os_str().is_empty() {
            break;
        }
        result.push(parent.to_path_buf());
        current = parent.parent();
    }
    result
}

fn remove_descendants(selected: &mut BTreeSet<PathBuf>, path: &Path) {
    let descendants: Vec<PathBuf> = selected
        .iter()
        .filter(|candidate| candidate != &path && candidate.starts_with(path))
        .cloned()
        .collect();
    for descendant in descendants {
        selected.remove(&descendant);
    }
}

fn insert_folder(selected: &mut BTreeSet<PathBuf>, path: &Path, has_children: bool) {
    selected.insert(path.to_path_buf());
    for ancestor in ancestors(path) {
        selected.remove(&ancestor);
    }
    if has_children {
        remove_descendants(selected, path);
    }
}

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

fn apply_entry_updates(list: &mut Vec<WavEntry>, updates: &[(WavEntry, WavEntry)]) {
    if updates.is_empty() {
        return;
    }
    let mut index_map: HashMap<PathBuf, usize> = list
        .iter()
        .enumerate()
        .map(|(idx, entry)| (entry.relative_path.clone(), idx))
        .collect();
    for (old_entry, new_entry) in updates {
        if let Some(idx) = index_map.remove(&old_entry.relative_path) {
            list[idx] = new_entry.clone();
            index_map.insert(new_entry.relative_path.clone(), idx);
        } else {
            list.push(new_entry.clone());
            index_map.insert(new_entry.relative_path.clone(), list.len() - 1);
        }
    }
    list.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
}
