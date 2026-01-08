use super::job_execution::{run_analysis_jobs_with_decoded_batch, run_job};
use crate::egui_app::controller::library::analysis_jobs::db as analysis_db;
use crate::egui_app::controller::jobs::JobMessage;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{
    Arc, Mutex, RwLock,
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
mod db;
mod dedup;
mod lease;
mod logging;
mod queue;
mod selection;

#[allow(unused_imports)]
pub(crate) use claim::{
    decode_queue_target, decode_worker_count_with_override, worker_count_with_override,
};
pub(crate) use queue::{
    DecodeOutcome, DecodedQueue, DecodedWork,
};

#[cfg(test)]
mod tests;

#[cfg_attr(test, allow(dead_code))]
pub(crate) fn spawn_decoder_worker(
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
        let mut connections: HashMap<std::path::PathBuf, Connection> = HashMap::new();
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
                if let Ok(conn) = db::open_connection_with_retry(&mut connections, &job.source_root)
                {
                    lease::release_claim(conn, job.id);
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
                eprintln!(
                    "analysis decode start: {} ({})",
                    job.sample_id, job.job_type
                );
            }
            let heartbeat = if job.job_type == analysis_db::ANALYZE_SAMPLE_JOB_TYPE {
                Some(db::spawn_decode_heartbeat(
                    job.source_root.clone(),
                    job.id,
                    Duration::from_secs(4),
                ))
            } else {
                None
            };
            let outcome = if job.job_type == analysis_db::ANALYZE_SAMPLE_JOB_TYPE {
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
pub(crate) fn spawn_compute_worker(
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
        let mut deferred_updates: Vec<db::DeferredJobUpdate> = Vec::new();
        let embedding_batch_max = crate::analysis::similarity::SIMILARITY_BATCH_MAX;
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
                db::flush_deferred_updates(
                    &mut connections,
                    &decode_queue,
                    &tx,
                    &progress_cache,
                    &mut deferred_updates,
                    log_jobs,
                );
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
                Vec<(
                    analysis_db::ClaimedJob,
                    crate::analysis::audio::AnalysisAudio,
                )>,
            > = HashMap::new();
            let mut immediate_jobs: Vec<(analysis_db::ClaimedJob, Result<(), String>)> = Vec::new();

            for work in batch {
                let allowed = allowed_source_ids
                    .read()
                    .ok()
                    .and_then(|guard| guard.clone());
                if !lease::job_allowed(&work.job, allowed.as_ref()) {
                    match db::open_connection_with_retry(&mut connections, &work.job.source_root) {
                        Ok(conn) => lease::release_claim(conn, work.job.id),
                        Err(err) => {
                            if log_jobs {
                                eprintln!(
                                    "analysis release failed: {} ({})",
                                    work.job.sample_id, err
                                );
                            }
                        }
                    }
                    decode_queue.clear_inflight(work.job.id);
                    continue;
                }
                if log_jobs {
                    eprintln!(
                        "analysis run start: {} ({})",
                        work.job.sample_id, work.job.job_type
                    );
                }
                let job_fallback = work.job.clone();
                let mut batch_job: Option<(
                    analysis_db::ClaimedJob,
                    crate::analysis::audio::AnalysisAudio,
                )> = None;
                let mut immediate_job: Option<(analysis_db::ClaimedJob, Result<(), String>)> = None;

                let outcome = catch_unwind(AssertUnwindSafe(|| {
                    let conn = match db::open_connection_with_retry(
                        &mut connections,
                        &work.job.source_root,
                    ) {
                        Ok(conn) => conn,
                        Err(err) => {
                            immediate_job = Some((work.job, Err(err)));
                            return Ok(());
                        }
                    };
                    match work.job.job_type.as_str() {
                        analysis_db::ANALYZE_SAMPLE_JOB_TYPE => match work.outcome {
                            DecodeOutcome::Decoded(decoded) => {
                                batch_job = Some((work.job, decoded));
                                Ok(())
                            }
                            DecodeOutcome::Skipped {
                                duration_seconds,
                                sample_rate,
                            } => {
                                let res = analysis_db::update_analysis_metadata(
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
                let conn = match db::open_connection_with_retry(&mut connections, &source_root) {
                    Ok(conn) => conn,
                    Err(err) => {
                        for (job, _) in jobs {
                            immediate_jobs.push((job, Err(err.clone())));
                        }
                        continue;
                    }
                };
                let jobs_for_failure: Vec<analysis_db::ClaimedJob> =
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
                if let Some(deferred) = db::finalize_immediate_job(
                    &mut connections,
                    &decode_queue,
                    &tx,
                    job,
                    outcome,
                    log_jobs,
                    &progress_cache,
                ) {
                    deferred_updates.push(deferred);
                }
            }
            db::flush_deferred_updates(
                &mut connections,
                &decode_queue,
                &tx,
                &progress_cache,
                &mut deferred_updates,
                log_jobs,
            );
        }
    })
}

fn decode_analysis_job(
    job: &analysis_db::ClaimedJob,
    max_duration_bits: &AtomicU32,
    analysis_sample_rate: &AtomicU32,
) -> DecodeOutcome {
    let (_source_id, relative_path) = match analysis_db::parse_sample_id(&job.sample_id) {
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
