use super::members::BrowserSampleContext;
use super::CollectionsController;
use std::path::PathBuf;

/// Shared move selection, validation, and focus data for collection moves.
pub(in crate::egui_app::controller::collections_controller) struct MovePlan {
    pub(in crate::egui_app::controller::collections_controller) rows: Vec<usize>,
    pub(in crate::egui_app::controller::collections_controller) contexts: Vec<BrowserSampleContext>,
    pub(in crate::egui_app::controller::collections_controller) last_error: Option<String>,
    pub(in crate::egui_app::controller::collections_controller) next_focus: Option<PathBuf>,
}

impl MovePlan {
    pub(in crate::egui_app::controller::collections_controller) fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

impl CollectionsController<'_> {
    /// Build a move plan from the browser selection, including validation and focus fallback.
    pub(in crate::egui_app::controller::collections_controller) fn build_browser_move_plan(
        &mut self,
    ) -> MovePlan {
        let rows = self.browser_selection_rows_for_move();
        let next_focus = self.next_browser_focus_path_after_move(&rows);
        let (contexts, last_error) = self.collect_browser_contexts(&rows);
        MovePlan {
            rows,
            contexts,
            last_error,
            next_focus,
        }
    }

    pub(in crate::egui_app::controller::collections_controller) fn browser_selection_rows_for_move(
        &mut self,
    ) -> Vec<usize> {
        let mut rows: Vec<usize> = self
            .ui
            .browser
            .selected_paths
            .clone()
            .iter()
            .filter_map(|path| self.visible_row_for_path(path))
            .collect();
        if rows.is_empty() {
            if let Some(row) = self
                .focused_browser_row()
                .or_else(|| self.primary_visible_row_for_browser_selection())
            {
                rows.push(row);
            }
        }
        rows.sort_unstable();
        rows.dedup();
        rows
    }

    pub(in crate::egui_app::controller::collections_controller) fn next_browser_focus_path_after_move(
        &mut self,
        rows: &[usize],
    ) -> Option<PathBuf> {
        if rows.is_empty() || self.ui.browser.visible.len() == 0 {
            return None;
        }
        let mut sorted = rows.to_vec();
        sorted.sort_unstable();
        let highest = sorted.last().copied()?;
        let first = sorted.first().copied().unwrap_or(highest);
        let after = highest
            .checked_add(1)
            .and_then(|idx| self.ui.browser.visible.get(idx))
            .and_then(|entry_idx| self.wav_entry(entry_idx))
            .map(|entry| entry.relative_path.clone());
        if after.is_some() {
            return after;
        }
        first
            .checked_sub(1)
            .and_then(|idx| self.ui.browser.visible.get(idx))
            .and_then(|entry_idx| self.wav_entry(entry_idx))
            .map(|entry| entry.relative_path.clone())
    }

    /// Resolve browser rows into unique move contexts, tracking the last error.
    pub(super) fn collect_browser_contexts(
        &mut self,
        rows: &[usize],
    ) -> (Vec<BrowserSampleContext>, Option<String>) {
        let mut contexts = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut last_error = None;
        for row in rows {
            match self.resolve_browser_sample(*row) {
                Ok(ctx) => {
                    if seen.insert(ctx.entry.relative_path.clone()) {
                        contexts.push(BrowserSampleContext {
                            source: ctx.source,
                            entry: ctx.entry,
                        });
                    }
                }
                Err(err) => last_error = Some(err),
            }
        }
        (contexts, last_error)
    }
}

#[cfg(test)]
mod tests {
    use crate::egui_app::controller::test_support::{prepare_with_source_and_wav_entries, sample_entry};
    use crate::sample_sources::SampleTag;

    fn controller_with_entries(names: &[&str]) -> crate::egui_app::controller::EguiController {
        let entries = names
            .iter()
            .map(|name| sample_entry(name, SampleTag::Neutral))
            .collect();
        let (controller, _source) = prepare_with_source_and_wav_entries(entries);
        controller
    }

    #[test]
    fn browser_selection_rows_for_move_empty_selection() {
        let mut controller = controller_with_entries(&["one.wav", "two.wav"]);
        controller.ui.browser.selected_paths.clear();
        controller.ui.browser.selected_visible = None;

        let rows = controller
            .collections_ctrl()
            .browser_selection_rows_for_move();

        assert!(rows.is_empty());
    }

    #[test]
    fn browser_selection_rows_for_move_uses_focused_row() {
        let mut controller = controller_with_entries(&["one.wav", "two.wav", "three.wav"]);
        controller.ui.browser.selected_paths.clear();
        controller.ui.browser.selected_visible = Some(1);

        let rows = controller
            .collections_ctrl()
            .browser_selection_rows_for_move();

        assert_eq!(rows, vec![1]);
    }

    #[test]
    fn next_browser_focus_path_after_move_falls_back_before_last_row() {
        let mut controller = controller_with_entries(&["one.wav", "two.wav", "three.wav"]);
        let expected = {
            let entry_index = controller
                .visible_browser_index(1)
                .expect("visible row");
            controller
                .wav_entry(entry_index)
                .expect("wav entry")
                .relative_path
                .clone()
        };

        let next_focus = controller
            .collections_ctrl()
            .next_browser_focus_path_after_move(&[2]);

        assert_eq!(next_focus, Some(expected));
    }
}
