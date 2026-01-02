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
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const POLL_INTERVAL_ACTIVE: Duration = Duration::from_millis(500);
const POLL_INTERVAL_IDLE: Duration = Duration::from_millis(1500);
const SOURCE_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);
const STALE_CLEANUP_INTERVAL: Duration = Duration::from_secs(10);

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

fn cleanup_stale_jobs(sources: &mut [ProgressSourceDb], stale_before: i64) -> usize {
    let mut changed = 0;
    for source in sources {
        if let Ok(updated) = db::fail_stale_running_jobs(&source.conn, stale_before) {
            changed += updated;
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
        let mut last_cleanup = Instant::now() - STALE_CLEANUP_INTERVAL;
        let mut idle_polls = 0u32;
        let mut last_sources_empty = None;
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
                    crate::egui_app::controller::analysis_jobs::stale_running_job_seconds(),
                );
                let _ = cleanup_stale_jobs(&mut sources, stale_before);
            }
            if cancel.load(Ordering::Relaxed) {
                sleep(POLL_INTERVAL_IDLE);
                continue;
            }
            let sources_empty = sources.is_empty();
            if last_sources_empty != Some(sources_empty) {
                last_sources_empty = Some(sources_empty);
                if sources_empty {
                    tracing::info!("Analysis progress poller has no sources to inspect");
                } else {
                    tracing::info!(
                        "Analysis progress poller inspecting {} source(s)",
                        sources.len()
                    );
                }
            }
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
        let mut sources = vec![ProgressSourceDb { conn }];
        let stale_before = now - 10;

        let changed = cleanup_stale_jobs(&mut sources, stale_before);

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
}
