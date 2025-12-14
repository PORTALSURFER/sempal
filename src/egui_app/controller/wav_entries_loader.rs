use super::{LoadEntriesError, SourceDatabase, WavEntry, WavLoadJob, WavLoadResult};
use std::{
    sync::mpsc::{Receiver, Sender},
    thread,
    time::Instant,
};

pub(super) fn spawn_wav_loader() -> (Sender<WavLoadJob>, Receiver<WavLoadResult>) {
    let (tx, rx) = std::sync::mpsc::channel::<WavLoadJob>();
    let (result_tx, result_rx) = std::sync::mpsc::channel::<WavLoadResult>();
    thread::spawn(move || {
        while let Ok(job) = rx.recv() {
            let start = Instant::now();
            let result = load_entries(&job);
            let _ = result_tx.send(WavLoadResult {
                source_id: job.source_id.clone(),
                result,
                elapsed: start.elapsed(),
            });
        }
    });
    (tx, result_rx)
}

pub(super) fn load_entries(job: &WavLoadJob) -> Result<Vec<WavEntry>, LoadEntriesError> {
    let db = SourceDatabase::open(&job.root).map_err(LoadEntriesError::Db)?;
    let mut entries = db.list_files().map_err(LoadEntriesError::Db)?;
    if entries.is_empty() {
        // New sources start empty; trigger a quick scan to populate before reporting.
        let _ = crate::sample_sources::scanner::scan_once(&db);
        entries = db.list_files().map_err(LoadEntriesError::Db)?;
    }
    Ok(entries)
}
