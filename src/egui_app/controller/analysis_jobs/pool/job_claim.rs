use super::job_runner::{run_analysis_jobs_with_decoded_batch, run_job};
use crate::egui_app::controller::analysis_jobs::db;
use crate::egui_app::controller::analysis_jobs::types::AnalysisJobMessage;
use crate::egui_app::controller::jobs::JobMessage;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet, VecDeque};
use std::cmp::min;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{
    Arc,
    Condvar,
    Mutex,
    RwLock,
    atomic::AtomicU32,
    atomic::AtomicUsize,
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
    len: AtomicUsize,
    pending_jobs: Mutex<HashSet<i64>>,
}

impl DecodedQueue {
    pub(super) fn new() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            ready: Condvar::new(),
            len: AtomicUsize::new(0),
            pending_jobs: Mutex::new(HashSet::new()),
        }
    }

    pub(super) fn push(&self, work: DecodedWork) -> bool {
        let mut guard = self.queue.lock().expect("decoded queue lock");
        if work.job.job_type == db::ANALYZE_SAMPLE_JOB_TYPE {
            let mut pending = self.pending_jobs.lock().expect("decoded queue pending lock");
            if pending.contains(&work.job.id) {
                return false;
            }
            pending.insert(work.job.id);
        }
        guard.push_back(work);
        self.len.fetch_add(1, Ordering::Relaxed);
        self.ready.notify_one();
        true
    }

    pub(super) fn pop(&self, shutdown: &AtomicBool) -> Option<DecodedWork> {
        let mut guard = self.queue.lock().expect("decoded queue lock");
        loop {
            if shutdown.load(Ordering::Relaxed) {
                return None;
            }
            if let Some(work) = guard.pop_front() {
                if work.job.job_type == db::ANALYZE_SAMPLE_JOB_TYPE {
                    let mut pending =
                        self.pending_jobs.lock().expect("decoded queue pending lock");
                    pending.remove(&work.job.id);
                }
                self.len.fetch_sub(1, Ordering::Relaxed);
                return Some(work);
            }
            let (next_guard, _) = self
                .ready
                .wait_timeout(guard, Duration::from_millis(50))
                .expect("decoded queue wait");
            guard = next_guard;
        }
    }

    pub(super) fn pop_batch(
        &self,
        shutdown: &AtomicBool,
        max: usize,
    ) -> (Vec<DecodedWork>, u64) {
        let mut guard = self.queue.lock().expect("decoded queue lock");
        let start = Instant::now();
        loop {
            if shutdown.load(Ordering::Relaxed) {
                return (Vec::new(), start.elapsed().as_millis() as u64);
            }
            if let Some(work) = guard.pop_front() {
                let mut batch = Vec::with_capacity(max.max(1));
                batch.push(work);
                self.len.fetch_sub(1, Ordering::Relaxed);
                while batch.len() < max {
                    if let Some(next) = guard.pop_front() {
                        batch.push(next);
                        self.len.fetch_sub(1, Ordering::Relaxed);
                    } else {
                        break;
                    }
                }
                {
                    let mut pending =
                        self.pending_jobs.lock().expect("decoded queue pending lock");
                    for item in &batch {
                        if item.job.job_type == db::ANALYZE_SAMPLE_JOB_TYPE {
                            pending.remove(&item.job.id);
                        }
                    }
                }
                return (batch, start.elapsed().as_millis() as u64);
            }
            let (next_guard, _) = self
                .ready
                .wait_timeout(guard, Duration::from_millis(50))
                .expect("decoded queue wait");
            guard = next_guard;
        }
    }

    pub(super) fn len(&self) -> usize {
        self.len.load(Ordering::Relaxed)
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
    allowed_source_ids: Option<&HashSet<crate::sample_sources::SourceId>>,
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
        if let Some(allowed) = allowed_source_ids {
            if !allowed.contains(&source.id) {
                continue;
            }
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
pub(super) fn decode_worker_count_with_override(
    worker_count: usize,
    override_count: u32,
) -> usize {
    if override_count >= 1 {
        return override_count as usize;
    }
    if let Ok(value) = std::env::var("SEMPAL_DECODE_WORKERS") {
        if let Ok(parsed) = value.trim().parse::<usize>() {
            if parsed >= 1 {
                return parsed;
            }
        }
    }
    let max_workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(worker_count.max(1));
    min(worker_count.saturating_mul(2).max(2), max_workers)
}

fn claim_batch_size() -> usize {
    if let Ok(value) = std::env::var("SEMPAL_ANALYSIS_CLAIM_BATCH") {
        if let Ok(parsed) = value.trim().parse::<usize>() {
            if parsed >= 1 {
                return parsed;
            }
        }
    }
    64
}

pub(super) fn decode_queue_target(embedding_batch_max: usize, worker_count: usize) -> usize {
    if let Ok(value) = std::env::var("SEMPAL_DECODE_QUEUE_TARGET") {
        if let Ok(parsed) = value.trim().parse::<usize>() {
            if parsed >= 1 {
                return parsed;
            }
        }
    }
    (embedding_batch_max.saturating_mul(worker_count)).max(4)
}

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
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        lower_worker_priority();
        let log_jobs = analysis_log_enabled();
        let mut sources = Vec::new();
        let mut last_refresh = Instant::now() - SOURCE_REFRESH_INTERVAL;
        let mut reset_done = HashSet::new();
        let mut next_source = 0usize;
        let mut local_queue: VecDeque<db::ClaimedJob> = VecDeque::new();
        let claim_batch = claim_batch_size();
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
            refresh_sources(
                &mut sources,
                &mut last_refresh,
                &mut reset_done,
                allowed.as_ref(),
            );
            if sources.is_empty() {
                sleep(Duration::from_millis(200));
                continue;
            }
            if local_queue.is_empty() {
                for _ in 0..sources.len() {
                    let idx = next_source % sources.len();
                    next_source = next_source.wrapping_add(1);
                    let source = &mut sources[idx];
                    let jobs = match db::claim_next_jobs(
                        &mut source.conn,
                        &source.source.root,
                        source.source.id.as_str(),
                        claim_batch,
                    ) {
                        Ok(jobs) => jobs,
                        Err(_) => continue,
                    };
                    if !jobs.is_empty() {
                        local_queue.extend(jobs);
                        break;
                    }
                }
                if local_queue.is_empty() {
                    sleep(Duration::from_millis(25));
                    continue;
                }
            }
            let Some(job) = local_queue.pop_front() else {
                sleep(Duration::from_millis(25));
                continue;
            };
            if let Some(allowed) = allowed.as_ref() {
                if let Ok((source_id, _)) = db::parse_sample_id(&job.sample_id) {
                    let source_id = crate::sample_sources::SourceId::from_string(source_id);
                    if !allowed.contains(&source_id) {
                        if let Ok(conn) = db::open_source_db(&job.source_root) {
                            let _ = db::mark_pending(&conn, job.id);
                        }
                        continue;
                    }
                }
            }
            if log_jobs {
                eprintln!("analysis decode start: {} ({})", job.sample_id, job.job_type);
            }
            let outcome = if job.job_type == db::ANALYZE_SAMPLE_JOB_TYPE {
                decode_analysis_job(&job, &max_duration_bits, &analysis_sample_rate)
            } else {
                DecodeOutcome::NotNeeded
            };
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
            let queued = decode_queue.push(DecodedWork { job, outcome });
            if !queued {
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
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        lower_worker_priority();
        let log_jobs = analysis_log_enabled();
        let log_queue = analysis_log_queue_enabled();
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
                if let Some(allowed) = allowed.as_ref() {
                    if let Ok((source_id, _)) = db::parse_sample_id(&work.job.sample_id) {
                        let source_id = crate::sample_sources::SourceId::from_string(source_id);
                        if !allowed.contains(&source_id) {
                            let conn = match connections.entry(work.job.source_root.clone()) {
                                std::collections::hash_map::Entry::Occupied(entry) => {
                                    entry.into_mut()
                                }
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
                            let _ = db::mark_pending(conn, work.job.id);
                            continue;
                        }
                    }
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
                .unwrap_or_else(|payload| Err(panic_to_string(payload)));

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
                let batch_outcomes = catch_unwind(AssertUnwindSafe(|| {
                    run_analysis_jobs_with_decoded_batch(conn, jobs, use_cache, &analysis_version)
                }))
                .unwrap_or_else(|payload| {
                    let err = panic_to_string(payload);
                    tracing::warn!("Analysis batch panicked: {err}");
                    jobs_for_failure
                        .into_iter()
                        .map(|job| (job, Err(err.clone())))
                        .collect()
                });
                immediate_jobs.extend(batch_outcomes);
            }

            for (job, outcome) in immediate_jobs {
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
                let conn = match connections.entry(job.source_root.clone()) {
                    std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        let conn = match db::open_source_db(&job.source_root) {
                            Ok(conn) => conn,
                            Err(_) => {
                                continue;
                            }
                        };
                        entry.insert(conn)
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
                if let Ok(progress) = db::current_progress(conn) {
                    let source_id = db::parse_sample_id(&job.sample_id)
                        .ok()
                        .map(|(source_id, _)| {
                            crate::sample_sources::SourceId::from_string(source_id)
                        });
                    let _ = tx.send(JobMessage::Analysis(AnalysisJobMessage::Progress {
                        source_id,
                        progress,
                    }));
                }
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

fn update_job_status_with_retry<F>(mut update: F)
where
    F: FnMut() -> Result<(), String>,
{
    const RETRIES: usize = 5;
    for attempt in 0..RETRIES {
        match update() {
            Ok(()) => return,
            Err(_) if attempt + 1 < RETRIES => {
                sleep(Duration::from_millis(50));
            }
            Err(err) => {
                tracing::warn!("Failed to update analysis job status: {err}");
                return;
            }
        }
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

fn analysis_log_enabled() -> bool {
    std::env::var("SEMPAL_ANALYSIS_LOG_JOBS")
        .map(|value| value.trim() == "1")
        .unwrap_or(false)
}

fn analysis_log_queue_enabled() -> bool {
    std::env::var("SEMPAL_ANALYSIS_LOG_QUEUE")
        .map(|value| value.trim() == "1")
        .unwrap_or(false)
}
