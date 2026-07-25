use std::collections::HashSet;

use radiant::{prelude as ui, widgets::PointerModifiers};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FileSelectionModel {
    focused_id: Option<String>,
    selected_ids: HashSet<String>,
    explicit: bool,
    keyboard_range: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SelectionToggleOutcome {
    pub(super) toggled_id: String,
    pub(super) toggled_selected: bool,
    pub(super) focused_id: String,
}

impl FileSelectionModel {
    pub(super) fn new(
        focused_id: Option<String>,
        selected_ids: HashSet<String>,
        explicit: bool,
    ) -> Self {
        Self {
            focused_id,
            selected_ids,
            explicit,
            keyboard_range: false,
        }
    }

    pub(super) fn focused_id(&self) -> Option<&str> {
        self.focused_id.as_deref()
    }

    pub(super) fn selected_ids(&self) -> &HashSet<String> {
        &self.selected_ids
    }

    pub(super) fn explicit(&self) -> bool {
        self.explicit
    }

    pub(super) fn keyboard_range(&self) -> bool {
        self.keyboard_range
    }

    pub(super) fn with_keyboard_range(mut self, keyboard_range: bool) -> Self {
        self.keyboard_range = keyboard_range;
        self
    }

    pub(super) fn active_ids(&self) -> HashSet<String> {
        if self.explicit || !self.selected_ids.is_empty() {
            return self.selected_ids.clone();
        }
        self.focused_id
            .as_deref()
            .map(|id| [id.to_string()].into_iter().collect())
            .unwrap_or_default()
    }

    pub(super) fn contains_selected_id(&self, file_id: &str) -> bool {
        self.selected_ids.contains(file_id)
    }

    pub(super) fn select_single(&mut self, id: String, visible_ids: &[String]) -> bool {
        if !visible_ids.contains(&id) {
            return false;
        }
        let mut selection = ui::KeyedListSelection::new();
        selection.select_with_intent(id, visible_ids, ui::ListSelectionIntent::Replace);
        self.apply_keyed_selection(selection);
        true
    }

    pub(super) fn select_with_modifiers(
        &mut self,
        id: String,
        visible_ids: &[String],
        modifiers: PointerModifiers,
    ) -> bool {
        if !visible_ids.contains(&id) {
            return false;
        }
        let mut selection = self.keyed_selection();
        selection.select_with_intent(
            id,
            visible_ids,
            ui::ListSelectionIntent::from_extend_toggle(modifiers.shift, modifiers.command),
        );
        self.apply_keyed_selection(selection);
        self.explicit = modifiers.shift || modifiers.command;
        true
    }

    pub(super) fn focus_preserving_selection(
        &mut self,
        id: String,
        visible_ids: &[String],
    ) -> bool {
        if self.selected_ids.contains(&id) && visible_ids.contains(&id) {
            self.focused_id = Some(id);
            return true;
        }
        self.select_single(id, visible_ids)
    }

    pub(super) fn select_all(&mut self, ids: Vec<String>) -> usize {
        let mut selection = self.keyed_selection();
        selection.select_all(&ids);
        self.apply_keyed_selection(selection);
        self.explicit = true;
        self.selected_ids.len()
    }

    pub(super) fn navigate(
        &mut self,
        delta: i32,
        extend: bool,
        visible_ids: &[String],
    ) -> Option<String> {
        let marked_selection = self.explicit && !self.keyboard_range;
        if marked_selection && !extend {
            return self.navigate_focus_preserving_selection(delta, visible_ids);
        }

        let mut selection = self.keyed_selection();
        let target = if extend {
            selection.navigate_preserving_existing(delta as isize, visible_ids)?
        } else {
            selection.navigate(delta as isize, visible_ids, false)?
        };
        self.apply_keyed_selection(selection);
        self.explicit = extend;
        self.keyboard_range = extend && !marked_selection;
        Some(target)
    }

    pub(super) fn navigate_to_adjacent_visible_id(&mut self, target: String) -> Option<String> {
        if self.focused_id.as_deref() == Some(target.as_str()) {
            return None;
        }
        if self.explicit && !self.keyboard_range {
            self.focused_id = Some(target.clone());
            return Some(target);
        }

        self.focused_id = Some(target.clone());
        self.selected_ids.clear();
        self.selected_ids.insert(target.clone());
        self.explicit = false;
        self.keyboard_range = false;
        Some(target)
    }

    pub(super) fn select_known_single(&mut self, target: String) -> bool {
        let changed = self.focused_id.as_deref() != Some(target.as_str())
            || self.explicit
            || self.selected_ids.len() != 1
            || !self.selected_ids.contains(&target);
        self.focused_id = Some(target.clone());
        self.selected_ids.clear();
        self.selected_ids.insert(target);
        self.explicit = false;
        self.keyboard_range = false;
        changed
    }

    pub(super) fn toggle_focused(
        &mut self,
        visible_ids: &[String],
    ) -> Option<SelectionToggleOutcome> {
        let focused = self.focused_id.as_ref()?;
        if !visible_ids.iter().any(|id| id == focused) {
            return None;
        }
        let toggled_id = focused.clone();
        let already_marked = self.explicit && self.selected_ids.contains(&toggled_id);
        let toggled_selected = if already_marked {
            self.selected_ids.remove(&toggled_id);
            false
        } else {
            self.selected_ids.insert(toggled_id.clone());
            true
        };
        self.explicit = true;
        let focused_id = toggled_id.clone();
        self.focused_id = Some(focused_id.clone());
        Some(SelectionToggleOutcome {
            toggled_id,
            toggled_selected,
            focused_id,
        })
    }

    pub(super) fn reconcile_visible(
        &mut self,
        previous_visible_ids: &[String],
        visible_ids: &[String],
    ) {
        let visible_id_set = visible_ids.iter().cloned().collect::<HashSet<_>>();
        let previous_focused_id = self.focused_id.clone();
        self.selected_ids.retain(|id| visible_id_set.contains(id));

        if previous_focused_id
            .as_ref()
            .is_some_and(|id| visible_id_set.contains(id))
        {
            return;
        }

        self.focused_id = previous_focused_id
            .as_ref()
            .and_then(|id| {
                previous_visible_ids
                    .iter()
                    .position(|candidate| candidate == id)
            })
            .and_then(|previous_index| {
                if visible_ids.is_empty() {
                    None
                } else {
                    visible_ids.get(previous_index.min(visible_ids.len() - 1))
                }
            })
            .cloned();

        if !self.explicit {
            self.selected_ids.clear();
            if let Some(focused_id) = self.focused_id.clone() {
                self.selected_ids.insert(focused_id);
            }
        }

        if self.focused_id.is_none() && self.selected_ids.is_empty() {
            self.explicit = false;
        }
    }

    fn navigate_focus_preserving_selection(
        &mut self,
        delta: i32,
        visible_ids: &[String],
    ) -> Option<String> {
        let current = self.focused_id.as_ref()?;
        let current_index = visible_ids.iter().position(|id| id == current)?;
        let target_index =
            ui::list_index_after_delta(current_index, delta as isize, visible_ids.len())?;
        if target_index == current_index {
            return None;
        }
        let target = visible_ids[target_index].clone();
        self.focused_id = Some(target.clone());
        Some(target)
    }

    fn keyed_selection(&self) -> ui::KeyedListSelection<String> {
        ui::KeyedListSelection::from_parts(
            self.focused_id.clone(),
            self.focused_id.clone(),
            self.selected_ids.clone(),
        )
    }

    fn apply_keyed_selection(&mut self, selection: ui::KeyedListSelection<String>) {
        self.focused_id = selection.focused_key().cloned();
        self.selected_ids = selection.selected_keys().iter().cloned().collect();
        self.explicit = false;
        self.keyboard_range = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn set(values: &[&str]) -> HashSet<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn implicit_selection_uses_focused_file_as_active_id() {
        let selection = FileSelectionModel::new(Some(String::from("kick")), HashSet::new(), false);

        assert_eq!(selection.active_ids(), set(&["kick"]));
    }

    #[test]
    fn command_selection_tracks_explicit_multi_selection() {
        let visible_ids = ids(&["kick", "snare", "hat"]);
        let mut selection = FileSelectionModel::new(None, HashSet::new(), false);

        assert!(selection.select_single(String::from("kick"), &visible_ids));
        assert!(selection.select_with_modifiers(
            String::from("hat"),
            &visible_ids,
            PointerModifiers {
                command: true,
                ..PointerModifiers::default()
            },
        ));

        assert_eq!(selection.focused_id(), Some("hat"));
        assert!(selection.explicit());
        assert_eq!(selection.active_ids(), set(&["kick", "hat"]));
    }

    #[test]
    fn navigation_uses_supplied_filtered_visible_ids() {
        let visible_ids = ids(&["kick", "hat"]);
        let mut selection =
            FileSelectionModel::new(Some(String::from("kick")), set(&["kick"]), false);

        let target = selection.navigate(1, false, &visible_ids);

        assert_eq!(target.as_deref(), Some("hat"));
        assert_eq!(selection.focused_id(), Some("hat"));
        assert!(!selection.explicit());
        assert_eq!(selection.active_ids(), set(&["hat"]));
    }

    #[test]
    fn adjacent_fast_navigation_replaces_implicit_selection() {
        let mut selection =
            FileSelectionModel::new(Some(String::from("kick")), set(&["kick"]), false);

        let target = selection.navigate_to_adjacent_visible_id(String::from("snare"));

        assert_eq!(target.as_deref(), Some("snare"));
        assert_eq!(selection.focused_id(), Some("snare"));
        assert!(!selection.explicit());
        assert_eq!(selection.active_ids(), set(&["snare"]));
    }

    #[test]
    fn adjacent_fast_navigation_preserves_explicit_selection() {
        let mut selection =
            FileSelectionModel::new(Some(String::from("kick")), set(&["kick", "hat"]), true);

        let target = selection.navigate_to_adjacent_visible_id(String::from("snare"));

        assert_eq!(target.as_deref(), Some("snare"));
        assert_eq!(selection.focused_id(), Some("snare"));
        assert!(selection.explicit());
        assert_eq!(selection.active_ids(), set(&["kick", "hat"]));
    }

    #[test]
    fn known_single_selection_collapses_explicit_selection_without_visible_ids() {
        let mut selection =
            FileSelectionModel::new(Some(String::from("kick")), set(&["kick", "hat"]), true);

        assert!(selection.select_known_single(String::from("snare")));

        assert_eq!(selection.focused_id(), Some("snare"));
        assert!(!selection.explicit());
        assert_eq!(selection.active_ids(), set(&["snare"]));
    }

    #[test]
    fn known_single_selection_reports_unchanged_when_already_single() {
        let mut selection =
            FileSelectionModel::new(Some(String::from("kick")), set(&["kick"]), false);

        assert!(!selection.select_known_single(String::from("kick")));

        assert_eq!(selection.focused_id(), Some("kick"));
        assert!(!selection.explicit());
        assert_eq!(selection.active_ids(), set(&["kick"]));
    }

    #[test]
    fn focus_preserving_selection_keeps_explicit_selection_set() {
        let visible_ids = ids(&["kick", "snare", "hat"]);
        let mut selection =
            FileSelectionModel::new(Some(String::from("kick")), set(&["kick", "hat"]), true);

        assert!(selection.focus_preserving_selection(String::from("hat"), &visible_ids));

        assert_eq!(selection.focused_id(), Some("hat"));
        assert!(selection.explicit());
        assert_eq!(selection.active_ids(), set(&["kick", "hat"]));
    }

    #[test]
    fn reconcile_visible_moves_hidden_focus_to_next_row() {
        let mut selection =
            FileSelectionModel::new(Some(String::from("kick")), set(&["kick", "hat"]), true);

        selection.reconcile_visible(&ids(&["kick", "hat"]), &ids(&["hat"]));

        assert_eq!(selection.focused_id(), Some("hat"));
        assert!(selection.explicit());
        assert_eq!(selection.active_ids(), set(&["hat"]));
    }

    #[test]
    fn reconcile_visible_moves_hidden_focus_to_previous_row_at_end() {
        let mut selection = FileSelectionModel::new(
            Some(String::from("hat")),
            set(&["kick", "snare", "hat"]),
            false,
        );

        selection.reconcile_visible(&ids(&["kick", "snare", "hat"]), &ids(&["kick"]));

        assert_eq!(selection.focused_id(), Some("kick"));
        assert!(!selection.explicit());
        assert_eq!(selection.active_ids(), set(&["kick"]));
    }

    #[test]
    fn reconcile_visible_clears_focus_when_no_rows_remain() {
        let mut selection =
            FileSelectionModel::new(Some(String::from("kick")), set(&["kick"]), false);

        selection.reconcile_visible(&ids(&["kick"]), &[]);

        assert_eq!(selection.focused_id(), None);
        assert!(!selection.explicit());
        assert!(selection.active_ids().is_empty());
    }

    #[test]
    fn toggle_focused_file_keeps_focus_within_visible_ids() {
        let visible_ids = ids(&["kick", "snare", "hat"]);
        let mut selection =
            FileSelectionModel::new(Some(String::from("kick")), HashSet::new(), false);

        let outcome = selection
            .toggle_focused(&visible_ids)
            .expect("focused file should toggle");

        assert_eq!(outcome.toggled_id, "kick");
        assert!(outcome.toggled_selected);
        assert_eq!(outcome.focused_id, "kick");
        assert_eq!(selection.focused_id(), Some("kick"));
        assert_eq!(selection.active_ids(), set(&["kick"]));
    }

    #[test]
    fn repeated_toggle_focused_file_selects_and_deselects_same_id() {
        let visible_ids = ids(&["kick", "snare", "hat"]);
        let mut selection =
            FileSelectionModel::new(Some(String::from("snare")), HashSet::new(), false);

        let first = selection
            .toggle_focused(&visible_ids)
            .expect("focused file should select");
        let second = selection
            .toggle_focused(&visible_ids)
            .expect("focused file should deselect");

        assert_eq!(first.toggled_id, "snare");
        assert!(first.toggled_selected);
        assert_eq!(second.toggled_id, "snare");
        assert!(!second.toggled_selected);
        assert_eq!(selection.focused_id(), Some("snare"));
        assert!(selection.selected_ids().is_empty());
        assert!(selection.explicit());
    }
}
