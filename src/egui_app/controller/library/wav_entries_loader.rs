use super::{LoadEntriesError, SourceDatabase, WavEntry, WavLoadJob, WavLoadResult};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, Sender},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

const WAV_LOADER_POLL_INTERVAL: Duration = Duration::from_millis(200);

/// Join handle and shutdown signal for the wav entries loader thread.
pub(crate) struct WavLoaderHandle {
    shutdown: Arc<AtomicBool>,
    join_handle: Option<thread::JoinHandle<()>>,
}

impl WavLoaderHandle {
    /// Signal the loader thread to exit and wait for it to finish.
    pub(crate) fn shutdown(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(handle) = self.join_handle.take() {
            let _ = handle.join();
        }
    }
}

/// Spawn the wav entries loader worker and return its job channel plus shutdown handle.
pub(crate) fn spawn_wav_loader(
) -> (Sender<WavLoadJob>, Receiver<WavLoadResult>, WavLoaderHandle) {
    let (tx, rx) = std::sync::mpsc::channel::<WavLoadJob>();
    let (result_tx, result_rx) = std::sync::mpsc::channel::<WavLoadResult>();
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_worker = Arc::clone(&shutdown);
    let handle = thread::spawn(move || {
        while !shutdown_worker.load(Ordering::Relaxed) {
            match rx.recv_timeout(WAV_LOADER_POLL_INTERVAL) {
                Ok(job) => {
                    let start = Instant::now();
                    let (result, total) = load_entries(&job);
                    let _ = result_tx.send(WavLoadResult {
                        source_id: job.source_id.clone(),
                        result,
                        elapsed: start.elapsed(),
                        total,
                        page_index: 0,
                    });
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });
    (
        tx,
        result_rx,
        WavLoaderHandle {
            shutdown,
            join_handle: Some(handle),
        },
    )
}

pub(crate) fn load_entries(job: &WavLoadJob) -> (Result<Vec<WavEntry>, LoadEntriesError>, usize) {
    let db = match SourceDatabase::open(&job.root) {
        Ok(db) => db,
        Err(err) => return (Err(LoadEntriesError::Db(err)), 0),
    };
    match crate::sample_sources::db::file_ops_journal::reconcile_pending_ops(&db) {
        Ok(summary) => {
            if summary.total > 0 {
                if summary.errors.is_empty() {
                    tracing::info!(
                        "Reconciled {} pending file ops for {}",
                        summary.completed,
                        job.root.display()
                    );
                } else {
                    for err in summary.errors {
                        tracing::warn!(
                            "File op recovery issue for {}: {err}",
                            job.root.display()
                        );
                    }
                }
            }
        }
        Err(err) => {
            tracing::warn!(
                "Failed to reconcile file ops for {}: {err}",
                job.root.display()
            );
        }
    }
    let mut total = match db.count_files() {
        Ok(total) => total,
        Err(err) => return (Err(LoadEntriesError::Db(err)), 0),
    };
    let mut entries = match db.list_files_page(job.page_size, 0) {
        Ok(entries) => entries,
        Err(err) => return (Err(LoadEntriesError::Db(err)), total),
    };
    if entries.is_empty() {
        // New sources start empty; trigger a quick scan to populate before reporting.
        let _ = crate::sample_sources::scanner::scan_once(&db);
        total = match db.count_files() {
            Ok(total) => total,
            Err(err) => return (Err(LoadEntriesError::Db(err)), total),
        };
        entries = match db.list_files_page(job.page_size, 0) {
            Ok(entries) => entries,
            Err(err) => return (Err(LoadEntriesError::Db(err)), total),
        };
    }
    (Ok(entries), total)
}
