use super::*;
use crate::egui_app::state::FolderRowView;
use std::collections::{BTreeMap, BTreeSet, HashMap};
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
}

impl FolderBrowserModel {
    fn clear_focus_if_missing(&mut self) {
        if let Some(focused) = self.focused.clone() {
            if !self.available.contains(&focused) {
                self.focused = None;
            }
        }
    }

    fn clear_anchor_if_missing(&mut self) {
        if let Some(anchor) = self.selection_anchor.clone() {
            if !self.available.contains(&anchor) {
                self.selection_anchor = None;
            }
        }
    }
}

impl EguiController {
    pub(super) fn refresh_folder_browser(&mut self) {
        let Some(source_id) = self.selected_source.clone() else {
            self.ui.sources.folders = FolderBrowserUiState::default();
            return;
        };
        let available = self.collect_folders();
        let snapshot = {
            let model = self
                .folder_browsers
                .entry(source_id.clone())
                .or_insert_with(FolderBrowserModel::default);
            model.available = available;
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
        if let Some(parent) = view.path.parent() {
            if !parent.as_os_str().is_empty() {
                if let Some(parent_index) = self
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

    pub(crate) fn nudge_folder_focus(&mut self, offset: isize) {
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
        self.focus_folder_row(target);
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
                .or_insert_with(FolderBrowserModel::default),
        )
    }

    fn build_folder_rows(&mut self, model: &FolderBrowserModel) {
        let tree = self.build_folder_tree(&model.available);
        let mut rows = Vec::new();
        let mut path_to_index: HashMap<PathBuf, usize> = HashMap::new();
        self.flatten_folder_tree(
            Path::new(""),
            0,
            &tree,
            model,
            &mut rows,
            &mut path_to_index,
        );
        let focused = model
            .focused
            .as_ref()
            .and_then(|path| path_to_index.get(path).copied());
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

    fn flatten_folder_tree(
        &self,
        parent: &Path,
        depth: usize,
        tree: &BTreeMap<PathBuf, Vec<PathBuf>>,
        model: &FolderBrowserModel,
        rows: &mut Vec<FolderRowView>,
        path_to_index: &mut HashMap<PathBuf, usize>,
    ) {
        let Some(children) = tree.get(parent) else {
            return;
        };
        for child in children {
            let has_children = tree.contains_key(child);
            let expanded = model.expanded.contains(child);
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
                expanded,
                selected,
            };
            let index = rows.len();
            rows.push(row);
            path_to_index.insert(child.clone(), index);
            if has_children && expanded {
                self.flatten_folder_tree(child, depth + 1, tree, model, rows, path_to_index);
            }
        }
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
