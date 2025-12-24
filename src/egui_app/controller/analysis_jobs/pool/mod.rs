mod job_claim;
mod job_cleanup;
mod job_progress;
mod job_runner;

use crate::sample_sources::SourceId;
use std::sync::{
    Arc, RwLock,
    atomic::AtomicU32,
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
};
use std::thread::JoinHandle;

/// Long-lived worker pool that claims and processes analysis jobs from the library database.
pub(in crate::egui_app::controller) struct AnalysisWorkerPool {
    cancel: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    pause_claiming: Arc<AtomicBool>,
    use_cache: Arc<AtomicBool>,
    allowed_source_ids: Arc<RwLock<Option<std::collections::HashSet<SourceId>>>>,
    max_duration_bits: Arc<AtomicU32>,
    analysis_sample_rate: Arc<AtomicU32>,
    analysis_version_override: Arc<RwLock<Option<String>>>,
    worker_count_override: Arc<AtomicU32>,
    threads: Vec<JoinHandle<()>>,
}

impl AnalysisWorkerPool {
    pub(in crate::egui_app::controller) fn new() -> Self {
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
            threads: Vec::new(),
        }
    }

    pub(in crate::egui_app::controller) fn set_max_analysis_duration_seconds(&self, value: f32) {
        let clamped = value.clamp(0.0, 60.0 * 60.0);
        self.max_duration_bits
            .store(clamped.to_bits(), Ordering::Relaxed);
    }

    pub(in crate::egui_app::controller) fn set_worker_count(&self, value: u32) {
        self.worker_count_override.store(value, Ordering::Relaxed);
    }

    pub(in crate::egui_app::controller) fn set_analysis_sample_rate(&self, value: u32) {
        let clamped = value.max(1);
        self.analysis_sample_rate.store(clamped, Ordering::Relaxed);
    }

    pub(in crate::egui_app::controller) fn set_analysis_cache_enabled(&self, enabled: bool) {
        self.use_cache.store(enabled, Ordering::Relaxed);
    }

    pub(in crate::egui_app::controller) fn set_analysis_version_override(
        &self,
        value: Option<String>,
    ) {
        if let Ok(mut guard) = self.analysis_version_override.write() {
            *guard = value;
        }
    }

    pub(in crate::egui_app::controller) fn set_allowed_sources(
        &self,
        sources: Option<Vec<SourceId>>,
    ) {
        if let Ok(mut guard) = self.allowed_source_ids.write() {
            *guard = sources.map(|ids| ids.into_iter().collect());
        }
    }

    pub(in crate::egui_app::controller) fn pause_claiming(&self) {
        self.pause_claiming.store(true, Ordering::Relaxed);
    }

    pub(in crate::egui_app::controller) fn resume_claiming(&self) {
        self.pause_claiming.store(false, Ordering::Relaxed);
    }

    pub(in crate::egui_app::controller) fn start(
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
            let decode_workers = worker_count.min(2).max(1);
            let queue = std::sync::Arc::new(job_claim::DecodedQueue::new());
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
                ));
            }
            self.threads.push(job_progress::spawn_progress_poller(
                message_tx,
                self.cancel.clone(),
                self.shutdown.clone(),
                self.allowed_source_ids.clone(),
            ));
        }
    }

    pub(in crate::egui_app::controller) fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        let _ = job_cleanup::reset_running_jobs();
    }

    pub(in crate::egui_app::controller) fn resume(&self) {
        self.cancel.store(false, Ordering::Relaxed);
    }

    pub(in crate::egui_app::controller) fn shutdown(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        self.cancel.store(true, Ordering::Relaxed);
        let _ = job_cleanup::reset_running_jobs();
        for handle in self.threads.drain(..) {
            let _ = handle.join();
        }
    }
}

impl Drop for AnalysisWorkerPool {
    fn drop(&mut self) {
        self.shutdown();
    }
}
