use std::fmt;

/// Persistent aggregate progress for analysis jobs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::egui_app::controller) struct AnalysisProgress {
    pub(in crate::egui_app::controller) pending: usize,
    pub(in crate::egui_app::controller) running: usize,
    pub(in crate::egui_app::controller) done: usize,
    pub(in crate::egui_app::controller) failed: usize,
}

impl AnalysisProgress {
    pub(in crate::egui_app::controller) fn total(&self) -> usize {
        self.pending + self.running + self.done + self.failed
    }

    pub(in crate::egui_app::controller) fn completed(&self) -> usize {
        self.done + self.failed
    }
}

impl fmt::Display for AnalysisProgress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "pending={} running={} done={} failed={}",
            self.pending, self.running, self.done, self.failed
        )
    }
}

/// Controller messages emitted by the background analysis system.
#[derive(Clone, Debug)]
pub(in crate::egui_app::controller) enum AnalysisJobMessage {
    /// Queue counts changed (either due to enqueue or workers making progress).
    Progress(AnalysisProgress),
    /// An enqueue job finished, including how many rows were inserted.
    EnqueueFinished { inserted: usize, progress: AnalysisProgress },
    /// An enqueue job failed.
    EnqueueFailed(String),
}
