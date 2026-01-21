use super::dedup::DedupTracker;
use crate::egui_app::controller::library::analysis_jobs::db;
use crate::egui_app::controller::library::analysis_jobs::wakeup::ClaimWakeup;
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::sync::{atomic::AtomicBool, atomic::AtomicUsize, atomic::Ordering};
use std::time::Duration;
use tracing::warn;

/// Bounded queue of decoded analysis work with dedup tracking.
pub(crate) struct DecodedQueue {
    queue: Mutex<VecDeque<DecodedWork>>,
    ready: Condvar,
    len: AtomicUsize,
    max_size: usize,
    dedup: DedupTracker,
    claim_wakeup: Option<Arc<ClaimWakeup>>,
}

impl DecodedQueue {
    /// Creates a decoded queue with a fixed maximum size for backpressure.
    #[cfg(test)]
    pub(crate) fn new(max_size: usize) -> Self {
        Self::new_with_wakeup(max_size, None)
    }

    /// Creates a decoded queue with a wakeup to notify claimers when space frees.
    pub(crate) fn new_with_wakeup(
        max_size: usize,
        claim_wakeup: Option<Arc<ClaimWakeup>>,
    ) -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            ready: Condvar::new(),
            len: AtomicUsize::new(0),
            max_size: max_size.max(1),
            dedup: DedupTracker::new(),
            claim_wakeup,
        }
    }

    /// Marks a job inflight if it is not already being decoded.
    pub(crate) fn try_mark_inflight(&self, job_id: i64) -> bool {
        self.dedup.try_mark_inflight(job_id)
    }

    /// Clears the inflight marker for a job once it has been finalized.
    pub(crate) fn clear_inflight(&self, job_id: i64) {
        self.dedup.clear_inflight(job_id);
    }

    /// Pushes decoded work, blocking when at capacity until space is available.
    ///
    /// Returns false if the job is already pending or shutdown interrupts the wait.
    pub(crate) fn push(&self, work: DecodedWork, shutdown: &AtomicBool) -> bool {
        let mut guard = self.lock_queue();
        let mut marked_pending = false;
        if work.job.job_type == db::ANALYZE_SAMPLE_JOB_TYPE {
            if !self.dedup.mark_pending(work.job.id) {
                return false;
            }
            marked_pending = true;
        }
        let mut last_full_log = std::time::Instant::now() - Duration::from_secs(1);
        while guard.len() >= self.max_size {
            if shutdown.load(Ordering::Relaxed) {
                if marked_pending {
                    self.dedup.clear_pending(work.job.id);
                }
                return false;
            }
            if last_full_log.elapsed() >= Duration::from_secs(1) {
                warn!(
                    "Decoded queue full; depth={}, max={}",
                    guard.len(),
                    self.max_size
                );
                last_full_log = std::time::Instant::now();
            }
            let (next_guard, _) = self.wait_ready(guard);
            guard = next_guard;
        }
        guard.push_back(work);
        self.len.fetch_add(1, Ordering::Relaxed);
        self.ready.notify_one();
        true
    }

    #[cfg(test)]
    /// Pops a single decoded job, blocking until one is available.
    pub(crate) fn pop(&self, shutdown: &AtomicBool) -> Option<DecodedWork> {
        let mut guard = self.lock_queue();
        loop {
            if shutdown.load(Ordering::Relaxed) {
                return None;
            }
            if let Some(work) = guard.pop_front() {
                if work.job.job_type == db::ANALYZE_SAMPLE_JOB_TYPE {
                    self.dedup.clear_pending(work.job.id);
                }
                self.len.fetch_sub(1, Ordering::Relaxed);
                if let Some(wakeup) = self.claim_wakeup.as_ref() {
                    wakeup.notify();
                }
                self.ready.notify_one();
                return Some(work);
            }
            let (next_guard, _) = self.wait_ready(guard);
            guard = next_guard;
        }
    }

    /// Pops up to `max` decoded jobs for batch processing.
    pub(crate) fn pop_batch(&self, shutdown: &AtomicBool, max: usize) -> (Vec<DecodedWork>, u64) {
        let mut guard = self.lock_queue();
        let start = std::time::Instant::now();
        loop {
            if shutdown.load(Ordering::Relaxed) {
                return (Vec::new(), start.elapsed().as_millis() as u64);
            }
            if let Some(work) = guard.pop_front() {
                let mut batch = Vec::with_capacity(max.max(1));
                batch.push(work);
                self.len.fetch_sub(1, Ordering::Relaxed);
                while batch.len() < max {
                    if let Some(next) = guard.pop_front() {
                        batch.push(next);
                        self.len.fetch_sub(1, Ordering::Relaxed);
                    } else {
                        break;
                    }
                }
                {
                    for item in &batch {
                        if item.job.job_type == db::ANALYZE_SAMPLE_JOB_TYPE {
                            self.dedup.clear_pending(item.job.id);
                        }
                    }
                }
                self.ready.notify_all();
                if let Some(wakeup) = self.claim_wakeup.as_ref() {
                    wakeup.notify();
                }
                return (batch, start.elapsed().as_millis() as u64);
            }
            let (next_guard, _) = self.wait_ready(guard);
            guard = next_guard;
        }
    }

    /// Returns the maximum number of decoded jobs the queue can hold.
    pub(crate) fn max_size(&self) -> usize {
        self.max_size
    }

    /// Returns the current number of queued decoded jobs.
    pub(crate) fn len(&self) -> usize {
        self.len.load(Ordering::Relaxed)
    }

    fn lock_queue(&self) -> std::sync::MutexGuard<'_, VecDeque<DecodedWork>> {
        self.queue.lock().unwrap_or_else(|poisoned| {
            warn!("Decoded queue lock poisoned; recovering.");
            poisoned.into_inner()
        })
    }

    fn wait_ready<'a>(
        &self,
        guard: std::sync::MutexGuard<'a, VecDeque<DecodedWork>>,
    ) -> (
        std::sync::MutexGuard<'a, VecDeque<DecodedWork>>,
        std::sync::WaitTimeoutResult,
    ) {
        self.ready
            .wait_timeout(guard, Duration::from_millis(50))
            .unwrap_or_else(|poisoned| {
                warn!("Decoded queue condvar poisoned; recovering.");
                poisoned.into_inner()
            })
    }
}

/// A decoded job ready for analysis or finalization.
pub(crate) struct DecodedWork {
    pub(crate) job: db::ClaimedJob,
    pub(crate) outcome: DecodeOutcome,
}

/// Result of attempting to decode audio for analysis.
pub(crate) enum DecodeOutcome {
    Decoded(crate::analysis::audio::AnalysisAudio),
    Skipped {
        duration_seconds: f32,
        sample_rate: u32,
    },
    Failed(String),
    NotNeeded,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, mpsc};
    use std::time::Duration;

    fn make_job(id: i64) -> db::ClaimedJob {
        db::ClaimedJob {
            id,
            sample_id: format!("source::sample-{id}.wav"),
            content_hash: None,
            job_type: db::ANALYZE_SAMPLE_JOB_TYPE.to_string(),
            source_root: std::path::PathBuf::from("root"),
        }
    }

    fn make_work(id: i64) -> DecodedWork {
        DecodedWork {
            job: make_job(id),
            outcome: DecodeOutcome::NotNeeded,
        }
    }

    #[test]
    fn try_mark_inflight_blocks_duplicates() {
        let queue = DecodedQueue::new(4);
        assert!(queue.try_mark_inflight(42));
        assert!(!queue.try_mark_inflight(42));
        queue.clear_inflight(42);
        assert!(queue.try_mark_inflight(42));
    }

    #[test]
    fn push_dedups_pending_jobs() {
        let queue = DecodedQueue::new(4);
        let shutdown = AtomicBool::new(false);
        assert!(queue.push(make_work(1), &shutdown));
        assert!(!queue.push(make_work(1), &shutdown));
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn pop_allows_reclaim_after_pending_cleared() {
        let queue = DecodedQueue::new(4);
        let shutdown = AtomicBool::new(false);
        assert!(queue.push(make_work(7), &shutdown));
        assert!(queue.pop(&shutdown).is_some());
        assert!(queue.push(make_work(7), &shutdown));
    }

    #[test]
    fn poisoned_queue_lock_recovers_on_push() {
        let queue = DecodedQueue::new(4);
        let shutdown = AtomicBool::new(false);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = queue.queue.lock().unwrap();
            panic!("poison");
        }));
        assert!(queue.push(make_work(11), &shutdown));
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn push_blocks_until_space_is_available() {
        let queue = Arc::new(DecodedQueue::new(1));
        let shutdown = Arc::new(AtomicBool::new(false));
        assert!(queue.push(make_work(1), &shutdown));

        let (started_tx, started_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let queue_for_thread = Arc::clone(&queue);
        let shutdown_for_thread = Arc::clone(&shutdown);
        std::thread::spawn(move || {
            let _ = started_tx.send(());
            let queued = queue_for_thread.push(make_work(2), shutdown_for_thread.as_ref());
            let _ = done_tx.send(queued);
        });

        started_rx.recv_timeout(Duration::from_millis(200)).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        assert!(done_rx.try_recv().is_err());

        assert!(queue.pop(shutdown.as_ref()).is_some());
        assert!(done_rx.recv_timeout(Duration::from_millis(200)).unwrap());
    }
}
