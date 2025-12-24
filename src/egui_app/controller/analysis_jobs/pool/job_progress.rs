use crate::egui_app::controller::analysis_jobs::db;
use crate::egui_app::controller::analysis_jobs::types::{AnalysisJobMessage, AnalysisProgress};
use crate::egui_app::controller::jobs::JobMessage;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
};
use std::thread::{JoinHandle, sleep};
use std::time::Duration;

const POLL_INTERVAL_ACTIVE: Duration = Duration::from_millis(500);
const POLL_INTERVAL_IDLE: Duration = Duration::from_millis(1500);

#[cfg_attr(test, allow(dead_code))]
pub(super) fn spawn_progress_poller(
    tx: Sender<JobMessage>,
    cancel: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let db_path = match super::library_db_path() {
            Ok(path) => path,
            Err(_) => return,
        };
        let conn = match db::open_library_db(&db_path) {
            Ok(conn) => conn,
            Err(_) => return,
        };
        let mut last: Option<AnalysisProgress> = None;
        let mut idle_polls = 0u32;
        loop {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }
            if cancel.load(Ordering::Relaxed) {
                sleep(POLL_INTERVAL_IDLE);
                continue;
            }
            let progress = match db::current_progress(&conn) {
                Ok(progress) => progress,
                Err(_) => {
                    sleep(POLL_INTERVAL_IDLE);
                    continue;
                }
            };
            if last != Some(progress) {
                last = Some(progress);
                idle_polls = 0;
                let _ = tx.send(JobMessage::Analysis(AnalysisJobMessage::Progress(progress)));
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
