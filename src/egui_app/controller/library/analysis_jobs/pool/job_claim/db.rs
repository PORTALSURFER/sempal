use super::super::job_execution::update_job_status_with_retry;
use super::super::progress_cache::ProgressCache;
use super::analysis_db;
use super::super::job_progress::ProgressPollerWakeup;
use super::queue::DecodedQueue;
use crate::egui_app::controller::library::analysis_jobs::types::AnalysisJobMessage;
use crate::egui_app::controller::jobs::JobMessage;
use rusqlite::Connection;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::sync::{atomic::AtomicBool, mpsc::Sender};
use std::thread::{JoinHandle, sleep};
use std::time::{Duration, Instant};

pub(crate) struct DeferredJobUpdate {
    pub(crate) job: analysis_db::ClaimedJob,
    pub(crate) error: String,
}

pub(crate) fn finalize_immediate_job(
    connections: &mut HashMap<std::path::PathBuf, Connection>,
    decode_queue: &DecodedQueue,
    tx: &Sender<JobMessage>,
    job: analysis_db::ClaimedJob,
    outcome: Result<(), String>,
    log_jobs: bool,
    progress_cache: &Arc<RwLock<ProgressCache>>,
    progress_wakeup: &ProgressPollerWakeup,
) -> Option<DeferredJobUpdate> {
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
    let error_for_open = outcome
        .as_ref()
        .err()
        .cloned()
        .unwrap_or_else(|| "Failed to open source DB".to_string());
    let conn = match open_connection_with_retry(connections, &job.source_root) {
        Ok(conn) => conn,
        Err(err) => {
            tracing::warn!("Analysis job DB open failed for {}: {err}", job.sample_id);
            decode_queue.clear_inflight(job.id);
            return Some(DeferredJobUpdate {
                job,
                error: error_for_open,
            });
        }
    };
    match outcome {
        Ok(()) => {
            update_job_status_with_retry(|| analysis_db::mark_done(conn, job.id));
        }
        Err(err) => {
            update_job_status_with_retry(|| {
                analysis_db::mark_failed_with_reason(conn, job.id, &err)
            });
        }
    }
    decode_queue.clear_inflight(job.id);
    if let Ok(progress) = analysis_db::current_progress(conn) {
        let source_id = analysis_db::parse_sample_id(&job.sample_id)
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
        progress_wakeup.notify();
    }
    None
}

pub(crate) fn open_connection_with_retry<'a>(
    connections: &'a mut HashMap<std::path::PathBuf, Connection>,
    source_root: &std::path::Path,
) -> Result<&'a mut Connection, String> {
    match connections.entry(source_root.to_path_buf()) {
        std::collections::hash_map::Entry::Occupied(entry) => Ok(entry.into_mut()),
        std::collections::hash_map::Entry::Vacant(entry) => {
            let mut last_err = None;
            for attempt in 0..=1 {
                match analysis_db::open_source_db(source_root) {
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

pub(crate) fn flush_deferred_updates(
    connections: &mut HashMap<std::path::PathBuf, Connection>,
    decode_queue: &DecodedQueue,
    tx: &Sender<JobMessage>,
    progress_cache: &Arc<RwLock<ProgressCache>>,
    progress_wakeup: &ProgressPollerWakeup,
    deferred_updates: &mut Vec<DeferredJobUpdate>,
    log_jobs: bool,
) {
    if deferred_updates.is_empty() {
        return;
    }
    let mut remaining = Vec::new();
    for deferred in deferred_updates.drain(..) {
        if let Some(next) = finalize_immediate_job(
            connections,
            decode_queue,
            tx,
            deferred.job,
            Err(deferred.error),
            log_jobs,
            progress_cache,
            progress_wakeup,
        ) {
            remaining.push(next);
        }
    }
    *deferred_updates = remaining;
}

pub(crate) fn spawn_decode_heartbeat(
    source_root: std::path::PathBuf,
    job_id: i64,
    interval: Duration,
) -> (Arc<AtomicBool>, JoinHandle<()>) {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_worker = Arc::clone(&stop);
    let handle = std::thread::spawn(move || {
        let mut connections = HashMap::new();
        let conn = match open_connection_with_retry(&mut connections, &source_root) {
            Ok(conn) => conn,
            Err(err) => {
                tracing::warn!(
                    "Analysis decode heartbeat failed to open DB for {}: {err}",
                    source_root.display()
                );
                return;
            }
        };
        let _ = analysis_db::touch_running_at(&conn, &[job_id]);
        let mut last_touch = Instant::now() - interval;
        let poll = Duration::from_millis(200);
        loop {
            if stop_worker.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            if last_touch.elapsed() >= interval {
                let _ = analysis_db::touch_running_at(&conn, &[job_id]);
                last_touch = Instant::now();
            }
            sleep(poll);
        }
    });
    (stop, handle)
}
