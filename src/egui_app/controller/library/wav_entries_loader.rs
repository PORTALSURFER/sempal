use super::{LoadEntriesError, SourceDatabase, WavEntry, WavLoadJob, WavLoadResult};
use std::{
    sync::mpsc::{Receiver, Sender},
    thread,
    time::Instant,
};

pub(crate) fn spawn_wav_loader() -> (Sender<WavLoadJob>, Receiver<WavLoadResult>) {
    let (tx, rx) = std::sync::mpsc::channel::<WavLoadJob>();
    let (result_tx, result_rx) = std::sync::mpsc::channel::<WavLoadResult>();
    thread::spawn(move || {
        while let Ok(job) = rx.recv() {
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
    });
    (tx, result_rx)
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
