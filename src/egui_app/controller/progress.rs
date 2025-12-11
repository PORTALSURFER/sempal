use super::*;

impl EguiController {
    /// Show a modal progress overlay with an optional cancel affordance.
    pub(crate) fn show_progress(
        &mut self,
        title: impl Into<String>,
        total: usize,
        cancelable: bool,
    ) {
        self.ui.progress = ProgressOverlayState::new(title, total, cancelable);
    }

    /// Update the current progress detail label without changing counts.
    pub(crate) fn update_progress_detail(&mut self, detail: impl Into<String>) {
        if self.ui.progress.visible {
            self.ui.progress.detail = Some(detail.into());
        }
    }

    /// Advance the progress counter by one step.
    pub(crate) fn advance_progress(&mut self) {
        if !self.ui.progress.visible {
            return;
        }
        let total = self.ui.progress.total;
        let next = self.ui.progress.completed.saturating_add(1);
        self.ui.progress.completed = if total == 0 { next } else { next.min(total) };
    }

    /// Clear any active progress overlay.
    pub(crate) fn clear_progress(&mut self) {
        self.ui.progress.reset();
    }

    /// Request cancellation of the active progress task.
    pub(crate) fn request_progress_cancel(&mut self) {
        if self.ui.progress.cancelable {
            self.ui.progress.cancel_requested = true;
        }
    }
}
