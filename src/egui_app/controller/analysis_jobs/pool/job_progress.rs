use crate::egui_app::controller::analysis_jobs::db;
use crate::egui_app::controller::analysis_jobs::types::{AnalysisJobMessage, AnalysisProgress};
use crate::egui_app::controller::jobs::JobMessage;
use rusqlite::Connection;
use std::sync::{
    Arc,
    RwLock,
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
};
use std::thread::{JoinHandle, sleep};
use std::time::{Duration, Instant};

const POLL_INTERVAL_ACTIVE: Duration = Duration::from_millis(500);
const POLL_INTERVAL_IDLE: Duration = Duration::from_millis(1500);
const SOURCE_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);

struct ProgressSourceDb {
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
        next.push(ProgressSourceDb { conn });
    }
    *sources = next;
}

fn current_progress_all(sources: &mut [ProgressSourceDb]) -> AnalysisProgress {
    let mut total = AnalysisProgress::default();
    for source in sources {
        if let Ok(progress) = db::current_progress(&source.conn) {
            total.pending += progress.pending;
            total.running += progress.running;
            total.done += progress.done;
            total.failed += progress.failed;
            total.samples_total += progress.samples_total;
            total.samples_pending_or_running += progress.samples_pending_or_running;
        }
    }
    total
}

#[cfg_attr(test, allow(dead_code))]
pub(super) fn spawn_progress_poller(
    tx: Sender<JobMessage>,
    cancel: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    allowed_source_ids: Arc<RwLock<Option<std::collections::HashSet<crate::sample_sources::SourceId>>>>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut sources = Vec::new();
        let mut last_refresh = Instant::now() - SOURCE_REFRESH_INTERVAL;
        let mut last: Option<AnalysisProgress> = None;
        let mut last_heartbeat = Instant::now() - HEARTBEAT_INTERVAL;
        let mut idle_polls = 0u32;
        loop {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }
            if cancel.load(Ordering::Relaxed) {
                sleep(POLL_INTERVAL_IDLE);
                continue;
            }
            let allowed = allowed_source_ids
                .read()
                .ok()
                .and_then(|guard| guard.clone());
            refresh_sources(&mut sources, &mut last_refresh, allowed.as_ref());
            let progress = current_progress_all(&mut sources);
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
            sleep(interval);
        }
    })
}
