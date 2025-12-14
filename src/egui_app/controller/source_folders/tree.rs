use super::*;
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

impl EguiController {
    pub(in crate::egui_app::controller) fn refresh_folder_browser(&mut self) {
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
            let model = self.folder_browsers.entry(source_id.clone()).or_default();
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

    pub(super) fn current_folder_model_mut(&mut self) -> Option<&mut FolderBrowserModel> {
        let id = self.selected_source.clone()?;
        Some(self.folder_browsers.entry(id).or_default())
    }

    pub(super) fn build_folder_rows(&mut self, model: &FolderBrowserModel) {
        self.ui.sources.folders.search_query = model.search_query.clone();
        let tree = self.build_folder_tree(&model.available);
        let searching = !model.search_query.trim().is_empty();
        let mut folder_rows = Vec::new();
        let expanded = if searching {
            model.available.clone()
        } else {
            model.expanded.clone()
        };
        Self::flatten_folder_tree(Path::new(""), 0, &tree, model, &expanded, &mut folder_rows);
        if searching {
            folder_rows = self.filter_folder_rows(folder_rows, &model.search_query);
        }
        let mut rows = Vec::new();
        if self.selected_source.is_some() && !searching {
            let has_children = !folder_rows.is_empty();
            rows.push(FolderRowView {
                path: PathBuf::new(),
                name: ".".into(),
                depth: 0,
                has_children,
                expanded: true,
                selected: false,
                is_root: true,
            });
        }
        rows.extend(folder_rows);
        let focused = model
            .focused
            .as_ref()
            .and_then(|path| rows.iter().position(|row| &row.path == path));
        self.ui.sources.folders.rows = rows;
        self.ui.sources.folders.focused = focused;
        self.ui.sources.folders.scroll_to = focused;
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
                is_root: false,
            };
            rows.push(row);
            if has_children && is_expanded {
                Self::flatten_folder_tree(child, depth + 1, tree, model, expanded, rows);
            }
        }
    }
}
