use std::fmt;

/// Persistent aggregate progress for analysis jobs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct AnalysisProgress {
    /// Job-level counts.
    pub(crate) pending: usize,
    pub(crate) running: usize,
    pub(crate) done: usize,
    pub(crate) failed: usize,
    /// Unique-sample counts derived from job rows.
    pub(crate) samples_total: usize,
    pub(crate) samples_pending_or_running: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct RunningJobInfo {
    pub(crate) sample_id: String,
    pub(crate) last_heartbeat_at: Option<i64>,
}

impl AnalysisProgress {
    pub(crate) fn total(&self) -> usize {
        self.pending + self.running + self.done + self.failed
    }

    pub(crate) fn completed(&self) -> usize {
        self.done + self.failed
    }

    pub(crate) fn samples_completed(&self) -> usize {
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
pub(crate) enum AnalysisJobMessage {
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
    /// Duration metadata was updated for a source.
    DurationsUpdated {
        source_id: crate::sample_sources::SourceId,
        updated: usize,
    },
}
