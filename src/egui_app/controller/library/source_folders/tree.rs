use super::*;
use crate::egui_app::state::FolderRowView;
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn is_root_path(path: &Path) -> bool {
    path.as_os_str().is_empty()
}

#[derive(Clone, Default)]
pub(in crate::egui_app::controller) struct FolderBrowserModel {
    pub(super) selected: BTreeSet<PathBuf>,
    pub(super) negated: BTreeSet<PathBuf>,
    pub(super) expanded: BTreeSet<PathBuf>,
    pub(super) focused: Option<PathBuf>,
    pub(super) available: BTreeSet<PathBuf>,
    pub(super) selection_anchor: Option<PathBuf>,
    pub(super) manual_folders: BTreeSet<PathBuf>,
    pub(super) search_query: String,
    pub(super) last_disk_refresh: Option<Instant>,
    pub(super) hotkeys: BTreeMap<u8, PathBuf>,
}

impl FolderBrowserModel {
    fn clear_focus_if_missing(&mut self) {
        if let Some(focused) = self.focused.clone()
            && !self.available.contains(&focused)
            && !is_root_path(&focused)
        {
            self.focused = None;
        }
    }

    fn clear_anchor_if_missing(&mut self) {
        if let Some(anchor) = self.selection_anchor.clone()
            && !self.available.contains(&anchor)
            && !is_root_path(&anchor)
        {
            self.selection_anchor = None;
        }
    }
}

impl EguiController {
    pub(in crate::egui_app::controller) fn refresh_folder_browser(&mut self) {
        let Some(source_id) = self.selection_state.ctx.selected_source.clone() else {
            self.ui.sources.folders = FolderBrowserUiState::default();
            return;
        };
        let Some(source) = self.current_source() else {
            self.ui.sources.folders = FolderBrowserUiState::default();
            return;
        };
        let pending_load = self.runtime.jobs.wav_load_pending_for(&source.id);
        let empty_entries = self.wav_entries_len() == 0;
        let now = Instant::now();
        let (cached_available, last_disk_refresh) = {
            let model = self
                .ui_cache
                .folders
                .models
                .entry(source_id.clone())
                .or_default();
            (model.available.clone(), model.last_disk_refresh)
        };
        let disk_refresh_due = last_disk_refresh.map_or(true, |last| {
            now.duration_since(last) >= Duration::from_secs(2)
        });
        let reuse_available = empty_entries && !cached_available.is_empty() && !disk_refresh_due;
        let available = if reuse_available || (pending_load && empty_entries && !disk_refresh_due) {
            cached_available
        } else if disk_refresh_due {
            self.collect_folders(&source.root, true)
        } else {
            let mut collected = self.collect_folders(&source.root, false);
            if !cached_available.is_empty() {
                collected.extend(cached_available);
            }
            collected
        };
        let snapshot = {
            let model = self
                .ui_cache
                .folders
                .models
                .entry(source_id.clone())
                .or_default();
            model
                .manual_folders
                .retain(|path| source.root.join(path).is_dir());
            model
                .hotkeys
                .retain(|_, path| is_root_path(path) || source.root.join(path).is_dir());
            model.available = available;
            if disk_refresh_due {
                model.last_disk_refresh = Some(now);
            }
            for path in model.manual_folders.iter().cloned() {
                model.available.insert(path);
            }
            model
                .selected
                .retain(|path| is_root_path(path) || model.available.contains(path));
            model
                .negated
                .retain(|path| is_root_path(path) || model.available.contains(path));
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

    pub(in crate::egui_app) fn refresh_folder_browser_if_stale(&mut self, max_age: Duration) {
        let Some(source_id) = self.selection_state.ctx.selected_source.clone() else {
            return;
        };
        let now = Instant::now();
        let needs_refresh = {
            let model = self.ui_cache.folders.models.entry(source_id).or_default();
            model
                .last_disk_refresh
                .map_or(true, |last| now.duration_since(last) >= max_age)
        };
        if needs_refresh {
            self.refresh_folder_browser();
        }
    }

    pub(super) fn current_folder_model_mut(&mut self) -> Option<&mut FolderBrowserModel> {
        let id = self.selection_state.ctx.selected_source.clone()?;
        Some(self.ui_cache.folders.models.entry(id).or_default())
    }

    pub(super) fn current_folder_model(&self) -> Option<&FolderBrowserModel> {
        let id = self.selection_state.ctx.selected_source.as_ref()?;
        self.ui_cache.folders.models.get(id)
    }

    pub(super) fn build_folder_rows(&mut self, model: &FolderBrowserModel) {
        self.ui.sources.folders.search_query = model.search_query.clone();
        let hotkey_lookup: BTreeMap<PathBuf, u8> = model
            .hotkeys
            .iter()
            .map(|(slot, path)| (path.clone(), *slot))
            .collect();
        let tree = self.build_folder_tree(&model.available);
        let searching = !model.search_query.trim().is_empty();
        let mut folder_rows = Vec::new();
        let expanded = if searching {
            model.available.clone()
        } else {
            model.expanded.clone()
        };
        Self::flatten_folder_tree(
            Path::new(""),
            0,
            &tree,
            model,
            &expanded,
            &hotkey_lookup,
            &mut folder_rows,
        );
        if searching {
            folder_rows = self.filter_folder_rows(folder_rows, &model.search_query);
        }
        let mut rows = Vec::new();
        if self.selection_state.ctx.selected_source.is_some() && !searching {
            let has_children = !folder_rows.is_empty();
            let hotkey = hotkey_lookup.get(Path::new("")).copied();
            rows.push(FolderRowView {
                path: PathBuf::new(),
                name: ".".into(),
                depth: 0,
                has_children,
                expanded: true,
                selected: model.selected.contains(Path::new("")),
                negated: model.negated.contains(Path::new("")),
                hotkey,
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

    fn collect_folders(&mut self, source_root: &Path, include_disk: bool) -> BTreeSet<PathBuf> {
        let mut candidates = BTreeSet::new();
        for index in 0..self.wav_entries_len() {
            let Some(entry) = self.wav_entry(index) else {
                continue;
            };
            let mut current = entry.relative_path.parent();
            while let Some(path) = current {
                if !path.as_os_str().is_empty() {
                    candidates.insert(path.to_path_buf());
                }
                current = path.parent();
            }
        }

        let mut folders = BTreeSet::new();
        for path in candidates {
            if source_root.join(&path).is_dir() {
                folders.insert(path);
            }
        }
        if include_disk {
            Self::collect_disk_folders(source_root, PathBuf::new(), &mut folders);
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
        hotkeys: &BTreeMap<PathBuf, u8>,
        rows: &mut Vec<FolderRowView>,
    ) {
        let Some(children) = tree.get(parent) else {
            return;
        };
        for child in children {
            let has_children = tree.contains_key(child);
            let is_expanded = expanded.contains(child);
            let selected = model.selected.contains(child);
            let negated = model.negated.contains(child);
            let hotkey = hotkeys.get(child).copied();
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
                negated,
                hotkey,
                is_root: false,
            };
            rows.push(row);
            if has_children && is_expanded {
                Self::flatten_folder_tree(child, depth + 1, tree, model, expanded, hotkeys, rows);
            }
        }
    }

    fn collect_disk_folders(root: &Path, parent: PathBuf, folders: &mut BTreeSet<PathBuf>) {
        let Ok(entries) = fs::read_dir(root) else {
            return;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                continue;
            };
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let relative = if parent.as_os_str().is_empty() {
                PathBuf::from(name)
            } else {
                parent.join(&name)
            };
            folders.insert(relative.clone());
            Self::collect_disk_folders(&entry.path(), relative, folders);
        }
    }
}
