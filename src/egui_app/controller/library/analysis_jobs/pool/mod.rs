mod job_claim;
mod job_cleanup;
mod job_execution;
mod job_progress;
mod progress_cache;

use crate::sample_sources::SourceId;
use progress_cache::ProgressCache;
#[cfg(not(test))]
use std::collections::HashSet;
use std::mem;
#[cfg(not(test))]
use std::sync::Mutex;
use std::sync::{
    Arc, RwLock,
    atomic::AtomicU32,
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
};
use std::thread::JoinHandle;
use tracing::info;

/// Long-lived worker pool that claims and processes analysis jobs from the library database.
pub(crate) struct AnalysisWorkerPool {
    cancel: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    pause_claiming: Arc<AtomicBool>,
    use_cache: Arc<AtomicBool>,
    allowed_source_ids: Arc<RwLock<Option<std::collections::HashSet<SourceId>>>>,
    max_duration_bits: Arc<AtomicU32>,
    analysis_sample_rate: Arc<AtomicU32>,
    analysis_version_override: Arc<RwLock<Option<String>>>,
    worker_count_override: Arc<AtomicU32>,
    #[cfg_attr(test, allow(dead_code))]
    decode_worker_count_override: Arc<AtomicU32>,
    _progress_cache: Arc<RwLock<ProgressCache>>,
    threads: Vec<JoinHandle<()>>,
}

impl AnalysisWorkerPool {
    pub(crate) fn new() -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            shutdown: Arc::new(AtomicBool::new(false)),
            pause_claiming: Arc::new(AtomicBool::new(false)),
            use_cache: Arc::new(AtomicBool::new(true)),
            allowed_source_ids: Arc::new(RwLock::new(None)),
            max_duration_bits: Arc::new(AtomicU32::new(30.0f32.to_bits())),
            analysis_sample_rate: Arc::new(AtomicU32::new(
                crate::analysis::audio::ANALYSIS_SAMPLE_RATE,
            )),
            analysis_version_override: Arc::new(RwLock::new(None)),
            worker_count_override: Arc::new(AtomicU32::new(0)),
            decode_worker_count_override: Arc::new(AtomicU32::new(0)),
            _progress_cache: Arc::new(RwLock::new(ProgressCache::default())),
            threads: Vec::new(),
        }
    }
    pub(crate) fn set_max_analysis_duration_seconds(&self, value: f32) {
        let clamped = value.clamp(0.0, 60.0 * 60.0);
        self.max_duration_bits
            .store(clamped.to_bits(), Ordering::Relaxed);
    }

    pub(crate) fn set_worker_count(&self, value: u32) {
        let previous = self.worker_count_override.swap(value, Ordering::Relaxed);
        if previous != value {
            tracing::debug!("Analysis worker count override set to {}", value);
        }
    }

    #[cfg_attr(test, allow(dead_code))]
    #[allow(dead_code)]
    pub(crate) fn set_decode_worker_count(&self, value: u32) {
        self.decode_worker_count_override
            .store(value, Ordering::Relaxed);
    }

    pub(crate) fn set_analysis_sample_rate(&self, value: u32) {
        let clamped = value.max(1);
        self.analysis_sample_rate.store(clamped, Ordering::Relaxed);
    }

    pub(crate) fn set_analysis_cache_enabled(&self, enabled: bool) {
        self.use_cache.store(enabled, Ordering::Relaxed);
    }

    pub(crate) fn set_analysis_version_override(
        &self,
        value: Option<String>,
    ) {
        if let Ok(mut guard) = self.analysis_version_override.write() {
            *guard = value;
        }
    }

    pub(crate) fn set_allowed_sources(
        &self,
        sources: Option<Vec<SourceId>>,
    ) {
        if let Ok(mut guard) = self.allowed_source_ids.write() {
            let next = sources.map(|ids| ids.into_iter().collect::<std::collections::HashSet<_>>());
            let count = next.as_ref().map(|ids| ids.len()).unwrap_or(0);
            *guard = next;
            if count == 0 {
                info!("Analysis sources set to all available sources");
            } else {
                info!("Analysis sources restricted to {} source(s)", count);
            }
        }
    }

    pub(crate) fn pause_claiming(&self) {
        let previous = self.pause_claiming.swap(true, Ordering::Relaxed);
        if !previous {
            tracing::debug!("Analysis job claiming paused");
        }
    }

    pub(crate) fn resume_claiming(&self) {
        let previous = self.pause_claiming.swap(false, Ordering::Relaxed);
        if previous {
            tracing::debug!("Analysis job claiming resumed");
        }
    }

    pub(crate) fn start(
        &mut self,
        message_tx: Sender<crate::egui_app::controller::jobs::JobMessage>,
    ) {
        let _ = &message_tx;
        if !self.threads.is_empty() {
            return;
        }
        #[cfg(not(test))]
        {
            let worker_count = job_claim::worker_count_with_override(
                self.worker_count_override.load(Ordering::Relaxed),
            );
            let decode_workers = job_claim::decode_worker_count_with_override(
                worker_count,
                self.decode_worker_count_override.load(Ordering::Relaxed),
            );
            let embedding_batch_max = crate::analysis::similarity::SIMILARITY_BATCH_MAX;
            let decode_queue_target =
                job_claim::decode_queue_target(embedding_batch_max, worker_count);
            let queue = std::sync::Arc::new(job_claim::DecodedQueue::new(decode_queue_target));
            let reset_done = Arc::new(Mutex::new(HashSet::new()));
            info!(
                "Analysis workers starting: compute={}, decode={}, queue_target={}, queue_max={}",
                worker_count,
                decode_workers,
                decode_queue_target,
                queue.max_size()
            );
            for worker_index in 0..decode_workers {
                self.threads.push(job_claim::spawn_decoder_worker(
                    worker_index,
                    queue.clone(),
                    self.cancel.clone(),
                    self.shutdown.clone(),
                    self.pause_claiming.clone(),
                    self.allowed_source_ids.clone(),
                    self.max_duration_bits.clone(),
                    self.analysis_sample_rate.clone(),
                    decode_queue_target,
                    reset_done.clone(),
                ));
            }
            for worker_index in 0..worker_count {
                self.threads.push(job_claim::spawn_compute_worker(
                    worker_index,
                    message_tx.clone(),
                    queue.clone(),
                    self.cancel.clone(),
                    self.shutdown.clone(),
                    self.use_cache.clone(),
                    self.allowed_source_ids.clone(),
                    self.max_duration_bits.clone(),
                    self.analysis_sample_rate.clone(),
                    self.analysis_version_override.clone(),
                    self._progress_cache.clone(),
                ));
            }
            self.threads.push(job_progress::spawn_progress_poller(
                message_tx,
                self.cancel.clone(),
                self.shutdown.clone(),
                self.allowed_source_ids.clone(),
                self._progress_cache.clone(),
            ));
        }
    }

    pub(crate) fn restart(
        &mut self,
        message_tx: Sender<crate::egui_app::controller::jobs::JobMessage>,
    ) {
        if self.threads.is_empty() {
            self.start(message_tx);
            return;
        }
        self.shutdown.store(true, Ordering::Relaxed);
        self.cancel.store(true, Ordering::Relaxed);
        self.pause_claiming.store(false, Ordering::Relaxed);
        std::thread::spawn(|| {
            let _ = job_cleanup::reset_running_jobs();
        });
        let old_threads = mem::take(&mut self.threads);
        std::thread::spawn(move || {
            for handle in old_threads {
                let _ = handle.join();
            }
        });
        self.shutdown = Arc::new(AtomicBool::new(false));
        self.cancel = Arc::new(AtomicBool::new(false));
        self.pause_claiming = Arc::new(AtomicBool::new(false));
        self.start(message_tx);
    }

    pub(crate) fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        let _ = job_cleanup::reset_running_jobs();
    }

    pub(crate) fn resume(&self) {
        self.cancel.store(false, Ordering::Relaxed);
    }

    pub(crate) fn shutdown(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        self.cancel.store(true, Ordering::Relaxed);
        let _ = job_cleanup::reset_running_jobs();
        for handle in self.threads.drain(..) {
            let _ = handle.join();
        }
    }
}

pub(crate) fn default_worker_count() -> usize {
    job_claim::worker_count_with_override(0)
}

impl Drop for AnalysisWorkerPool {
    fn drop(&mut self) {
        self.shutdown();
    }
}
