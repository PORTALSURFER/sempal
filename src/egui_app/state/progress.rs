/// Modal progress indicator for slow tasks.
/// Identifies the long-running task responsible for updating the progress overlay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgressTaskKind {
    TrashMove,
    WavLoad,
    Scan,
    Analysis,
    ModelTraining,
}

#[derive(Clone, Debug, Default)]
pub struct ProgressOverlayState {
    pub visible: bool,
    /// When true, the modal overlay is rendered (otherwise progress is status-bar only).
    pub modal: bool,
    /// The task currently driving the progress overlay (when visible).
    pub task: Option<ProgressTaskKind>,
    pub title: String,
    pub detail: Option<String>,
    pub completed: usize,
    pub total: usize,
    pub cancelable: bool,
    pub cancel_requested: bool,
}

impl ProgressOverlayState {
    /// Create and show a progress overlay with the provided title and total step count.
    pub fn new(
        task: ProgressTaskKind,
        title: impl Into<String>,
        total: usize,
        cancelable: bool,
    ) -> Self {
        Self {
            visible: true,
            modal: true,
            task: Some(task),
            title: title.into(),
            detail: None,
            completed: 0,
            total,
            cancelable,
            cancel_requested: false,
        }
    }

    /// Reset the overlay back to its default (hidden) state.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Return completion in the range `[0.0, 1.0]`.
    pub fn fraction(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (self.completed as f32 / self.total as f32).clamp(0.0, 1.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ProgressOverlayState, ProgressTaskKind};

    #[test]
    fn progress_fraction_handles_zero_total() {
        let progress = ProgressOverlayState::new(ProgressTaskKind::TrashMove, "Task", 0, false);
        assert_eq!(progress.fraction(), 0.0);
    }

    #[test]
    fn progress_reset_clears_visibility() {
        let mut progress = ProgressOverlayState::new(ProgressTaskKind::TrashMove, "Task", 2, true);
        progress.completed = 3;
        assert!(progress.fraction() <= 1.0);
        progress.reset();
        assert!(!progress.visible);
        assert_eq!(progress.task, None);
        assert_eq!(progress.completed, 0);
        assert_eq!(progress.total, 0);
    }
}
