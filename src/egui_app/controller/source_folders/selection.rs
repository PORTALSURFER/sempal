use super::*;

impl EguiController {
    pub(crate) fn replace_folder_selection(&mut self, row_index: usize) {
        self.apply_folder_selection(row_index, FolderSelectMode::Replace);
    }

    pub(crate) fn select_folder_range(&mut self, row_index: usize) {
        let rows = self.ui.sources.folders.rows.clone();
        if rows.is_empty() {
            return;
        }
        if rows.get(row_index).is_some_and(|row| row.is_root) {
            self.focus_folder_row(row_index);
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
        self.apply_folder_selection(row_index, FolderSelectMode::Toggle);
    }

    pub(in crate::egui_app::controller) fn toggle_focused_folder_selection(&mut self) {
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
        if view.is_root {
            return;
        }
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
        if view.is_root {
            return;
        }
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
                if is_root_path(&focused) {
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

    pub(in crate::egui_app::controller) fn drop_folder_focus(&mut self) {
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
        let Some(row) = self.ui.sources.folders.rows.get(row_index).cloned() else {
            return;
        };
        if row.is_root {
            return;
        }
        let path = row.path.clone();
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
        let Some(row) = self.ui.sources.folders.rows.get(row_index).cloned() else {
            return;
        };
        let path = row.path.clone();
        let snapshot = {
            let Some(model) = self.current_folder_model_mut() else {
                return;
            };
            if !row.is_root && !model.available.contains(&path) {
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
        let Some(row) = self.ui.sources.folders.rows.get(row_index).cloned() else {
            return;
        };
        if row.is_root {
            self.focus_folder_row(row_index);
            self.clear_folder_selection();
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

    pub(in crate::egui_app::controller) fn folder_selection_for_filter(
        &self,
    ) -> Option<&BTreeSet<PathBuf>> {
        let id = self.selected_source.as_ref()?;
        self.folder_browsers.get(id).map(|model| &model.selected)
    }

    pub(in crate::egui_app::controller) fn folder_filter_accepts(
        &self,
        relative_path: &Path,
    ) -> bool {
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

    pub(super) fn focused_folder_path(&self) -> Option<PathBuf> {
        let row = self.ui.sources.folders.focused?;
        self.ui
            .sources
            .folders
            .rows
            .get(row)
            .map(|row| row.path.clone())
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
        let Some(row) = self.ui.sources.folders.rows.get(row_index).cloned() else {
            return;
        };
        if row.is_root {
            self.focus_folder_row(row_index);
            self.clear_folder_selection();
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
        let id = self.selected_source.as_ref()?;
        self.folder_browsers
            .get(id)
            .and_then(|model| model.selection_anchor.clone())
    }

    pub(super) fn focus_folder_by_path(&mut self, path: &Path) {
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
