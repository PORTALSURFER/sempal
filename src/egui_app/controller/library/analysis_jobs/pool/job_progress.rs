use crate::egui_app::controller::library::analysis_jobs::db;
use crate::egui_app::controller::library::analysis_jobs::types::{AnalysisJobMessage, AnalysisProgress};
use crate::egui_app::controller::jobs::{JobMessage, JobMessageSender};
use rusqlite::Connection;
use std::sync::{
    Arc, Condvar, Mutex, RwLock,
    atomic::{AtomicBool, Ordering},
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::progress_cache::ProgressCache;

const POLL_INTERVAL_ACTIVE: Duration = Duration::from_millis(500);
const POLL_INTERVAL_IDLE: Duration = Duration::from_millis(1500);
const SOURCE_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);
const STALE_CLEANUP_INTERVAL: Duration = Duration::from_secs(10);
const DB_REFRESH_INTERVAL: Duration = Duration::from_secs(5);

struct ProgressPollerWakeupState {
    counter: u64,
}

/// Condvar-backed wakeup used to nudge the progress poller on job updates.
pub(crate) struct ProgressPollerWakeup {
    state: Mutex<ProgressPollerWakeupState>,
    ready: Condvar,
}

impl ProgressPollerWakeup {
    /// Create a new progress poller wakeup handle.
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(ProgressPollerWakeupState { counter: 0 }),
            ready: Condvar::new(),
        }
    }

    /// Notify the poller that progress state has changed.
    pub(crate) fn notify(&self) {
        let mut state = self
            .state
            .lock()
            .expect("progress poller wakeup poisoned");
        state.counter = state.counter.wrapping_add(1);
        self.ready.notify_one();
    }

    /// Wait until notified or until the timeout elapses.
    pub(crate) fn wait_for(&self, seen: &mut u64, timeout: Duration) -> bool {
        let state = self
            .state
            .lock()
            .expect("progress poller wakeup poisoned");
        if state.counter != *seen {
            *seen = state.counter;
            return true;
        }
        let (state, _timeout) = self
            .ready
            .wait_timeout(state, timeout)
            .expect("progress poller wakeup poisoned");
        if state.counter != *seen {
            *seen = state.counter;
            return true;
        }
        false
    }
}

struct ProgressSourceDb {
    source_id: crate::sample_sources::SourceId,
    conn: Connection,
}

fn refresh_sources(
    sources: &mut Vec<ProgressSourceDb>,
    last_refresh: &mut Instant,
    allowed_source_ids: Option<&std::collections::HashSet<crate::sample_sources::SourceId>>,
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
            Err(_) => continue,
        };
        next.push(ProgressSourceDb {
            source_id: source.id.clone(),
            conn,
        });
    }
    *sources = next;
}

fn current_progress_all(
    sources: &mut [ProgressSourceDb],
    progress_cache: &Arc<RwLock<ProgressCache>>,
    refresh_cache: bool,
) -> AnalysisProgress {
    if refresh_cache
        || progress_cache
            .read()
            .map(|cache| cache.is_empty())
            .unwrap_or(true)
    {
        let mut total = AnalysisProgress::default();
        let mut updates = Vec::new();
        for source in sources {
            if let Ok(progress) = db::current_progress(&source.conn) {
                total.pending += progress.pending;
                total.running += progress.running;
                total.done += progress.done;
                total.failed += progress.failed;
                total.samples_total += progress.samples_total;
                total.samples_pending_or_running += progress.samples_pending_or_running;
                updates.push((source.source_id.clone(), progress));
            }
        }
        if let Ok(mut cache) = progress_cache.write() {
            cache.update_many(updates);
        }
        return total;
    }
    if let Ok(cache) = progress_cache.read() {
        return cache.total_for_sources(sources.iter().map(|source| &source.source_id));
    }
    AnalysisProgress::default()
}

fn cleanup_stale_jobs(
    sources: &mut [ProgressSourceDb],
    stale_before: i64,
    progress_cache: &Arc<RwLock<ProgressCache>>,
    tx: &JobMessageSender,
    signal: &Arc<Mutex<Option<egui::Context>>>,
) -> usize {
    let mut changed = 0;
    let mut touched_sources = std::collections::HashSet::new();
    for source in &mut *sources {
        if let Ok((updated, source_ids)) =
            db::fail_stale_running_jobs_with_sources(&source.conn, stale_before)
        {
            if updated > 0 {
                changed += updated;
                for source_id in source_ids {
                    touched_sources.insert(source_id);
                }
            }
        }
    }
    if touched_sources.is_empty() {
        return changed;
    }
    let mut updates = Vec::new();
    for source in &mut *sources {
        if !touched_sources.contains(&source.source_id) {
            continue;
        }
        if let Ok(progress) = db::current_progress(&source.conn) {
            updates.push((source.source_id.clone(), progress));
        }
    }
    if let Ok(mut cache) = progress_cache.write() {
        cache.update_many(updates);
    }
    if let Ok(cache) = progress_cache.read() {
        let total = cache.total_for_sources(sources.iter().map(|source| &source.source_id));
        let _ = tx.send(JobMessage::Analysis(AnalysisJobMessage::Progress {
            source_id: None,
            progress: total,
        }));
        if let Ok(lock) = signal.lock() {
            if let Some(ctx) = lock.as_ref() {
                ctx.request_repaint();
            }
        }
    }
    changed
}

fn now_epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs() as i64
}

#[cfg_attr(test, allow(dead_code))]
pub(crate) fn spawn_progress_poller(
    tx: JobMessageSender,
    signal: Arc<Mutex<Option<egui::Context>>>,
    cancel: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    allowed_source_ids: Arc<
        RwLock<Option<std::collections::HashSet<crate::sample_sources::SourceId>>>,
    >,
    progress_cache: Arc<RwLock<ProgressCache>>,
    progress_wakeup: Arc<ProgressPollerWakeup>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut sources = Vec::new();
        let mut last_refresh = Instant::now() - SOURCE_REFRESH_INTERVAL;
        let mut last: Option<AnalysisProgress> = None;
        let mut last_heartbeat = Instant::now() - HEARTBEAT_INTERVAL;
        let mut last_db_refresh = Instant::now() - DB_REFRESH_INTERVAL;
        let mut last_cleanup = Instant::now() - STALE_CLEANUP_INTERVAL;
        let mut idle_polls = 0u32;
        let mut last_sources_empty = None;
        let mut wake_counter = 0u64;
        loop {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }
            let allowed = allowed_source_ids
                .read()
                .ok()
                .and_then(|guard| guard.clone());
            refresh_sources(&mut sources, &mut last_refresh, allowed.as_ref());
            if last_cleanup.elapsed() >= STALE_CLEANUP_INTERVAL {
                last_cleanup = Instant::now();
                let stale_before = now_epoch_seconds().saturating_sub(
                    crate::egui_app::controller::library::analysis_jobs::stale_running_job_seconds(),
                );
                let _ = cleanup_stale_jobs(&mut sources, stale_before, &progress_cache, &tx, &signal);
            }
            if cancel.load(Ordering::Relaxed) {
                let _ = progress_wakeup.wait_for(&mut wake_counter, POLL_INTERVAL_IDLE);
                continue;
            }
            let sources_empty = sources.is_empty();
            if last_sources_empty != Some(sources_empty) {
                last_sources_empty = Some(sources_empty);
                if sources_empty {
                    tracing::info!("Analysis progress poller has no sources to inspect");
                } else {
                    tracing::debug!(
                        "Analysis progress poller inspecting {} source(s)",
                        sources.len()
                    );
                }
            }
            let refresh_cache = should_refresh_db(last_db_refresh, &progress_cache);
            if refresh_cache {
                last_db_refresh = Instant::now();
            }
            let progress = current_progress_all(&mut sources, &progress_cache, refresh_cache);
            let unchanged = last == Some(progress);
            let should_heartbeat = unchanged
                && (progress.pending > 0 || progress.running > 0)
                && last_heartbeat.elapsed() >= HEARTBEAT_INTERVAL;
            if !unchanged || should_heartbeat {
                last = Some(progress);
                idle_polls = 0;
                last_heartbeat = Instant::now();
                let _ = tx.send(JobMessage::Analysis(AnalysisJobMessage::Progress {
                    source_id: None,
                    progress,
                }));
                if let Ok(lock) = signal.lock() {
                    if let Some(ctx) = lock.as_ref() {
                        ctx.request_repaint();
                    }
                }
            }
            if progress.pending == 0 && progress.running == 0 {
                idle_polls = idle_polls.saturating_add(1);
            } else {
                idle_polls = 0;
            }
            let interval = if idle_polls > 2 {
                POLL_INTERVAL_IDLE
            } else {
                POLL_INTERVAL_ACTIVE
            };
            let _ = progress_wakeup.wait_for(&mut wake_counter, interval);
        }
    })
}

fn should_refresh_db(
    last_db_refresh: Instant,
    progress_cache: &Arc<RwLock<ProgressCache>>,
) -> bool {
    if last_db_refresh.elapsed() >= DB_REFRESH_INTERVAL {
        return true;
    }
    progress_cache
        .read()
        .map(|cache| cache.is_empty())
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn cleanup_runs_without_workers() {
        let dir = TempDir::new().unwrap();
        let conn = db::open_source_db(dir.path()).unwrap();
        let now = now_epoch_seconds();
        conn.execute(
            "INSERT INTO analysis_jobs (sample_id, job_type, status, attempts, created_at, running_at)
             VALUES (?1, ?2, 'running', 1, ?3, ?4)",
            rusqlite::params![
                "source::stale.wav",
                db::ANALYZE_SAMPLE_JOB_TYPE,
                now,
                now - 120
            ],
        )
        .unwrap();
        let mut sources = vec![ProgressSourceDb {
            source_id: crate::sample_sources::SourceId::from_string("source".to_string()),
            conn,
        }];
        let cache = Arc::new(RwLock::new(ProgressCache::default()));
        let (tx, _rx) = std::sync::mpsc::channel();
        let stale_before = now - 10;
        let signal = Arc::new(Mutex::new(None));

        let changed = cleanup_stale_jobs(&mut sources, stale_before, &cache, &tx, &signal);

        let status: String = sources[0]
            .conn
            .query_row(
                "SELECT status FROM analysis_jobs WHERE sample_id = ?1",
                rusqlite::params!["source::stale.wav"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(changed, 1);
        assert_eq!(status, "failed");
    }

    #[test]
    fn cleanup_updates_cache_and_emits_message() {
        let dir = TempDir::new().unwrap();
        let conn = db::open_source_db(dir.path()).unwrap();
        conn.execute(
            "INSERT INTO wav_files (path, file_size, modified_ns, missing)
             VALUES (?1, 1, 0, 0)",
            rusqlite::params!["a.wav"],
        )
        .unwrap();
        let now = now_epoch_seconds();
        conn.execute(
            "INSERT INTO analysis_jobs (sample_id, source_id, relative_path, job_type, status, attempts, created_at, running_at)
             VALUES (?1, ?2, ?3, ?4, 'running', 1, ?5, ?6)",
            rusqlite::params![
                "source::a.wav",
                "source",
                "a.wav",
                db::ANALYZE_SAMPLE_JOB_TYPE,
                now,
                now - 120
            ],
        )
        .unwrap();
        let mut sources = vec![ProgressSourceDb {
            source_id: crate::sample_sources::SourceId::from_string("source".to_string()),
            conn,
        }];
        let cache = Arc::new(RwLock::new(ProgressCache::default()));
        let (tx, rx) = std::sync::mpsc::channel();
        let stale_before = now - 10;
        let signal = Arc::new(Mutex::new(None));

        let changed = cleanup_stale_jobs(&mut sources, stale_before, &cache, &tx, &signal);

        assert_eq!(changed, 1);
        let cached = cache.read().unwrap().total_for_sources(std::iter::once(
            &crate::sample_sources::SourceId::from_string("source".to_string()),
        ));
        assert_eq!(cached.failed, 1);
        let message = rx.recv_timeout(Duration::from_millis(50)).unwrap();
        let JobMessage::Analysis(AnalysisJobMessage::Progress { progress, .. }) = message else {
            panic!("unexpected message");
        };
        assert_eq!(progress.failed, 1);
    }

    #[test]
    fn should_refresh_db_when_cache_empty_or_stale() {
        let cache = Arc::new(RwLock::new(ProgressCache::default()));
        assert!(should_refresh_db(Instant::now(), &cache));

        let mut cache_guard = cache.write().unwrap();
        cache_guard.update(
            crate::sample_sources::SourceId::from_string("source".to_string()),
            AnalysisProgress::default(),
        );
        drop(cache_guard);

        assert!(!should_refresh_db(Instant::now(), &cache));
        assert!(should_refresh_db(
            Instant::now() - DB_REFRESH_INTERVAL - Duration::from_millis(1),
            &cache
        ));
    }

    #[test]
    fn wakeup_returns_when_notified() {
        let wakeup = ProgressPollerWakeup::new();
        let mut seen = 0;

        wakeup.notify();

        assert!(wakeup.wait_for(&mut seen, Duration::from_millis(1)));
        assert!(!wakeup.wait_for(&mut seen, Duration::from_millis(1)));
    }
}
