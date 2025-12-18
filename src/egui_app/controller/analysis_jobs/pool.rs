use super::db;
use super::inference;
use super::types::{AnalysisJobMessage, AnalysisProgress};
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::AtomicU32,
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
};
use std::thread::{JoinHandle, sleep};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Long-lived worker pool that claims and processes analysis jobs from the library database.
pub(in crate::egui_app::controller) struct AnalysisWorkerPool {
    cancel: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    unknown_threshold_bits: Arc<AtomicU32>,
    threads: Vec<JoinHandle<()>>,
}

impl AnalysisWorkerPool {
    pub(in crate::egui_app::controller) fn new() -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            shutdown: Arc::new(AtomicBool::new(false)),
            unknown_threshold_bits: Arc::new(AtomicU32::new(0.8f32.to_bits())),
            threads: Vec::new(),
        }
    }

    pub(in crate::egui_app::controller) fn set_unknown_confidence_threshold(&self, value: f32) {
        let clamped = value.clamp(0.0, 1.0);
        self.unknown_threshold_bits
            .store(clamped.to_bits(), Ordering::Relaxed);
    }

    pub(in crate::egui_app::controller) fn start(
        &mut self,
        message_tx: Sender<super::super::jobs::JobMessage>,
    ) {
        let _ = &message_tx;
        if !self.threads.is_empty() {
            return;
        }
        #[cfg(not(test))]
        {
            let worker_count = worker_count();
            for worker_index in 0..worker_count {
                self.threads.push(spawn_worker(
                    worker_index,
                    message_tx.clone(),
                    self.cancel.clone(),
                    self.shutdown.clone(),
                    self.unknown_threshold_bits.clone(),
                ));
            }
            self.threads.push(spawn_progress_poller(
                message_tx,
                self.cancel.clone(),
                self.shutdown.clone(),
            ));
        }
    }

    pub(in crate::egui_app::controller) fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        let _ = reset_running_jobs();
    }

    pub(in crate::egui_app::controller) fn resume(&self) {
        self.cancel.store(false, Ordering::Relaxed);
    }

    pub(in crate::egui_app::controller) fn shutdown(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        self.cancel.store(true, Ordering::Relaxed);
        let _ = reset_running_jobs();
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

#[cfg_attr(test, allow(dead_code))]
fn worker_count() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .saturating_sub(1)
        .max(1)
}

#[cfg_attr(test, allow(dead_code))]
fn reset_running_jobs() -> Result<(), String> {
    let db_path = library_db_path()?;
    let conn = db::open_library_db(&db_path)?;
    let _ = db::reset_running_to_pending(&conn)?;
    Ok(())
}

#[cfg_attr(test, allow(dead_code))]
fn spawn_progress_poller(
    tx: Sender<super::super::jobs::JobMessage>,
    cancel: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let db_path = match library_db_path() {
            Ok(path) => path,
            Err(_) => return,
        };
        let conn = match db::open_library_db(&db_path) {
            Ok(conn) => conn,
            Err(_) => return,
        };
        let mut last: Option<AnalysisProgress> = None;
        loop {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }
            if cancel.load(Ordering::Relaxed) {
                sleep(Duration::from_millis(200));
                continue;
            }
            let progress = match db::current_progress(&conn) {
                Ok(progress) => progress,
                Err(_) => {
                    sleep(Duration::from_millis(500));
                    continue;
                }
            };
            if last != Some(progress) {
                last = Some(progress);
                let _ = tx.send(super::super::jobs::JobMessage::Analysis(
                    AnalysisJobMessage::Progress(progress),
                ));
            }
            sleep(Duration::from_millis(200));
        }
    })
}

#[cfg_attr(test, allow(dead_code))]
fn spawn_worker(
    _worker_index: usize,
    tx: Sender<super::super::jobs::JobMessage>,
    cancel: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    unknown_threshold_bits: Arc<AtomicU32>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let db_path = match library_db_path() {
            Ok(path) => path,
            Err(_) => return,
        };
        let mut conn = match db::open_library_db(&db_path) {
            Ok(conn) => conn,
            Err(_) => return,
        };
        let _ = db::reset_running_to_pending(&conn);
        let mut model_cache: Option<inference::CachedModel> = None;
        let _ = inference::refresh_latest_model(&conn, &mut model_cache);

        loop {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }
            if cancel.load(Ordering::Relaxed) {
                sleep(Duration::from_millis(50));
                continue;
            }
            let job = match db::claim_next_job(&mut conn) {
                Ok(job) => job,
                Err(_) => {
                    sleep(Duration::from_millis(200));
                    continue;
                }
            };
            let Some(job) = job else {
                sleep(Duration::from_millis(25));
                continue;
            };
            let unknown_threshold = f32::from_bits(unknown_threshold_bits.load(Ordering::Relaxed));
            let outcome = run_job(&conn, &job, &mut model_cache, unknown_threshold);
            match outcome {
                Ok(()) => {
                    let _ = db::mark_done(&conn, job.id);
                }
                Err(err) => {
                    let _ = db::mark_failed(&conn, job.id, &err);
                }
            }
            if let Ok(progress) = db::current_progress(&conn) {
                let _ = tx.send(super::super::jobs::JobMessage::Analysis(
                    AnalysisJobMessage::Progress(progress),
                ));
            }
        }
    })
}

#[cfg_attr(test, allow(dead_code))]
fn run_job(
    conn: &rusqlite::Connection,
    job: &db::ClaimedJob,
    model_cache: &mut Option<inference::CachedModel>,
    unknown_confidence_threshold: f32,
) -> Result<(), String> {
    if job.job_type != db::DEFAULT_JOB_TYPE {
        return Err(format!("Unknown job type: {}", job.job_type));
    }
    let (source_id, relative_path) = db::parse_sample_id(&job.sample_id)?;
    let Some(root) = db::source_root_for(conn, &source_id)? else {
        return Err(format!("Source not found for job sample_id={}", job.sample_id));
    };
    let absolute = root.join(&relative_path);
    let decoded = crate::analysis::audio::decode_for_analysis(&absolute)?;
    let time_domain = crate::analysis::time_domain::extract_time_domain_features(
        &decoded.mono,
        decoded.sample_rate_used,
    );
    let frequency_domain = crate::analysis::frequency_domain::extract_frequency_domain_features(
        &decoded.mono,
        decoded.sample_rate_used,
    );
    let features = crate::analysis::features::AnalysisFeaturesV1::new(time_domain, frequency_domain);
    db::update_analysis_metadata(
        conn,
        &job.sample_id,
        job.content_hash.as_deref(),
        decoded.duration_seconds,
        decoded.sample_rate_used,
    )?;
    let content_hash = job
        .content_hash
        .as_deref()
        .ok_or_else(|| format!("Missing content_hash for analysis job {}", job.sample_id))?;
    let current_hash = db::sample_content_hash(conn, &job.sample_id)?;
    if current_hash.as_deref() != Some(content_hash) {
        return Ok(());
    }
    let vector = crate::analysis::vector::to_f32_vector_v1(&features);
    let blob = crate::analysis::vector::encode_f32_le_blob(&vector);
    let computed_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs() as i64;
    db::upsert_analysis_features(
        conn,
        &job.sample_id,
        &blob,
        crate::analysis::vector::FEATURE_VERSION_V1,
        computed_at,
    )?;
    inference::infer_and_upsert_prediction(
        conn,
        model_cache,
        &job.sample_id,
        content_hash,
        &vector,
        computed_at,
        unknown_confidence_threshold,
    )?;
    Ok(())
}

fn library_db_path() -> Result<PathBuf, String> {
    let dir = crate::app_dirs::app_root_dir().map_err(|err| err.to_string())?;
    Ok(dir.join(crate::sample_sources::library::LIBRARY_DB_FILE_NAME))
}
