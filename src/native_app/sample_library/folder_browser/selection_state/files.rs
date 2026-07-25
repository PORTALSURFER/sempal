use super::{BrowserSelectionSnapshot, BrowserSelectionState};
use crate::native_app::sample_library::folder_browser::file_selection_model::{
    FileSelectionModel, SelectionToggleOutcome,
};
use radiant::widgets::PointerModifiers;
use std::collections::HashSet;

impl BrowserSelectionState {
    pub(in crate::native_app::sample_library::folder_browser) fn clear_file_selection(&mut self) {
        self.selected_file = None;
        self.selected_file_ids.clear();
        self.selected_file_ids_explicit = false;
        self.selected_file_ids_keyboard_range = false;
    }

    pub(in crate::native_app::sample_library::folder_browser) fn active_file_ids(
        &self,
    ) -> HashSet<String> {
        self.file_selection_model().active_ids()
    }

    pub(in crate::native_app::sample_library::folder_browser) fn selected_file_ids(
        &self,
    ) -> &HashSet<String> {
        &self.selected_file_ids
    }

    pub(in crate::native_app::sample_library::folder_browser) fn selected_file_ids_explicit(
        &self,
    ) -> bool {
        self.selected_file_ids_explicit
    }

    pub(in crate::native_app::sample_library::folder_browser) fn selected_file_count(
        &self,
    ) -> usize {
        self.selected_file_ids.len()
    }

    pub(in crate::native_app::sample_library::folder_browser) fn selected_file_id(
        &self,
    ) -> Option<&str> {
        self.selected_file.as_deref()
    }

    pub(in crate::native_app::sample_library::folder_browser) fn selected_file_ids_contains(
        &self,
        file_id: &str,
    ) -> bool {
        self.file_selection_model().contains_selected_id(file_id)
    }

    pub(in crate::native_app::sample_library::folder_browser) fn selected_file_active(
        &self,
    ) -> bool {
        self.selected_file.is_some()
    }

    pub(in crate::native_app::sample_library::folder_browser) fn snapshot(
        &self,
    ) -> BrowserSelectionSnapshot {
        BrowserSelectionSnapshot {
            selected_folder: self.selected_folder.clone(),
            selected_folder_ids: self.selected_folder_ids.clone(),
            selected_folder_ids_explicit: self.selected_folder_ids_explicit,
            folder_selection_anchor: self.folder_selection_anchor.clone(),
            selected_file: self.selected_file.clone(),
            selected_file_ids: self.selected_file_ids.clone(),
            selected_file_ids_explicit: self.selected_file_ids_explicit,
            selected_file_ids_keyboard_range: self.selected_file_ids_keyboard_range,
            selected_collection: self.selected_collection,
            folder_before_collection: self.folder_before_collection.clone(),
        }
    }

    pub(in crate::native_app::sample_library::folder_browser) fn restore_snapshot(
        &mut self,
        snapshot: BrowserSelectionSnapshot,
    ) {
        self.selected_folder = snapshot.selected_folder;
        self.selected_folder_ids = snapshot.selected_folder_ids;
        self.selected_folder_ids_explicit = snapshot.selected_folder_ids_explicit;
        self.folder_selection_anchor = snapshot.folder_selection_anchor;
        self.selected_file = snapshot.selected_file;
        self.selected_file_ids = snapshot.selected_file_ids;
        self.selected_file_ids_explicit = snapshot.selected_file_ids_explicit;
        self.selected_file_ids_keyboard_range = snapshot.selected_file_ids_keyboard_range;
        self.selected_collection = snapshot.selected_collection;
        self.folder_before_collection = snapshot.folder_before_collection;
    }

    pub(in crate::native_app::sample_library::folder_browser) fn select_single_file(
        &mut self,
        id: String,
        visible_ids: &[String],
    ) -> bool {
        if !visible_ids.contains(&id) {
            return false;
        }
        let mut selection = self.file_selection_model();
        let selected = selection.select_single(id, visible_ids);
        self.apply_file_selection_model(selection);
        selected
    }

    pub(in crate::native_app::sample_library::folder_browser) fn select_file_with_modifiers(
        &mut self,
        id: String,
        visible_ids: &[String],
        modifiers: PointerModifiers,
    ) -> bool {
        if !visible_ids.contains(&id) {
            return false;
        }
        let mut selection = self.file_selection_model();
        let selected = selection.select_with_modifiers(id, visible_ids, modifiers);
        self.apply_file_selection_model(selection);
        selected
    }

    pub(in crate::native_app::sample_library::folder_browser) fn focus_file_preserving_selection(
        &mut self,
        id: String,
        visible_ids: &[String],
    ) -> bool {
        let mut selection = self.file_selection_model();
        let focused = selection.focus_preserving_selection(id, visible_ids);
        self.apply_file_selection_model(selection);
        focused
    }

    pub(in crate::native_app::sample_library::folder_browser) fn select_all_files(
        &mut self,
        ids: Vec<String>,
    ) -> usize {
        let mut selection = self.file_selection_model();
        let count = selection.select_all(ids);
        self.apply_file_selection_model(selection);
        count
    }

    pub(in crate::native_app::sample_library::folder_browser) fn navigate_file(
        &mut self,
        delta: i32,
        extend: bool,
        visible_ids: &[String],
    ) -> Option<String> {
        let mut selection = self.file_selection_model();
        let target = selection.navigate(delta, extend, visible_ids)?;
        self.apply_file_selection_model(selection);
        Some(target)
    }

    pub(in crate::native_app::sample_library::folder_browser) fn navigate_file_to_adjacent_visible_id(
        &mut self,
        target: String,
    ) -> Option<String> {
        let mut selection = self.file_selection_model();
        let target = selection.navigate_to_adjacent_visible_id(target)?;
        self.apply_file_selection_model(selection);
        Some(target)
    }

    pub(in crate::native_app::sample_library::folder_browser) fn select_known_single_file(
        &mut self,
        target: String,
    ) -> bool {
        let mut selection = self.file_selection_model();
        let changed = selection.select_known_single(target);
        self.apply_file_selection_model(selection);
        changed
    }

    pub(in crate::native_app::sample_library::folder_browser) fn toggle_focused_file(
        &mut self,
        visible_ids: &[String],
    ) -> Option<SelectionToggleOutcome> {
        let mut selection = self.file_selection_model();
        let outcome = selection.toggle_focused(visible_ids)?;
        self.apply_file_selection_model(selection);
        Some(outcome)
    }

    pub(in crate::native_app::sample_library::folder_browser) fn reconcile_visible_files(
        &mut self,
        previous_visible_ids: &[String],
        visible_ids: &[String],
    ) {
        let mut selection = self.file_selection_model();
        selection.reconcile_visible(previous_visible_ids, visible_ids);
        self.apply_file_selection_model(selection);
    }

    pub(in crate::native_app::sample_library::folder_browser) fn retain_visible_files(
        &mut self,
        visible_ids: &HashSet<String>,
    ) {
        self.selected_file_ids.retain(|id| visible_ids.contains(id));
        if self
            .selected_file
            .as_ref()
            .is_some_and(|id| !visible_ids.contains(id))
        {
            self.selected_file = None;
        }
        if self.selected_file_ids.is_empty() {
            self.selected_file_ids_explicit = false;
        }
    }

    pub(in crate::native_app::sample_library::folder_browser) fn set_focus_file_set(
        &mut self,
        file_id: String,
    ) {
        self.selected_file = Some(file_id.clone());
        self.selected_file_ids.clear();
        self.selected_file_ids.insert(file_id);
        self.selected_file_ids_explicit = false;
    }

    pub(in crate::native_app::sample_library::folder_browser) fn restore_file_selection(
        &mut self,
        focused_id: String,
        selected_ids: HashSet<String>,
    ) {
        self.selected_file = Some(focused_id);
        self.selected_file_ids_explicit = selected_ids.len() > 1;
        self.selected_file_ids_keyboard_range = false;
        self.selected_file_ids = selected_ids;
    }

    fn file_selection_model(&self) -> FileSelectionModel {
        FileSelectionModel::new(
            self.selected_file.clone(),
            self.selected_file_ids.clone(),
            self.selected_file_ids_explicit,
        )
        .with_keyboard_range(self.selected_file_ids_keyboard_range)
    }

    fn apply_file_selection_model(&mut self, selection: FileSelectionModel) {
        self.selected_file = selection.focused_id().map(ToOwned::to_owned);
        self.selected_file_ids = selection.selected_ids().clone();
        self.selected_file_ids_explicit = selection.explicit();
        self.selected_file_ids_keyboard_range = selection.keyboard_range();
    }
}
