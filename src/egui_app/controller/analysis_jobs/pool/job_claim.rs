use super::job_runner::{run_analysis_job_with_decoded, run_job};
use crate::egui_app::controller::analysis_jobs::db;
use crate::egui_app::controller::analysis_jobs::types::AnalysisJobMessage;
use crate::egui_app::controller::jobs::JobMessage;
use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{
    Arc,
    Condvar,
    Mutex,
    atomic::AtomicU32,
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
};
use std::thread::{JoinHandle, sleep};
use std::time::Duration;

#[cfg(target_os = "windows")]
use windows::Win32::System::Threading::{
    GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
};

pub(super) struct DecodedQueue {
    queue: Mutex<VecDeque<DecodedWork>>,
    ready: Condvar,
}

impl DecodedQueue {
    pub(super) fn new() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            ready: Condvar::new(),
        }
    }

    pub(super) fn push(&self, work: DecodedWork) {
        let mut guard = self.queue.lock().expect("decoded queue lock");
        guard.push_back(work);
        self.ready.notify_one();
    }

    pub(super) fn pop(&self, shutdown: &AtomicBool) -> Option<DecodedWork> {
        let mut guard = self.queue.lock().expect("decoded queue lock");
        loop {
            if shutdown.load(Ordering::Relaxed) {
                return None;
            }
            if let Some(work) = guard.pop_front() {
                return Some(work);
            }
            let (next_guard, _) = self
                .ready
                .wait_timeout(guard, Duration::from_millis(50))
                .expect("decoded queue wait");
            guard = next_guard;
        }
    }
}

pub(super) struct DecodedWork {
    pub(super) job: db::ClaimedJob,
    pub(super) outcome: DecodeOutcome,
}

pub(super) enum DecodeOutcome {
    Decoded(crate::analysis::audio::AnalysisAudio),
    Skipped {
        duration_seconds: f32,
        sample_rate: u32,
    },
    Failed(String),
    NotNeeded,
}

#[cfg_attr(test, allow(dead_code))]
pub(super) fn worker_count_with_override(override_count: u32) -> usize {
    if override_count >= 1 {
        return override_count as usize;
    }
    if let Ok(value) = std::env::var("SEMPAL_ANALYSIS_WORKERS") {
        if let Ok(parsed) = value.trim().parse::<usize>() {
            if parsed >= 1 {
                return parsed;
            }
        }
    }
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .saturating_sub(2)
        .max(1)
}

#[cfg_attr(test, allow(dead_code))]
pub(super) fn spawn_decoder_worker(
    _worker_index: usize,
    decode_queue: Arc<DecodedQueue>,
    cancel: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    pause_claiming: Arc<AtomicBool>,
    max_duration_bits: Arc<AtomicU32>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        lower_worker_priority();
        let db_path = match super::library_db_path() {
            Ok(path) => path,
            Err(_) => return,
        };
        let mut conn = match db::open_library_db(&db_path) {
            Ok(conn) => conn,
            Err(_) => return,
        };
        let _ = db::prune_jobs_for_missing_sources(&conn);
        let _ = db::reset_running_to_pending(&conn);
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
            let outcome = if job.job_type == db::ANALYZE_SAMPLE_JOB_TYPE {
                decode_analysis_job(&conn, &job, &max_duration_bits)
            } else {
                DecodeOutcome::NotNeeded
            };
            decode_queue.push(DecodedWork { job, outcome });
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
    max_duration_bits: Arc<AtomicU32>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        lower_worker_priority();
        let db_path = match super::library_db_path() {
            Ok(path) => path,
            Err(_) => return,
        };
        let mut conn = match db::open_library_db(&db_path) {
            Ok(conn) => conn,
            Err(_) => return,
        };
        let _ = db::prune_jobs_for_missing_sources(&conn);
        let _ = db::reset_running_to_pending(&conn);
        loop {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }
            if cancel.load(Ordering::Relaxed) {
                sleep(Duration::from_millis(50));
                continue;
            }
            let Some(work) = decode_queue.pop(&shutdown) else {
                continue;
            };
            let max_analysis_duration_seconds =
                f32::from_bits(max_duration_bits.load(Ordering::Relaxed));
            let outcome = catch_unwind(AssertUnwindSafe(|| match work.job.job_type.as_str() {
                db::ANALYZE_SAMPLE_JOB_TYPE => match work.outcome {
                    DecodeOutcome::Decoded(decoded) => {
                        run_analysis_job_with_decoded(&conn, &work.job, decoded)
                    }
                    DecodeOutcome::Skipped {
                        duration_seconds,
                        sample_rate,
                    } => db::update_analysis_metadata(
                        &conn,
                        &work.job.sample_id,
                        work.job.content_hash.as_deref(),
                        duration_seconds,
                        sample_rate,
                    ),
                    DecodeOutcome::Failed(err) => Err(err),
                    DecodeOutcome::NotNeeded => Err("Decode missing for analysis job".to_string()),
                },
                _ => run_job(&conn, &work.job, max_analysis_duration_seconds),
            }))
            .unwrap_or_else(|payload| Err(panic_to_string(payload)));
            match outcome {
                Ok(()) => {
                    let _ = db::mark_done(&conn, work.job.id);
                }
                Err(err) => {
                    let _ = db::mark_failed(&conn, work.job.id, &err);
                }
            }
            if let Ok(progress) = db::current_progress(&conn) {
                let _ = tx.send(JobMessage::Analysis(AnalysisJobMessage::Progress(progress)));
            }
        }
    })
}

fn decode_analysis_job(
    conn: &rusqlite::Connection,
    job: &db::ClaimedJob,
    max_duration_bits: &AtomicU32,
) -> DecodeOutcome {
    let (source_id, relative_path) = match db::parse_sample_id(&job.sample_id) {
        Ok(parsed) => parsed,
        Err(err) => return DecodeOutcome::Failed(err),
    };
    let Some(root) = match db::source_root_for(conn, &source_id) {
        Ok(root) => root,
        Err(err) => return DecodeOutcome::Failed(err),
    } else {
        return DecodeOutcome::Failed(format!(
            "Source not found for job sample_id={}",
            job.sample_id
        ));
    };
    let absolute = root.join(&relative_path);
    let max_analysis_duration_seconds = f32::from_bits(max_duration_bits.load(Ordering::Relaxed));
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
    match crate::analysis::audio::decode_for_analysis(&absolute) {
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

fn panic_to_string(payload: Box<dyn std::any::Any + Send>) -> String {
    let message = if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "Unknown panic payload".to_string()
    };
    let backtrace = std::backtrace::Backtrace::capture();
    format!("Analysis worker panicked: {message}\n{backtrace}")
}
