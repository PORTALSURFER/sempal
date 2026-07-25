use super::{BrowserSelectionSnapshot, BrowserSelectionState};
use crate::native_app::sample_library::folder_browser::path_helpers::rewrite_path_id;
use std::collections::{HashMap, HashSet};
use std::path::Path;

impl BrowserSelectionState {
    pub(in crate::native_app::sample_library::folder_browser) fn rewrite_folder_prefix(
        &mut self,
        old_path: &Path,
        new_path: &Path,
    ) {
        self.selected_folder = rewrite_path_id(&self.selected_folder, old_path, new_path);
        self.selected_folder_ids = self
            .selected_folder_ids
            .iter()
            .map(|id| rewrite_path_id(id, old_path, new_path))
            .collect();
        self.folder_selection_anchor = self
            .folder_selection_anchor
            .take()
            .map(|id| rewrite_path_id(&id, old_path, new_path));
        self.selected_file = self
            .selected_file
            .take()
            .map(|id| rewrite_path_id(&id, old_path, new_path));
        self.selected_file_ids = self
            .selected_file_ids
            .iter()
            .map(|id| rewrite_path_id(id, old_path, new_path))
            .collect();
    }

    pub(in crate::native_app::sample_library::folder_browser) fn set_renamed_file(
        &mut self,
        new_id: String,
    ) {
        self.set_focus_file_set(new_id);
    }

    pub(in crate::native_app::sample_library::folder_browser) fn restore_after_moved_files(
        &mut self,
        snapshot: BrowserSelectionSnapshot,
        moved_file_ids: &[(String, String)],
        visible_ids: &[String],
        fallback_id: Option<String>,
    ) {
        let visible_id_set = visible_ids.iter().cloned().collect::<HashSet<_>>();
        let moved_ids = moved_file_ids
            .iter()
            .cloned()
            .collect::<HashMap<String, String>>();
        self.restore_list_context_after_moved_files(&snapshot);
        self.selected_file_ids = snapshot
            .selected_file_ids
            .into_iter()
            .filter_map(|id| visible_moved_or_original_id(id, &moved_ids, &visible_id_set))
            .collect();
        self.selected_file_ids_explicit =
            snapshot.selected_file_ids_explicit && self.selected_file_ids.len() > 1;
        self.selected_file_ids_keyboard_range =
            snapshot.selected_file_ids_keyboard_range && !self.selected_file_ids.is_empty();
        self.selected_file = snapshot
            .selected_file
            .and_then(|id| visible_moved_or_original_id(id, &moved_ids, &visible_id_set))
            .or_else(|| {
                visible_ids
                    .iter()
                    .find(|id| self.selected_file_ids.contains(*id))
                    .cloned()
            })
            .or_else(|| fallback_id.filter(|id| visible_id_set.contains(id)));

        if self.selected_file.is_none()
            && self.selected_file_ids.is_empty()
            && let Some(first_visible) = visible_ids.first().cloned()
        {
            self.set_focus_file_set(first_visible);
        } else if self.selected_file.is_none() && self.selected_file_ids.is_empty() {
            self.selected_file_ids_explicit = false;
            self.selected_file_ids_keyboard_range = false;
        } else if let Some(selected_file) = self.selected_file.clone()
            && self.selected_file_ids.is_empty()
        {
            self.set_focus_file_set(selected_file);
        }
    }

    pub(in crate::native_app::sample_library::folder_browser) fn restore_list_context_after_moved_files(
        &mut self,
        snapshot: &BrowserSelectionSnapshot,
    ) {
        self.selected_folder = snapshot.selected_folder.clone();
        if snapshot.selected_collection.is_some() {
            self.selected_folder_ids = snapshot.selected_folder_ids.clone();
            self.selected_folder_ids_explicit = snapshot.selected_folder_ids_explicit;
            self.folder_selection_anchor = snapshot.folder_selection_anchor.clone();
            self.selected_collection = snapshot.selected_collection;
            self.folder_before_collection = snapshot.folder_before_collection.clone();
            return;
        }

        self.selected_folder_ids = [snapshot.selected_folder.clone()].into_iter().collect();
        self.selected_folder_ids_explicit = false;
        self.folder_selection_anchor = Some(snapshot.selected_folder.clone());
        self.selected_collection = None;
        self.folder_before_collection = None;
    }

    pub(in crate::native_app::sample_library::folder_browser) fn select_moved_files(
        &mut self,
        target_folder_id: String,
        moved_file_ids: &[(String, String)],
    ) {
        let selected_file_was_moved = self
            .selected_file
            .as_ref()
            .is_some_and(|id| moved_file_ids.iter().any(|(old_id, _)| old_id == id));
        self.selected_file = if selected_file_was_moved {
            self.selected_file.take().map(|id| {
                moved_file_ids
                    .iter()
                    .find(|(old_id, _)| old_id == &id)
                    .map(|(_, new_id)| new_id.clone())
                    .unwrap_or(id)
            })
        } else {
            moved_file_ids.first().map(|(_, new_id)| new_id.clone())
        };
        self.selected_file_ids = if self
            .selected_file_ids
            .iter()
            .any(|id| moved_file_ids.iter().any(|(old_id, _)| old_id == id))
        {
            self.selected_file_ids
                .iter()
                .map(|id| {
                    moved_file_ids
                        .iter()
                        .find(|(old_id, _)| old_id == id)
                        .map(|(_, new_id)| new_id.clone())
                        .unwrap_or_else(|| id.clone())
                })
                .collect()
        } else {
            moved_file_ids
                .iter()
                .map(|(_, new_id)| new_id.clone())
                .collect()
        };
        self.selected_folder = target_folder_id;
        self.selected_file_ids_explicit = self.selected_file_ids.len() > 1;
        self.selected_file_ids_keyboard_range = false;
    }

    pub(in crate::native_app::sample_library::folder_browser) fn discard_files(
        &mut self,
        removed_ids: &HashSet<String>,
    ) {
        if self
            .selected_file
            .as_ref()
            .is_some_and(|id| removed_ids.contains(id))
        {
            self.selected_file = None;
        }
        self.selected_file_ids
            .retain(|id| !removed_ids.contains(id));
        if self.selected_file.is_none() && self.selected_file_ids.is_empty() {
            self.selected_file_ids_explicit = false;
            self.selected_file_ids_keyboard_range = false;
        }
    }
}

fn visible_moved_or_original_id(
    id: String,
    moved_ids: &HashMap<String, String>,
    visible_id_set: &HashSet<String>,
) -> Option<String> {
    let restored_id = moved_ids.get(&id).cloned().unwrap_or(id);
    visible_id_set.contains(&restored_id).then_some(restored_id)
}
