use super::db;
use super::inference;
use super::types::{AnalysisJobMessage, AnalysisProgress};
use std::path::PathBuf;
use std::panic::{AssertUnwindSafe, catch_unwind};
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
    max_duration_bits: Arc<AtomicU32>,
    threads: Vec<JoinHandle<()>>,
}

impl AnalysisWorkerPool {
    pub(in crate::egui_app::controller) fn new() -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            shutdown: Arc::new(AtomicBool::new(false)),
            unknown_threshold_bits: Arc::new(AtomicU32::new(0.8f32.to_bits())),
            max_duration_bits: Arc::new(AtomicU32::new(30.0f32.to_bits())),
            threads: Vec::new(),
        }
    }

    pub(in crate::egui_app::controller) fn set_unknown_confidence_threshold(&self, value: f32) {
        let clamped = value.clamp(0.0, 1.0);
        self.unknown_threshold_bits
            .store(clamped.to_bits(), Ordering::Relaxed);
    }

    pub(in crate::egui_app::controller) fn set_max_analysis_duration_seconds(&self, value: f32) {
        let clamped = value.clamp(1.0, 60.0 * 60.0);
        self.max_duration_bits
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
                    self.max_duration_bits.clone(),
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
    max_duration_bits: Arc<AtomicU32>,
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
            let max_analysis_duration_seconds =
                f32::from_bits(max_duration_bits.load(Ordering::Relaxed));
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                run_job(
                    &conn,
                    &job,
                    &mut model_cache,
                    unknown_threshold,
                    max_analysis_duration_seconds,
                )
            }))
            .unwrap_or_else(|_| Err("Analysis worker panicked".to_string()));
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
    max_analysis_duration_seconds: f32,
) -> Result<(), String> {
    match job.job_type.as_str() {
        db::DEFAULT_JOB_TYPE => run_analysis_job(
            conn,
            job,
            model_cache,
            unknown_confidence_threshold,
            max_analysis_duration_seconds,
        ),
        db::INFERENCE_JOB_TYPE => {
            run_inference_job(conn, job, model_cache, unknown_confidence_threshold)
        }
        _ => Err(format!("Unknown job type: {}", job.job_type)),
    }
}

fn run_analysis_job(
    conn: &rusqlite::Connection,
    job: &db::ClaimedJob,
    model_cache: &mut Option<inference::CachedModel>,
    unknown_confidence_threshold: f32,
    max_analysis_duration_seconds: f32,
) -> Result<(), String> {
    let (source_id, relative_path) = db::parse_sample_id(&job.sample_id)?;
    let Some(root) = db::source_root_for(conn, &source_id)? else {
        return Err(format!("Source not found for job sample_id={}", job.sample_id));
    };
    let absolute = root.join(&relative_path);
    if max_analysis_duration_seconds.is_finite() && max_analysis_duration_seconds > 0.0 {
        if let Ok(Some(duration_seconds)) = crate::analysis::audio::probe_duration_seconds(&absolute)
        {
            if duration_seconds > max_analysis_duration_seconds {
                db::update_analysis_metadata(
                    conn,
                    &job.sample_id,
                    job.content_hash.as_deref(),
                    duration_seconds,
                    crate::analysis::audio::ANALYSIS_SAMPLE_RATE,
                )?;
                return Ok(());
            }
        }
    }
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
    let computed_at = now_epoch_seconds();
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

fn run_inference_job(
    conn: &rusqlite::Connection,
    job: &db::ClaimedJob,
    model_cache: &mut Option<inference::CachedModel>,
    unknown_confidence_threshold: f32,
) -> Result<(), String> {
    let content_hash = job
        .content_hash
        .as_deref()
        .ok_or_else(|| format!("Missing content_hash for inference job {}", job.sample_id))?;
    let current_hash = db::sample_content_hash(conn, &job.sample_id)?;
    if current_hash.as_deref() != Some(content_hash) {
        return Ok(());
    }
    let vec_blob = load_feature_blob(conn, &job.sample_id)?;
    let features = decode_f32le_feature_row(&vec_blob)?;
    let computed_at = now_epoch_seconds();
    inference::infer_and_upsert_prediction(
        conn,
        model_cache,
        &job.sample_id,
        content_hash,
        &features,
        computed_at,
        unknown_confidence_threshold,
    )?;
    Ok(())
}

fn load_feature_blob(conn: &rusqlite::Connection, sample_id: &str) -> Result<Vec<u8>, String> {
    conn.query_row(
        "SELECT vec_blob FROM features WHERE sample_id = ?1",
        rusqlite::params![sample_id],
        |row| row.get::<_, Vec<u8>>(0),
    )
    .map_err(|err| format!("Failed to load feature blob for {sample_id}: {err}"))
}

fn decode_f32le_feature_row(blob: &[u8]) -> Result<Vec<f32>, String> {
    if blob.len() % 4 != 0 {
        return Err("Feature blob length is not a multiple of 4 bytes".to_string());
    }
    let mut out = Vec::with_capacity(blob.len() / 4);
    for chunk in blob.chunks_exact(4) {
        out.push(f32::from_le_bytes(
            chunk.try_into().expect("chunk size verified"),
        ));
    }
    Ok(out)
}

fn now_epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs() as i64
}

fn library_db_path() -> Result<PathBuf, String> {
    let dir = crate::app_dirs::app_root_dir().map_err(|err| err.to_string())?;
    Ok(dir.join(crate::sample_sources::library::LIBRARY_DB_FILE_NAME))
}
