use super::job_runner::{run_analysis_job_with_decoded, run_job};
use crate::egui_app::controller::analysis_jobs::db;
use crate::egui_app::controller::analysis_jobs::types::AnalysisJobMessage;
use crate::egui_app::controller::jobs::JobMessage;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet, VecDeque};
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
use std::time::{Duration, Instant};

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

struct SourceClaimDb {
    source: crate::sample_sources::SampleSource,
    conn: Connection,
}

const SOURCE_REFRESH_INTERVAL: Duration = Duration::from_secs(5);

fn refresh_sources(
    sources: &mut Vec<SourceClaimDb>,
    last_refresh: &mut Instant,
    reset_done: &mut HashSet<std::path::PathBuf>,
) {
    if last_refresh.elapsed() < SOURCE_REFRESH_INTERVAL {
        return;
    }
    *last_refresh = Instant::now();
    let Ok(state) = crate::sample_sources::library::load() else {
        return;
    };
    let mut next = Vec::new();
    for source in state.sources {
        if !source.root.is_dir() {
            continue;
        }
        let conn = match db::open_source_db(&source.root) {
            Ok(conn) => conn,
            Err(err) => {
                tracing::debug!(
                    "Source DB open failed for {}: {err}",
                    source.root.display()
                );
                continue;
            }
        };
        if reset_done.insert(source.root.clone()) {
            let _ = db::prune_jobs_for_missing_sources(&conn);
            let _ = db::reset_running_to_pending(&conn);
        }
        next.push(SourceClaimDb { source, conn });
    }
    *sources = next;
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
    analysis_sample_rate: Arc<AtomicU32>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        lower_worker_priority();
        let mut sources = Vec::new();
        let mut last_refresh = Instant::now() - SOURCE_REFRESH_INTERVAL;
        let mut reset_done = HashSet::new();
        let mut next_source = 0usize;
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
            refresh_sources(&mut sources, &mut last_refresh, &mut reset_done);
            if sources.is_empty() {
                sleep(Duration::from_millis(200));
                continue;
            }
            let mut claimed = None;
            for _ in 0..sources.len() {
                let idx = next_source % sources.len();
                next_source = next_source.wrapping_add(1);
                let source = &mut sources[idx];
                let job = match db::claim_next_job(&mut source.conn, &source.source.root) {
                    Ok(job) => job,
                    Err(_) => continue,
                };
                if job.is_some() {
                    claimed = job;
                    break;
                }
            }
            let Some(job) = claimed else {
                sleep(Duration::from_millis(25));
                continue;
            };
            let outcome = if job.job_type == db::ANALYZE_SAMPLE_JOB_TYPE {
                decode_analysis_job(&job, &max_duration_bits, &analysis_sample_rate)
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
    analysis_sample_rate: Arc<AtomicU32>,
    analysis_version_override: Arc<std::sync::RwLock<Option<String>>>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        lower_worker_priority();
        let mut connections: HashMap<std::path::PathBuf, Connection> = HashMap::new();
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
            let analysis_sample_rate = analysis_sample_rate.load(Ordering::Relaxed).max(1);
            let analysis_version = analysis_version_override
                .read()
                .ok()
                .and_then(|guard| guard.clone())
                .unwrap_or_else(|| crate::analysis::version::analysis_version().to_string());
            let conn = match connections.entry(work.job.source_root.clone()) {
                std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::hash_map::Entry::Vacant(entry) => {
                    let conn = match db::open_source_db(&work.job.source_root) {
                        Ok(conn) => conn,
                        Err(_) => {
                            continue;
                        }
                    };
                    entry.insert(conn)
                }
            };
            let outcome = catch_unwind(AssertUnwindSafe(|| match work.job.job_type.as_str() {
                db::ANALYZE_SAMPLE_JOB_TYPE => match work.outcome {
                    DecodeOutcome::Decoded(decoded) => run_analysis_job_with_decoded(
                        conn,
                        &work.job,
                        decoded,
                        &analysis_version,
                    ),
                    DecodeOutcome::Skipped {
                        duration_seconds,
                        sample_rate,
                    } => db::update_analysis_metadata(
                        conn,
                        &work.job.sample_id,
                        work.job.content_hash.as_deref(),
                        duration_seconds,
                        sample_rate,
                        &analysis_version,
                    ),
                    DecodeOutcome::Failed(err) => Err(err),
                    DecodeOutcome::NotNeeded => Err("Decode missing for analysis job".to_string()),
                },
                _ => run_job(
                    conn,
                    &work.job,
                    max_analysis_duration_seconds,
                    analysis_sample_rate,
                    &analysis_version,
                ),
            }))
            .unwrap_or_else(|payload| Err(panic_to_string(payload)));
            match outcome {
                Ok(()) => {
                    let _ = db::mark_done(conn, work.job.id);
                }
                Err(err) => {
                    let _ = db::mark_failed(conn, work.job.id, &err);
                }
            }
            if let Ok(progress) = db::current_progress(conn) {
                let source_id = db::parse_sample_id(&work.job.sample_id)
                    .ok()
                    .map(|(source_id, _)| crate::sample_sources::SourceId::from_string(source_id));
                let _ = tx.send(JobMessage::Analysis(AnalysisJobMessage::Progress {
                    source_id,
                    progress,
                }));
            }
        }
    })
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
