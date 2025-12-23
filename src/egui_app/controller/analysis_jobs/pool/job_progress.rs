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
        loop {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }
            if cancel.load(Ordering::Relaxed) {
                sleep(Duration::from_millis(200));
                continue;
            }
            let progress = match db::current_progress(&conn) {
                Ok(progress) => progress,
                Err(_) => {
                    sleep(Duration::from_millis(500));
                    continue;
                }
            };
            if last != Some(progress) {
                last = Some(progress);
                let _ = tx.send(JobMessage::Analysis(
                    AnalysisJobMessage::Progress(progress),
                ));
            }
            sleep(Duration::from_millis(200));
        }
    })
}
