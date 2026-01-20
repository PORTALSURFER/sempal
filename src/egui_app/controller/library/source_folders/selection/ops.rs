use super::super::*;
use crate::egui_app::state::FolderRowView;
use std::collections::BTreeSet;

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

#[derive(Clone, Copy)]
enum FolderSelectMode {
    Replace,
    Toggle,
}

impl EguiController {
    pub(crate) fn replace_folder_selection(&mut self, row_index: usize) {
        self.clear_drop_target_selection();
        self.apply_folder_selection(row_index, FolderSelectMode::Replace);
    }

    pub(crate) fn select_folder_range(&mut self, row_index: usize) {
        self.clear_drop_target_selection();
        let rows = self.ui.sources.folders.rows.clone();
        if rows.is_empty() {
            return;
        }
        if rows.get(row_index).is_some_and(|row| row.is_root) {
            self.replace_folder_selection(row_index);
            return;
        }
        let Some(anchor_idx) = self.folder_anchor_index(&rows) else {
            self.replace_folder_selection(row_index);
            return;
        };
        let anchor_idx = anchor_idx.min(rows.len().saturating_sub(1));
        let row_index = row_index.min(rows.len().saturating_sub(1));
        if rows.get(anchor_idx).is_some_and(|row| row.is_root) {
            self.replace_folder_selection(row_index);
            return;
        }
        let start = anchor_idx.min(row_index);
        let end = anchor_idx.max(row_index);
        let selection: Vec<(PathBuf, bool)> = rows[start..=end]
            .iter()
            .filter(|row| !row.is_root)
            .map(|row| (row.path.clone(), row.has_children))
            .collect();
        if selection.is_empty() {
            return;
        }
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
        self.clear_drop_target_selection();
        self.apply_folder_selection(row_index, FolderSelectMode::Toggle);
    }

    pub(crate) fn toggle_focused_folder_selection(&mut self) {
        let Some(row) = self.ui.sources.folders.focused else {
            return;
        };
        self.toggle_folder_row_selection(row);
    }

    pub(crate) fn clear_folder_selection(&mut self) {
        self.clear_drop_target_selection();
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
                if focused.as_os_str().is_empty() {
                    model.selection_anchor = None;
                } else {
                    model.selection_anchor = Some(focused);
                }
            }
            model.clone()
        };
        // Preserve focus on the last focused row even after clearing selection.
        self.ui.sources.folders.scroll_to = self.ui.sources.folders.focused;
        self.build_folder_rows(&snapshot);
        self.rebuild_browser_lists();
    }

    pub(crate) fn add_folder_to_selection(&mut self, row_index: usize) {
        self.clear_drop_target_selection();
        let Some(row) = self.ui.sources.folders.rows.get(row_index).cloned() else {
            return;
        };
        if row.is_root {
            let (snapshot, selection_changed) = {
                let Some(model) = self.current_folder_model_mut() else {
                    return;
                };
                let before = model.selected.clone();
                model.selected.insert(PathBuf::new());
                if model.selection_anchor.is_none() {
                    model.selection_anchor = Some(PathBuf::new());
                }
                model.focused = Some(PathBuf::new());
                (model.clone(), before != model.selected)
            };
            self.ui.sources.folders.focused = Some(row_index);
            self.ui.sources.folders.scroll_to = Some(row_index);
            self.focus_folder_context();
            self.build_folder_rows(&snapshot);
            if selection_changed {
                self.rebuild_browser_lists();
            }
            return;
        }
        let path = row.path.clone();
        let (snapshot, selection_changed) = {
            let Some(model) = self.current_folder_model_mut() else {
                return;
            };
            if !model.available.contains(&path) {
                return;
            }
            let before = model.selected.clone();
            insert_folder(&mut model.selected, &path, row.has_children);
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

    pub(crate) fn toggle_folder_row_negation(&mut self, row_index: usize) {
        self.clear_drop_target_selection();
        let Some(row) = self.ui.sources.folders.rows.get(row_index).cloned() else {
            return;
        };
        let path = row.path.clone();
        let (snapshot, negation_changed) = {
            let Some(model) = self.current_folder_model_mut() else {
                return;
            };
            if !row.is_root && !model.available.contains(&path) {
                return;
            }
            let before = model.negated.clone();
            if model.negated.contains(&path) {
                model.negated.remove(&path);
            } else {
                model.negated.insert(path.clone());
            }
            model.focused = Some(path.clone());
            (model.clone(), before != model.negated)
        };
        self.ui.sources.folders.focused = Some(row_index);
        self.ui.sources.folders.scroll_to = Some(row_index);
        self.focus_folder_context();
        self.build_folder_rows(&snapshot);
        if negation_changed {
            self.rebuild_browser_lists();
        }
    }

    #[allow(dead_code)]
    pub(crate) fn selected_folder_paths(&self) -> Vec<PathBuf> {
        let Some(id) = self.selection_state.ctx.selected_source.as_ref() else {
            return Vec::new();
        };
        self.ui_cache
            .folders
            .models
            .get(id)
            .map(|model| model.selected.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub(in super::super) fn focused_folder_path(&self) -> Option<PathBuf> {
        let row = self.ui.sources.folders.focused?;
        self.ui
            .sources
            .folders
            .rows
            .get(row)
            .map(|row| row.path.clone())
    }

    fn apply_folder_selection(&mut self, row_index: usize, mode: FolderSelectMode) {
        let Some(row) = self.ui.sources.folders.rows.get(row_index).cloned() else {
            return;
        };
        if row.is_root {
            let (snapshot, selection_changed) = {
                let Some(model) = self.current_folder_model_mut() else {
                    return;
                };
                let before = model.selected.clone();
                let before_mode = model.root_filter_mode;
                let root_path = PathBuf::new();
                match mode {
                    FolderSelectMode::Replace => {
                        if model.selected.contains(&root_path) {
                            model.root_filter_mode = model.root_filter_mode.toggle();
                        } else {
                            model.selected.clear();
                            model.selected.insert(root_path.clone());
                            model.selection_anchor = Some(root_path.clone());
                            model.root_filter_mode =
                                crate::egui_app::state::RootFolderFilterMode::AllDescendants;
                        }
                    }
                    FolderSelectMode::Toggle => {
                        if model.selected.contains(&root_path) {
                            model.selected.remove(&root_path);
                            if model.selection_anchor.as_ref() == Some(&root_path) {
                                model.selection_anchor = None;
                            }
                        } else {
                            model.selected.insert(root_path.clone());
                            if model.selection_anchor.is_none() {
                                model.selection_anchor = Some(root_path.clone());
                            }
                            model.root_filter_mode =
                                crate::egui_app::state::RootFolderFilterMode::AllDescendants;
                        }
                    }
                }
                if model.selected.is_empty() {
                    model.selection_anchor = None;
                }
                let changed = before != model.selected || before_mode != model.root_filter_mode;
                if changed {
                    model.focused = Some(root_path);
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
            return;
        }
        let path = row.path.clone();
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
                    insert_folder(&mut model.selected, &path, row.has_children);
                    model.selection_anchor = Some(path.clone());
                }
                FolderSelectMode::Toggle => {
                    if model.selected.contains(&path) {
                        model.selected.remove(&path);
                        if model.selection_anchor.as_ref() == Some(&path) {
                            model.selection_anchor = None;
                        }
                    } else {
                        insert_folder(&mut model.selected, &path, row.has_children);
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
        let id = self.selection_state.ctx.selected_source.as_ref()?;
        self.ui_cache
            .folders
            .models
            .get(id)
            .and_then(|model| model.selection_anchor.clone())
    }

    pub(in super::super) fn focus_folder_by_path(&mut self, path: &Path) {
        self.clear_drop_target_selection();
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
}
