use super::job_execution::{
    run_analysis_jobs_with_decoded_batch, run_job, update_job_status_with_retry,
};
use crate::egui_app::controller::analysis_jobs::db;
use crate::egui_app::controller::analysis_jobs::types::AnalysisJobMessage;
use crate::egui_app::controller::jobs::JobMessage;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{
    Arc,
    Mutex,
    RwLock,
    atomic::AtomicU32,
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
};
use std::thread::{JoinHandle, sleep};
use std::time::{Duration, Instant};

use super::progress_cache::ProgressCache;

#[cfg(target_os = "windows")]
use windows::Win32::System::Threading::{
    GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
};

mod claim;
mod dedup;
mod lease;
mod logging;
mod queue;
mod selection;

#[allow(unused_imports)]
pub(in crate::egui_app::controller::analysis_jobs) use claim::{
    decode_queue_target, decode_worker_count_with_override, worker_count_with_override,
};
pub(in crate::egui_app::controller::analysis_jobs) use queue::{
    DecodeOutcome, DecodedQueue, DecodedWork,
};


#[cfg_attr(test, allow(dead_code))]
pub(super) fn spawn_decoder_worker(
    _worker_index: usize,
    decode_queue: Arc<DecodedQueue>,
    cancel: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    pause_claiming: Arc<AtomicBool>,
    allowed_source_ids: Arc<RwLock<Option<HashSet<crate::sample_sources::SourceId>>>>,
    max_duration_bits: Arc<AtomicU32>,
    analysis_sample_rate: Arc<AtomicU32>,
    decode_queue_target: usize,
    reset_done: Arc<Mutex<HashSet<std::path::PathBuf>>>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        lower_worker_priority();
        let log_jobs = logging::analysis_log_enabled();
        let mut selector = selection::ClaimSelector::new(reset_done);
        let decode_queue_target = decode_queue_target.max(1);
        loop {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }
            if cancel.load(Ordering::Relaxed) {
                sleep(Duration::from_millis(50));
                continue;
            }
            if pause_claiming.load(Ordering::Relaxed) {
                sleep(Duration::from_millis(50));
                continue;
            }
            if decode_queue.len() >= decode_queue_target {
                sleep(Duration::from_millis(10));
                continue;
            }
            let allowed = allowed_source_ids
                .read()
                .ok()
                .and_then(|guard| guard.clone());
            let job = match selector.select_next(allowed.as_ref()) {
                selection::ClaimSelection::Job(job) => job,
                selection::ClaimSelection::NoSources => {
                    sleep(Duration::from_millis(200));
                    continue;
                }
                selection::ClaimSelection::Idle => {
                    sleep(Duration::from_millis(25));
                    continue;
                }
            };
            if !lease::job_allowed(&job, allowed.as_ref()) {
                if let Ok(conn) = db::open_source_db(&job.source_root) {
                    lease::release_claim(&conn, job.id);
                }
                continue;
            }
            if !decode_queue.try_mark_inflight(job.id) {
                if log_jobs {
                    eprintln!("analysis decode skipped inflight: {}", job.sample_id);
                }
                continue;
            }
            if log_jobs {
                eprintln!("analysis decode start: {} ({})", job.sample_id, job.job_type);
            }
            let heartbeat = if job.job_type == db::ANALYZE_SAMPLE_JOB_TYPE {
                Some(spawn_decode_heartbeat(
                    job.source_root.clone(),
                    job.id,
                    Duration::from_secs(4),
                ))
            } else {
                None
            };
            let outcome = if job.job_type == db::ANALYZE_SAMPLE_JOB_TYPE {
                decode_analysis_job(&job, &max_duration_bits, &analysis_sample_rate)
            } else {
                DecodeOutcome::NotNeeded
            };
            if let Some((stop, handle)) = heartbeat {
                stop.store(true, Ordering::Relaxed);
                let _ = handle.join();
            }
            if log_jobs {
                match &outcome {
                    DecodeOutcome::Decoded(_) => {
                        eprintln!("analysis decode done: {}", job.sample_id);
                    }
                    DecodeOutcome::Skipped { .. } => {
                        eprintln!("analysis decode skipped: {}", job.sample_id);
                    }
                    DecodeOutcome::Failed(err) => {
                        eprintln!("analysis decode failed: {} ({})", job.sample_id, err);
                    }
                    DecodeOutcome::NotNeeded => {
                        eprintln!("analysis decode not needed: {}", job.sample_id);
                    }
                }
            }
            let job_sample_id = job.sample_id.clone();
            let job_id = job.id;
            let queued = decode_queue.push(DecodedWork { job, outcome });
            if !queued {
                decode_queue.clear_inflight(job_id);
                if log_jobs {
                    eprintln!("analysis decode skipped duplicate: {}", job_sample_id);
                }
            }
        }
    })
}

#[cfg_attr(test, allow(dead_code))]
pub(super) fn spawn_compute_worker(
    _worker_index: usize,
    tx: Sender<JobMessage>,
    decode_queue: Arc<DecodedQueue>,
    cancel: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    use_cache: Arc<AtomicBool>,
    allowed_source_ids: Arc<RwLock<Option<HashSet<crate::sample_sources::SourceId>>>>,
    max_duration_bits: Arc<AtomicU32>,
    analysis_sample_rate: Arc<AtomicU32>,
    analysis_version_override: Arc<std::sync::RwLock<Option<String>>>,
    progress_cache: Arc<RwLock<ProgressCache>>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        lower_worker_priority();
        let log_jobs = logging::analysis_log_enabled();
        let log_queue = logging::analysis_log_queue_enabled();
        let mut last_queue_log = Instant::now();
        let mut connections: HashMap<std::path::PathBuf, Connection> = HashMap::new();
        let embedding_batch_max = crate::analysis::embedding::embedding_batch_max();
        loop {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }
            if cancel.load(Ordering::Relaxed) {
                sleep(Duration::from_millis(50));
                continue;
            }
            let (batch, wait_ms) = decode_queue.pop_batch(&shutdown, embedding_batch_max);
            if batch.is_empty() {
                continue;
            }
            if log_queue && last_queue_log.elapsed() >= Duration::from_secs(2) {
                last_queue_log = Instant::now();
                eprintln!(
                    "analysis queue: decoded={}, batch={}, wait_ms={}",
                    decode_queue.len(),
                    batch.len(),
                    wait_ms
                );
            }
            let max_analysis_duration_seconds =
                f32::from_bits(max_duration_bits.load(Ordering::Relaxed));
            let analysis_sample_rate = analysis_sample_rate.load(Ordering::Relaxed).max(1);
            let use_cache = use_cache.load(Ordering::Relaxed);
            let analysis_version = analysis_version_override
                .read()
                .ok()
                .and_then(|guard| guard.clone())
                .unwrap_or_else(|| crate::analysis::version::analysis_version().to_string());
            let mut decoded_batches: HashMap<
                std::path::PathBuf,
                Vec<(db::ClaimedJob, crate::analysis::audio::AnalysisAudio)>,
            > = HashMap::new();
            let mut immediate_jobs: Vec<(db::ClaimedJob, Result<(), String>)> = Vec::new();

            for work in batch {
                let allowed = allowed_source_ids
                    .read()
                    .ok()
                    .and_then(|guard| guard.clone());
                if !lease::job_allowed(&work.job, allowed.as_ref()) {
                    let conn = match connections.entry(work.job.source_root.clone()) {
                        std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                        std::collections::hash_map::Entry::Vacant(entry) => {
                            let conn = match db::open_source_db(&work.job.source_root) {
                                Ok(conn) => conn,
                                Err(_) => {
                                    decode_queue.clear_inflight(work.job.id);
                                    continue;
                                }
                            };
                            entry.insert(conn)
                        }
                    };
                    lease::release_claim(conn, work.job.id);
                    decode_queue.clear_inflight(work.job.id);
                    continue;
                }
                if log_jobs {
                    eprintln!("analysis run start: {} ({})", work.job.sample_id, work.job.job_type);
                }
                let job_fallback = work.job.clone();
                let mut batch_job: Option<(db::ClaimedJob, crate::analysis::audio::AnalysisAudio)> =
                    None;
                let mut immediate_job: Option<(db::ClaimedJob, Result<(), String>)> = None;

                let outcome = catch_unwind(AssertUnwindSafe(|| {
                    let conn = match connections.entry(work.job.source_root.clone()) {
                        std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                        std::collections::hash_map::Entry::Vacant(entry) => {
                            let conn = match db::open_source_db(&work.job.source_root) {
                                Ok(conn) => conn,
                                Err(_) => {
                                    immediate_job = Some((
                                        work.job,
                                        Err("Failed to open source DB".to_string()),
                                    ));
                                    return Ok(());
                                }
                            };
                            entry.insert(conn)
                        }
                    };
                    match work.job.job_type.as_str() {
                        db::ANALYZE_SAMPLE_JOB_TYPE => match work.outcome {
                            DecodeOutcome::Decoded(decoded) => {
                                batch_job = Some((work.job, decoded));
                                Ok(())
                            }
                            DecodeOutcome::Skipped {
                                duration_seconds,
                                sample_rate,
                            } => {
                                let res = db::update_analysis_metadata(
                                    conn,
                                    &work.job.sample_id,
                                    work.job.content_hash.as_deref(),
                                    duration_seconds,
                                    sample_rate,
                                    &analysis_version,
                                );
                                immediate_job = Some((work.job, res));
                                Ok(())
                            }
                            DecodeOutcome::Failed(err) => {
                                immediate_job = Some((work.job, Err(err)));
                                Ok(())
                            }
                            DecodeOutcome::NotNeeded => {
                                immediate_job = Some((
                                    work.job,
                                    Err("Decode missing for analysis job".to_string()),
                                ));
                                Ok(())
                            }
                        },
                        _ => {
                            let res = run_job(
                                conn,
                                &work.job,
                                use_cache,
                                max_analysis_duration_seconds,
                                analysis_sample_rate,
                                &analysis_version,
                            );
                            immediate_job = Some((work.job, res));
                            Ok(())
                        }
                    }
                }))
                .unwrap_or_else(|payload| Err(logging::panic_to_string(payload)));

                if let Err(err) = outcome {
                    immediate_job = Some((job_fallback, Err(err)));
                }
                if let Some((job, decoded)) = batch_job {
                    decoded_batches
                        .entry(job.source_root.clone())
                        .or_default()
                        .push((job, decoded));
                }
                if let Some(entry) = immediate_job {
                    immediate_jobs.push(entry);
                }
            }

            for (source_root, jobs) in decoded_batches {
                let conn = match connections.entry(source_root.clone()) {
                    std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        let conn = match db::open_source_db(&source_root) {
                            Ok(conn) => conn,
                            Err(_) => {
                                for (job, _) in jobs {
                                    immediate_jobs.push((
                                        job,
                                        Err("Failed to open source DB".to_string()),
                                    ));
                                }
                                continue;
                            }
                        };
                        entry.insert(conn)
                    }
                };
                let jobs_for_failure: Vec<db::ClaimedJob> =
                    jobs.iter().map(|(job, _)| job.clone()).collect();
                let analysis_context = super::job_execution::AnalysisContext {
                    use_cache,
                    max_analysis_duration_seconds,
                    analysis_sample_rate,
                    analysis_version: analysis_version.as_str(),
                };
                let batch_outcomes = catch_unwind(AssertUnwindSafe(|| {
                    run_analysis_jobs_with_decoded_batch(conn, jobs, &analysis_context)
                }))
                .unwrap_or_else(|payload| {
                    let err = logging::panic_to_string(payload);
                    tracing::warn!("Analysis batch panicked: {err}");
                    jobs_for_failure
                        .into_iter()
                        .map(|job| (job, Err(err.clone())))
                        .collect()
                });
                immediate_jobs.extend(batch_outcomes);
            }

            for (job, outcome) in immediate_jobs {
                finalize_immediate_job(
                    &mut connections,
                    &decode_queue,
                    &tx,
                    job,
                    outcome,
                    log_jobs,
                    &progress_cache,
                );
            }
        }
    })
}

fn finalize_immediate_job(
    connections: &mut HashMap<std::path::PathBuf, Connection>,
    decode_queue: &DecodedQueue,
    tx: &Sender<JobMessage>,
    job: db::ClaimedJob,
    outcome: Result<(), String>,
    log_jobs: bool,
    progress_cache: &Arc<RwLock<ProgressCache>>,
) {
    if log_jobs {
        match &outcome {
            Ok(()) => {
                eprintln!("analysis run done: {}", job.sample_id);
            }
            Err(err) => {
                eprintln!("analysis run failed: {} ({})", job.sample_id, err);
            }
        }
    }
    let conn = match open_connection_with_retry(connections, &job.source_root) {
        Ok(conn) => conn,
        Err(err) => {
            tracing::warn!(
                "Analysis job DB open failed for {}: {err}",
                job.sample_id
            );
            decode_queue.clear_inflight(job.id);
            return;
        }
    };
    match outcome {
        Ok(()) => {
            update_job_status_with_retry(|| db::mark_done(conn, job.id));
        }
        Err(err) => {
            update_job_status_with_retry(|| db::mark_failed(conn, job.id, &err));
        }
    }
    decode_queue.clear_inflight(job.id);
    if let Ok(progress) = db::current_progress(conn) {
        let source_id = db::parse_sample_id(&job.sample_id)
            .ok()
            .map(|(source_id, _)| crate::sample_sources::SourceId::from_string(source_id));
        if let Some(source_id) = source_id.as_ref() {
            if let Ok(mut cache) = progress_cache.write() {
                cache.update(source_id.clone(), progress);
            }
        }
        let _ = tx.send(JobMessage::Analysis(AnalysisJobMessage::Progress {
            source_id,
            progress,
        }));
    }
}

fn open_connection_with_retry<'a>(
    connections: &'a mut HashMap<std::path::PathBuf, Connection>,
    source_root: &std::path::Path,
) -> Result<&'a mut Connection, String> {
    match connections.entry(source_root.to_path_buf()) {
        std::collections::hash_map::Entry::Occupied(entry) => Ok(entry.into_mut()),
        std::collections::hash_map::Entry::Vacant(entry) => {
            let mut last_err = None;
            for attempt in 0..=1 {
                match db::open_source_db(source_root) {
                    Ok(conn) => return Ok(entry.insert(conn)),
                    Err(err) => {
                        last_err = Some(err);
                        if attempt == 0 {
                            sleep(Duration::from_millis(50));
                        }
                    }
                }
            }
            Err(last_err.unwrap_or_else(|| "Failed to open source DB".to_string()))
        }
    }
}

fn spawn_decode_heartbeat(
    source_root: std::path::PathBuf,
    job_id: i64,
    interval: Duration,
) -> (Arc<AtomicBool>, JoinHandle<()>) {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_worker = Arc::clone(&stop);
    let handle = std::thread::spawn(move || {
        let conn = match db::open_source_db(&source_root) {
            Ok(conn) => conn,
            Err(err) => {
                tracing::warn!(
                    "Analysis decode heartbeat failed to open DB for {}: {err}",
                    source_root.display()
                );
                return;
            }
        };
        let _ = db::touch_running_at(&conn, &[job_id]);
        let mut last_touch = Instant::now() - interval;
        let poll = Duration::from_millis(200);
        loop {
            if stop_worker.load(Ordering::Relaxed) {
                break;
            }
            if last_touch.elapsed() >= interval {
                let _ = db::touch_running_at(&conn, &[job_id]);
                last_touch = Instant::now();
            }
            sleep(poll);
        }
    });
    (stop, handle)
}

fn decode_analysis_job(
    job: &db::ClaimedJob,
    max_duration_bits: &AtomicU32,
    analysis_sample_rate: &AtomicU32,
) -> DecodeOutcome {
    let (_source_id, relative_path) = match db::parse_sample_id(&job.sample_id) {
        Ok(parsed) => parsed,
        Err(err) => return DecodeOutcome::Failed(err),
    };
    let absolute = job.source_root.join(&relative_path);
    let max_analysis_duration_seconds = f32::from_bits(max_duration_bits.load(Ordering::Relaxed));
    let sample_rate = analysis_sample_rate.load(Ordering::Relaxed).max(1);
    if max_analysis_duration_seconds.is_finite() && max_analysis_duration_seconds > 0.0 {
        if let Ok(probe) = crate::analysis::audio::probe_metadata(&absolute) {
            if let Some(duration_seconds) = probe.duration_seconds {
                if duration_seconds > max_analysis_duration_seconds {
                    let sample_rate = probe
                        .sample_rate
                        .unwrap_or(crate::analysis::audio::ANALYSIS_SAMPLE_RATE);
                    return DecodeOutcome::Skipped {
                        duration_seconds,
                        sample_rate,
                    };
                }
            }
        }
    }
    match crate::analysis::audio::decode_for_analysis_with_rate(&absolute, sample_rate) {
        Ok(decoded) => DecodeOutcome::Decoded(decoded),
        Err(err) => DecodeOutcome::Failed(err),
    }
}

fn lower_worker_priority() {
    #[cfg(target_os = "windows")]
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::mpsc;
    use std::sync::{Arc, RwLock};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    use tempfile::NamedTempFile;
    use tempfile::TempDir;

    #[test]
    fn clears_inflight_when_db_open_fails() {
        let file = NamedTempFile::new().unwrap();
        let source_root = file.path().to_path_buf();
        let job = db::ClaimedJob {
            id: 42,
            sample_id: "source::missing.wav".to_string(),
            content_hash: None,
            job_type: db::ANALYZE_SAMPLE_JOB_TYPE.to_string(),
            source_root: source_root.clone(),
        };
        let queue = DecodedQueue::new();
        assert!(queue.try_mark_inflight(job.id));
        let (tx, _rx) = mpsc::channel::<JobMessage>();
        let mut connections = HashMap::new();
        let progress_cache = Arc::new(RwLock::new(ProgressCache::default()));

        finalize_immediate_job(
            &mut connections,
            &queue,
            &tx,
            job,
            Err("failed".to_string()),
            false,
            &progress_cache,
        );

        assert!(queue.try_mark_inflight(42));
        assert!(connections.is_empty());
    }

    #[test]
    fn decode_heartbeat_keeps_running_job_fresh() {
        let dir = TempDir::new().unwrap();
        let conn = db::open_source_db(dir.path()).unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| Duration::from_secs(0))
            .as_secs() as i64;
        conn.execute(
            "INSERT INTO analysis_jobs (sample_id, job_type, status, attempts, created_at, running_at)
             VALUES (?1, ?2, 'running', 1, ?3, ?4)",
            rusqlite::params![
                "source::long.wav",
                db::ANALYZE_SAMPLE_JOB_TYPE,
                now,
                now - 120
            ],
        )
        .unwrap();
        let job_id: i64 = conn
            .query_row(
                "SELECT id FROM analysis_jobs WHERE sample_id = ?1",
                rusqlite::params!["source::long.wav"],
                |row| row.get(0),
            )
            .unwrap();

        let (stop, handle) =
            spawn_decode_heartbeat(dir.path().to_path_buf(), job_id, Duration::from_millis(10));
        let deadline = Instant::now() + Duration::from_millis(500);
        loop {
            let running_at: Option<i64> = conn
                .query_row(
                    "SELECT running_at FROM analysis_jobs WHERE id = ?1",
                    rusqlite::params![job_id],
                    |row| row.get(0),
                )
                .unwrap_or(None);
            if running_at.is_some_and(|ts| ts >= now - 1) {
                break;
            }
            if Instant::now() >= deadline {
                break;
            }
            sleep(Duration::from_millis(10));
        }
        let stale_before = now - 1;
        let changed = db::fail_stale_running_jobs(&conn, stale_before).unwrap();
        stop.store(true, Ordering::Relaxed);
        let _ = handle.join();

        assert_eq!(changed, 0);
    }
}
