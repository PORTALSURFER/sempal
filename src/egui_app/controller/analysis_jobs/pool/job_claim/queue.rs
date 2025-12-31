use crate::egui_app::controller::analysis_jobs::db;
use super::dedup::DedupTracker;
use std::collections::VecDeque;
use std::sync::{Condvar, Mutex};
use std::sync::{atomic::AtomicBool, atomic::AtomicUsize, atomic::Ordering};
use std::time::Duration;

pub(in crate::egui_app::controller::analysis_jobs) struct DecodedQueue {
    queue: Mutex<VecDeque<DecodedWork>>,
    ready: Condvar,
    len: AtomicUsize,
    dedup: DedupTracker,
}

impl DecodedQueue {
    pub(in crate::egui_app::controller::analysis_jobs) fn new() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            ready: Condvar::new(),
            len: AtomicUsize::new(0),
            dedup: DedupTracker::new(),
        }
    }

    pub(super) fn try_mark_inflight(&self, job_id: i64) -> bool {
        self.dedup.try_mark_inflight(job_id)
    }

    pub(super) fn clear_inflight(&self, job_id: i64) {
        self.dedup.clear_inflight(job_id);
    }

    pub(super) fn push(&self, work: DecodedWork) -> bool {
        let mut guard = self.queue.lock().expect("decoded queue lock");
        if work.job.job_type == db::ANALYZE_SAMPLE_JOB_TYPE {
            if !self.dedup.mark_pending(work.job.id) {
                return false;
            }
        }
        guard.push_back(work);
        self.len.fetch_add(1, Ordering::Relaxed);
        self.ready.notify_one();
        true
    }

    #[cfg(test)]
    pub(super) fn pop(&self, shutdown: &AtomicBool) -> Option<DecodedWork> {
        let mut guard = self.queue.lock().expect("decoded queue lock");
        loop {
            if shutdown.load(Ordering::Relaxed) {
                return None;
            }
            if let Some(work) = guard.pop_front() {
                if work.job.job_type == db::ANALYZE_SAMPLE_JOB_TYPE {
                    self.dedup.clear_pending(work.job.id);
                }
                self.len.fetch_sub(1, Ordering::Relaxed);
                return Some(work);
            }
            let (next_guard, _) = self
                .ready
                .wait_timeout(guard, Duration::from_millis(50))
                .expect("decoded queue wait");
            guard = next_guard;
        }
    }

    pub(super) fn pop_batch(
        &self,
        shutdown: &AtomicBool,
        max: usize,
    ) -> (Vec<DecodedWork>, u64) {
        let mut guard = self.queue.lock().expect("decoded queue lock");
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
                return (batch, start.elapsed().as_millis() as u64);
            }
            let (next_guard, _) = self
                .ready
                .wait_timeout(guard, Duration::from_millis(50))
                .expect("decoded queue wait");
            guard = next_guard;
        }
    }

    pub(super) fn len(&self) -> usize {
        self.len.load(Ordering::Relaxed)
    }
}

pub(in crate::egui_app::controller::analysis_jobs) struct DecodedWork {
    pub(super) job: db::ClaimedJob,
    pub(super) outcome: DecodeOutcome,
}

pub(in crate::egui_app::controller::analysis_jobs) enum DecodeOutcome {
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
        let queue = DecodedQueue::new();
        assert!(queue.try_mark_inflight(42));
        assert!(!queue.try_mark_inflight(42));
        queue.clear_inflight(42);
        assert!(queue.try_mark_inflight(42));
    }

    #[test]
    fn push_dedups_pending_jobs() {
        let queue = DecodedQueue::new();
        assert!(queue.push(make_work(1)));
        assert!(!queue.push(make_work(1)));
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn pop_allows_reclaim_after_pending_cleared() {
        let queue = DecodedQueue::new();
        let shutdown = AtomicBool::new(false);
        assert!(queue.push(make_work(7)));
        assert!(queue.pop(&shutdown).is_some());
        assert!(queue.push(make_work(7)));
    }
}
