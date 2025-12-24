use super::job_runner::run_job;
use crate::egui_app::controller::analysis_jobs::db;
use crate::egui_app::controller::analysis_jobs::types::AnalysisJobMessage;
use crate::egui_app::controller::jobs::JobMessage;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{
    Arc,
    atomic::AtomicU32,
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
};
use std::thread::{JoinHandle, sleep};
use std::time::Duration;

#[cfg(target_os = "windows")]
use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL};

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
pub(super) fn spawn_worker(
    _worker_index: usize,
    tx: Sender<JobMessage>,
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
            let max_analysis_duration_seconds =
                f32::from_bits(max_duration_bits.load(Ordering::Relaxed));
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                run_job(&conn, &job, max_analysis_duration_seconds)
            }))
            .unwrap_or_else(|payload| Err(panic_to_string(payload)));
            match outcome {
                Ok(()) => {
                    let _ = db::mark_done(&conn, job.id);
                }
                Err(err) => {
                    let _ = db::mark_failed(&conn, job.id, &err);
                }
            }
            if let Ok(progress) = db::current_progress(&conn) {
                let _ = tx.send(JobMessage::Analysis(AnalysisJobMessage::Progress(progress)));
            }
        }
    })
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
