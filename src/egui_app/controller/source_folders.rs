use super::*;
use crate::egui_app::state::{FolderActionPrompt, FolderRowView, InlineFolderCreation};
use rfd::{MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

mod actions;
mod entry_updates;
mod selection;
mod tree;

fn is_root_path(path: &Path) -> bool {
    path.as_os_str().is_empty()
}

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

// Folder entry/db/cache update helpers moved to `entry_updates` submodule.

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

// `apply_entry_updates` moved to `entry_updates` submodule.
