use super::*;
use crate::egui_app::state::{FocusContext, SampleBrowserActionPrompt};
use crate::egui_app::ui::style::StatusTone;
use crate::egui_app::view_model;
use std::path::Path;

impl EguiController {
    pub(in crate::egui_app::controller) fn visible_row_for_path(
        &self,
        path: &Path,
    ) -> Option<usize> {
        let entry_index = self.wav_entries.lookup.get(path)?;
        self.ui
            .browser
            .visible
            .iter()
            .position(|idx| idx == entry_index)
    }

    fn set_single_browser_selection(&mut self, path: &Path) {
        self.ui.browser.selected_paths.clear();
        self.ui.browser.selected_paths.push(path.to_path_buf());
    }

    fn toggle_browser_selection(&mut self, path: &Path) {
        if let Some(pos) = self
            .ui
            .browser
            .selected_paths
            .iter()
            .position(|p| p == path)
        {
            self.ui.browser.selected_paths.remove(pos);
        } else {
            self.ui.browser.selected_paths.push(path.to_path_buf());
        }
    }

    fn extend_browser_selection_to(&mut self, target_visible: usize, additive: bool) {
        if self.ui.browser.visible.is_empty() {
            return;
        }
        let max_row = self.ui.browser.visible.len().saturating_sub(1);
        let target_visible = target_visible.min(max_row);
        let anchor = self
            .ui
            .browser
            .selection_anchor_visible
            .or(self.ui.browser.selected_visible)
            .unwrap_or(target_visible)
            .min(max_row);
        let start = anchor.min(target_visible);
        let end = anchor.max(target_visible);
        if !additive {
            self.ui.browser.selected_paths.clear();
        }
        for row in start..=end {
            if let Some(path) = self.browser_path_for_visible(row)
                && !self.ui.browser.selected_paths.iter().any(|p| p == &path)
            {
                self.ui.browser.selected_paths.push(path);
            }
        }
        self.ui.browser.selection_anchor_visible = Some(anchor);
    }

    /// Focus a browser row and update multi-selection state.
    pub fn focus_browser_row(&mut self, visible_row: usize) {
        self.apply_browser_selection(visible_row, SelectionAction::Replace);
    }

    /// Focus a browser row without mutating the multi-selection set.
    pub fn focus_browser_row_only(&mut self, visible_row: usize) {
        let Some(path) = self.browser_path_for_visible(visible_row) else {
            return;
        };
        self.ui.collections.selected_sample = None;
        self.focus_browser_context();
        self.ui.browser.autoscroll = true;
        self.ui.browser.selection_anchor_visible = Some(visible_row);
        self.select_wav_by_path_with_rebuild(&path, true);
    }

    pub(crate) fn start_browser_rename(&mut self) {
        let Some(path) = self.focused_browser_path() else {
            self.set_status("Focus a sample to rename it", StatusTone::Info);
            return;
        };
        let default = view_model::sample_display_label(&path);
        self.focus_browser_context();
        self.ui.browser.pending_action = Some(SampleBrowserActionPrompt::Rename {
            target: path,
            name: default,
        });
        self.ui.browser.rename_focus_requested = true;
    }

    pub(crate) fn cancel_browser_rename(&mut self) {
        self.ui.browser.pending_action = None;
        self.ui.browser.rename_focus_requested = false;
    }

    pub(crate) fn apply_pending_browser_rename(&mut self) {
        let action = self.ui.browser.pending_action.clone();
        if let Some(SampleBrowserActionPrompt::Rename { target, name }) = action {
            let Some(row) = self.visible_row_for_path(&target) else {
                self.cancel_browser_rename();
                self.set_status("Sample not found to rename", StatusTone::Info);
                return;
            };
            match self.rename_browser_sample(row, &name) {
                Ok(()) => {
                    self.cancel_browser_rename();
                }
                Err(err) => self.set_status(err, StatusTone::Error),
            }
        }
    }

    /// Toggle whether a visible browser row is included in the multi-selection set.
    pub fn toggle_browser_row_selection(&mut self, visible_row: usize) {
        self.apply_browser_selection(visible_row, SelectionAction::Toggle);
    }

    /// Extend the multi-selection range to a visible browser row (replaces the selection set).
    pub fn extend_browser_selection_to_row(&mut self, visible_row: usize) {
        self.apply_browser_selection(visible_row, SelectionAction::Extend { additive: false });
    }

    /// Extend the multi-selection range to a visible browser row (adds to the selection set).
    pub fn add_range_browser_selection(&mut self, visible_row: usize) {
        self.apply_browser_selection(visible_row, SelectionAction::Extend { additive: true });
    }

    /// Toggle the focused sample's inclusion in the browser multi-selection set.
    pub fn toggle_focused_selection(&mut self) {
        let Some(path) = self.wav_selection.selected_wav.clone() else {
            return;
        };
        if let Some(row) = self.ui.browser.selected_visible
            && self.ui.browser.selection_anchor_visible.is_none()
        {
            self.ui.browser.selection_anchor_visible = Some(row);
        }
        self.toggle_browser_selection(&path);
        self.rebuild_browser_lists();
    }

    /// Clear the multi-selection set.
    pub fn clear_browser_selection(&mut self) {
        if self.ui.browser.selected_paths.is_empty() {
            return;
        }
        self.ui.browser.selected_paths.clear();
        self.ui.browser.selection_anchor_visible = None;
        self.rebuild_browser_lists();
    }

    /// Reveal the given sample browser item in the OS file explorer.
    pub fn reveal_browser_sample_in_file_explorer(&mut self, relative_path: &Path) {
        let Some(source) = self.current_source() else {
            self.set_status("Select a source first", StatusTone::Info);
            return;
        };
        let absolute = source.root.join(relative_path);
        if !absolute.exists() {
            self.set_status(
                format!("File missing: {}", absolute.display()),
                StatusTone::Warning,
            );
            return;
        }
        if let Err(err) = super::super::os_explorer::reveal_in_file_explorer(&absolute) {
            self.set_status(err, StatusTone::Error);
        }
    }

    /// Clear sample browser focus/selection when another surface takes focus.
    pub fn blur_browser_focus(&mut self) {
        if matches!(self.ui.focus.context, FocusContext::Waveform) {
            return;
        }
        if self.ui.browser.selected.is_none()
            && self.ui.browser.selected_visible.is_none()
            && self.ui.browser.selection_anchor_visible.is_none()
            && self.ui.browser.selected_paths.is_empty()
        {
            return;
        }
        self.ui.browser.autoscroll = false;
        self.ui.browser.selected = None;
        self.ui.browser.selected_visible = None;
        self.ui.browser.selection_anchor_visible = None;
        self.ui.browser.selected_paths.clear();
        self.rebuild_browser_lists();
    }

    fn apply_browser_selection(&mut self, visible_row: usize, action: SelectionAction) {
        let Some(path) = self.browser_path_for_visible(visible_row) else {
            return;
        };
        self.ui.collections.selected_sample = None;
        self.focus_browser_context();
        self.ui.browser.autoscroll = true;
        match action {
            SelectionAction::Replace => {
                self.ui.browser.selection_anchor_visible = Some(visible_row);
                self.set_single_browser_selection(&path);
            }
            SelectionAction::Toggle => {
                let anchor = self
                    .ui
                    .browser
                    .selection_anchor_visible
                    .or(self.ui.browser.selected_visible)
                    .unwrap_or(visible_row);
                self.ui.browser.selection_anchor_visible = Some(anchor);
                if self.ui.browser.selected_paths.is_empty()
                    && anchor != visible_row
                    && let Some(anchor_path) = self.browser_path_for_visible(anchor)
                    && !self
                        .ui
                        .browser
                        .selected_paths
                        .iter()
                        .any(|p| p == &anchor_path)
                {
                    self.ui.browser.selected_paths.push(anchor_path);
                }
                self.toggle_browser_selection(&path);
            }
            SelectionAction::Extend { additive } => {
                self.extend_browser_selection_to(visible_row, additive);
            }
        }
        self.select_wav_by_path_with_rebuild(&path, false);
        self.rebuild_browser_lists();
    }

    /// Return the set of action rows for a primary row (multi-select aware).
    pub fn action_rows_from_primary(&self, primary_visible_row: usize) -> Vec<usize> {
        let mut rows: Vec<usize> = self
            .ui
            .browser
            .selected_paths
            .iter()
            .filter_map(|path| self.visible_row_for_path(path))
            .collect();
        if !rows.contains(&primary_visible_row) {
            rows.push(primary_visible_row);
        }
        rows.sort_unstable();
        rows.dedup();
        rows
    }
}

#[derive(Clone, Copy)]
enum SelectionAction {
    Replace,
    Toggle,
    Extend { additive: bool },
}
