//! Dedup tracking for analysis job claims.

use std::collections::HashSet;
use std::sync::Mutex;

/// Tracks inflight and pending jobs to prevent duplicate work.
pub(super) struct DedupTracker {
    pending_jobs: Mutex<HashSet<i64>>,
    inflight_jobs: Mutex<HashSet<i64>>,
}

impl DedupTracker {
    /// Creates a new dedup tracker.
    pub(super) fn new() -> Self {
        Self {
            pending_jobs: Mutex::new(HashSet::new()),
            inflight_jobs: Mutex::new(HashSet::new()),
        }
    }

    /// Marks a job inflight if it is not already inflight.
    pub(super) fn try_mark_inflight(&self, job_id: i64) -> bool {
        let mut inflight = self
            .inflight_jobs
            .lock()
            .expect("decoded queue inflight lock");
        if inflight.contains(&job_id) {
            return false;
        }
        inflight.insert(job_id);
        true
    }

    /// Clears an inflight marker for a job.
    pub(super) fn clear_inflight(&self, job_id: i64) {
        let mut inflight = self
            .inflight_jobs
            .lock()
            .expect("decoded queue inflight lock");
        inflight.remove(&job_id);
    }

    /// Marks a job pending if it has not already been queued.
    pub(super) fn mark_pending(&self, job_id: i64) -> bool {
        let mut pending = self
            .pending_jobs
            .lock()
            .expect("decoded queue pending lock");
        if pending.contains(&job_id) {
            return false;
        }
        pending.insert(job_id);
        true
    }

    /// Clears a pending marker for a job.
    pub(super) fn clear_pending(&self, job_id: i64) {
        let mut pending = self
            .pending_jobs
            .lock()
            .expect("decoded queue pending lock");
        pending.remove(&job_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mark_pending_dedups_and_allows_reclaim() {
        let dedup = DedupTracker::new();
        assert!(dedup.mark_pending(17));
        assert!(!dedup.mark_pending(17));
        dedup.clear_pending(17);
        assert!(dedup.mark_pending(17));
    }

    #[test]
    fn inflight_prevents_duplicates_until_cleared() {
        let dedup = DedupTracker::new();
        assert!(dedup.try_mark_inflight(42));
        assert!(!dedup.try_mark_inflight(42));
        dedup.clear_inflight(42);
        assert!(dedup.try_mark_inflight(42));
    }
}
