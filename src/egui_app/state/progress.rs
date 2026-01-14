/// Modal progress indicator for slow tasks.
/// Identifies the long-running task responsible for updating the progress overlay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgressTaskKind {
    /// Moving files to trash.
    TrashMove,
    /// Loading a waveform from disk.
    WavLoad,
    /// Scanning a sample source.
    Scan,
    /// Running background analysis jobs.
    Analysis,
    /// Normalizing audio samples.
    Normalization,
}

use std::time::Instant;

/// UI state for the progress overlay and its counters.
#[derive(Clone, Debug, Default)]
pub struct ProgressOverlayState {
    /// Whether the overlay is visible.
    pub visible: bool,
    /// When true, the modal overlay is rendered (otherwise progress is status-bar only).
    pub modal: bool,
    /// The task currently driving the progress overlay (when visible).
    pub task: Option<ProgressTaskKind>,
    /// Title text for the overlay.
    pub title: String,
    /// Optional detail text for the overlay.
    pub detail: Option<String>,
    /// Completed steps.
    pub completed: usize,
    /// Total steps.
    pub total: usize,
    /// Whether cancel is allowed.
    pub cancelable: bool,
    /// Whether the user requested cancellation.
    pub cancel_requested: bool,
    /// Last time the overlay was updated.
    pub last_update_at: Option<Instant>,
    /// Last time progress advanced.
    pub last_progress_at: Option<Instant>,
    /// Optional analysis progress snapshot.
    pub analysis: Option<AnalysisProgressSnapshot>,
}

/// Snapshot of analysis progress for display.
#[derive(Clone, Debug)]
pub struct AnalysisProgressSnapshot {
    /// Number of pending jobs.
    pub pending: usize,
    /// Number of running jobs.
    pub running: usize,
    /// Number of failed jobs.
    pub failed: usize,
    /// Completed samples count.
    pub samples_completed: usize,
    /// Total samples to process.
    pub samples_total: usize,
    /// Snapshot of running jobs.
    pub running_jobs: Vec<RunningJobSnapshot>,
    /// Staleness threshold in seconds.
    pub stale_after_secs: Option<i64>,
}

/// Summary of a running job heartbeat for display.
#[derive(Clone, Debug)]
pub struct RunningJobSnapshot {
    /// Human-readable job label.
    pub label: String,
    /// Last heartbeat timestamp, epoch seconds.
    pub last_heartbeat_at: Option<i64>,
    /// Whether the job appears stalled.
    pub possibly_stalled: bool,
}

impl RunningJobSnapshot {
    /// Build a snapshot and mark it stalled when the heartbeat is stale.
    pub fn from_heartbeat(
        label: String,
        last_heartbeat_at: Option<i64>,
        stale_after_secs: Option<i64>,
        now_epoch: Option<i64>,
    ) -> Self {
        let possibly_stalled = match (last_heartbeat_at, stale_after_secs, now_epoch) {
            (Some(heartbeat), Some(stale_after), Some(now)) => {
                now.saturating_sub(heartbeat) >= stale_after
            }
            _ => false,
        };
        Self {
            label,
            last_heartbeat_at,
            possibly_stalled,
        }
    }
}

impl ProgressOverlayState {
    /// Create and show a progress overlay with the provided title and total step count.
    pub fn new(
        task: ProgressTaskKind,
        title: impl Into<String>,
        total: usize,
        cancelable: bool,
    ) -> Self {
        let now = Instant::now();
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
            last_update_at: Some(now),
            last_progress_at: Some(now),
            analysis: None,
        }
    }

    /// Reset the overlay back to its default (hidden) state.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Update the detail text and refresh the timestamp.
    pub fn set_detail(&mut self, detail: Option<String>) {
        self.detail = detail;
        self.last_update_at = Some(Instant::now());
    }

    /// Update total/completed counts and refresh timestamps.
    pub fn set_counts(&mut self, total: usize, completed: usize) {
        if self.total != total || self.completed != completed {
            self.last_progress_at = Some(Instant::now());
        }
        self.total = total;
        self.completed = completed;
        self.last_update_at = Some(Instant::now());
    }

    /// Update the analysis progress snapshot.
    pub fn set_analysis_snapshot(&mut self, snapshot: Option<AnalysisProgressSnapshot>) {
        self.analysis = snapshot;
        self.last_update_at = Some(Instant::now());
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
    use super::{ProgressOverlayState, ProgressTaskKind, RunningJobSnapshot};

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

    #[test]
    fn running_job_marks_stale_heartbeat() {
        let snapshot =
            RunningJobSnapshot::from_heartbeat("job".to_string(), Some(10), Some(5), Some(20));
        assert!(snapshot.possibly_stalled);

        let snapshot =
            RunningJobSnapshot::from_heartbeat("job".to_string(), Some(18), Some(5), Some(20));
        assert!(!snapshot.possibly_stalled);
    }
}
