mod job_claim;
mod job_cleanup;
mod job_progress;
mod job_runner;

use std::sync::{
    Arc,
    atomic::AtomicU32,
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
};
use std::thread::JoinHandle;

/// Long-lived worker pool that claims and processes analysis jobs from the library database.
pub(in crate::egui_app::controller) struct AnalysisWorkerPool {
    cancel: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    max_duration_bits: Arc<AtomicU32>,
    worker_count_override: Arc<AtomicU32>,
    threads: Vec<JoinHandle<()>>,
}

impl AnalysisWorkerPool {
    pub(in crate::egui_app::controller) fn new() -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            shutdown: Arc::new(AtomicBool::new(false)),
            max_duration_bits: Arc::new(AtomicU32::new(30.0f32.to_bits())),
            worker_count_override: Arc::new(AtomicU32::new(0)),
            threads: Vec::new(),
        }
    }

    pub(in crate::egui_app::controller) fn set_max_analysis_duration_seconds(&self, value: f32) {
        let clamped = value.clamp(1.0, 60.0 * 60.0);
        self.max_duration_bits
            .store(clamped.to_bits(), Ordering::Relaxed);
    }

    pub(in crate::egui_app::controller) fn set_worker_count(&self, value: u32) {
        self.worker_count_override.store(value, Ordering::Relaxed);
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
            for worker_index in 0..worker_count {
                self.threads.push(job_claim::spawn_worker(
                    worker_index,
                    message_tx.clone(),
                    self.cancel.clone(),
                    self.shutdown.clone(),
                    self.max_duration_bits.clone(),
                ));
            }
            self.threads.push(job_progress::spawn_progress_poller(
                message_tx,
                self.cancel.clone(),
                self.shutdown.clone(),
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

pub(super) fn library_db_path() -> Result<std::path::PathBuf, String> {
    let dir = crate::app_dirs::app_root_dir().map_err(|err| err.to_string())?;
    Ok(dir.join(crate::sample_sources::library::LIBRARY_DB_FILE_NAME))
}
