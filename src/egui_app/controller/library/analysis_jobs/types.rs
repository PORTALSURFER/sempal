use std::fmt;

/// Persistent aggregate progress for analysis jobs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::egui_app::controller) struct AnalysisProgress {
    /// Job-level counts.
    pub(in crate::egui_app::controller) pending: usize,
    pub(in crate::egui_app::controller) running: usize,
    pub(in crate::egui_app::controller) done: usize,
    pub(in crate::egui_app::controller) failed: usize,
    /// Unique-sample counts derived from job rows.
    pub(in crate::egui_app::controller) samples_total: usize,
    pub(in crate::egui_app::controller) samples_pending_or_running: usize,
}

#[derive(Clone, Debug)]
pub(in crate::egui_app::controller) struct RunningJobInfo {
    pub(in crate::egui_app::controller) sample_id: String,
    pub(in crate::egui_app::controller) last_heartbeat_at: Option<i64>,
}

impl AnalysisProgress {
    pub(in crate::egui_app::controller) fn total(&self) -> usize {
        self.pending + self.running + self.done + self.failed
    }

    pub(in crate::egui_app::controller) fn completed(&self) -> usize {
        self.done + self.failed
    }

    pub(in crate::egui_app::controller) fn samples_completed(&self) -> usize {
        self.samples_total
            .saturating_sub(self.samples_pending_or_running)
    }
}

impl fmt::Display for AnalysisProgress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "pending={} running={} done={} failed={} samples_total={} samples_pending_or_running={}",
            self.pending,
            self.running,
            self.done,
            self.failed,
            self.samples_total,
            self.samples_pending_or_running
        )
    }
}

/// Controller messages emitted by the background analysis system.
#[derive(Clone, Debug)]
pub(in crate::egui_app::controller) enum AnalysisJobMessage {
    /// Queue counts changed (either due to enqueue or workers making progress).
    Progress {
        source_id: Option<crate::sample_sources::SourceId>,
        progress: AnalysisProgress,
    },
    /// An enqueue job finished, including how many rows were inserted.
    EnqueueFinished {
        inserted: usize,
        progress: AnalysisProgress,
    },
    /// An enqueue job failed.
    EnqueueFailed(String),
    /// Embedding backfill enqueue finished.
    EmbeddingBackfillEnqueueFinished {
        inserted: usize,
        progress: AnalysisProgress,
    },
    /// Embedding backfill enqueue failed.
    EmbeddingBackfillEnqueueFailed(String),
}
