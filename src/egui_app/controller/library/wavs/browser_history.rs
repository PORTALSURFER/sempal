use super::*;
use std::path::Path;

impl EguiController {
    pub(crate) fn focus_previous_sample_history(&mut self) {
        if self.history.focus_history.entries.is_empty() {
            return;
        }
        let current = self
            .history
            .focus_history
            .cursor
            .unwrap_or_else(|| self.history.focus_history.entries.len().saturating_sub(1));
        if current == 0 {
            self.history.focus_history.cursor = Some(0);
            return;
        }
        let target = current - 1;
        self.history.focus_history.cursor = Some(target);
        if let Some(entry) = self.history.focus_history.entries.get(target).cloned() {
            focus_history_entry(self, entry);
        }
    }

    pub(crate) fn focus_next_sample_history(&mut self) {
        if self.history.focus_history.entries.is_empty() {
            return;
        }
        let current = self
            .history
            .focus_history
            .cursor
            .unwrap_or_else(|| self.history.focus_history.entries.len().saturating_sub(1));
        let last = self.history.focus_history.entries.len().saturating_sub(1);
        if current >= last {
            self.history.focus_history.cursor = Some(last);
            return;
        }
        let target = current + 1;
        self.history.focus_history.cursor = Some(target);
        if let Some(entry) = self.history.focus_history.entries.get(target).cloned() {
            focus_history_entry(self, entry);
        }
    }

    pub(crate) fn record_focus_history(&mut self, path: &Path) {
        if self.history.focus_history.suspend_push {
            return;
        }
        let Some(source_id) = self.selection_state.ctx.selected_source.clone() else {
            return;
        };
        let entry = FocusHistoryEntry {
            source_id,
            relative_path: path.to_path_buf(),
        };
        let history = &mut self.history.focus_history;
        if let Some(cursor) = history.cursor
            && history
                .entries
                .get(cursor)
                .is_some_and(|current| current == &entry)
        {
            return;
        }
        if let Some(cursor) = history.cursor
            && cursor + 1 < history.entries.len()
        {
            history.entries.truncate(cursor + 1);
        }
        if history.entries.back().is_some_and(|last| last == &entry) {
            history.cursor = Some(history.entries.len().saturating_sub(1));
            return;
        }
        history.entries.push_back(entry);
        if history.entries.len() > super::super::FOCUS_HISTORY_LIMIT {
            history.entries.pop_front();
            if let Some(cursor) = history.cursor {
                history.cursor = Some(cursor.saturating_sub(1));
            }
        }
        history.cursor = Some(history.entries.len().saturating_sub(1));
    }
}

fn focus_history_entry(controller: &mut EguiController, entry: FocusHistoryEntry) {
    controller.history.focus_history.suspend_push = true;
    controller.focus_browser_context();
    if controller.selection_state.ctx.selected_source.as_ref() != Some(&entry.source_id) {
        controller
            .runtime
            .jobs
            .set_pending_select_path(Some(entry.relative_path.clone()));
        controller.select_source_internal(Some(entry.source_id), Some(entry.relative_path));
        controller.history.focus_history.suspend_push = false;
        return;
    }
    if let Some(row) = controller.visible_row_for_path(&entry.relative_path) {
        controller.focus_browser_row_only(row);
    } else {
        controller.select_wav_by_path(&entry.relative_path);
    }
    controller.history.focus_history.suspend_push = false;
}
