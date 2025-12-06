use crate::sample_sources::{SourceDatabase, SourceDbError, SourceId, WavEntry};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;

pub struct WavListJob {
    pub source_id: SourceId,
    pub root: PathBuf,
}

pub struct WavListJobResult {
    pub source_id: SourceId,
    pub result: Result<WavListPayload, SourceDbError>,
}

pub struct WavListPayload {
    pub entries: Vec<WavEntry>,
    pub missing_paths: Vec<PathBuf>,
}

pub fn spawn_wav_list_worker() -> (Sender<WavListJob>, Receiver<WavListJobResult>) {
    let (tx, rx) = channel::<WavListJob>();
    let (result_tx, result_rx) = channel::<WavListJobResult>();
    thread::spawn(move || {
        while let Ok(job) = rx.recv() {
            let result = load_entries(&job);
            let _ = result_tx.send(WavListJobResult {
                source_id: job.source_id,
                result,
            });
        }
    });
    (tx, result_rx)
}

fn load_entries(job: &WavListJob) -> Result<WavListPayload, SourceDbError> {
    let db = SourceDatabase::open(&job.root)?;
    let entries = db.list_files()?;
    let mut missing = Vec::new();
    for entry in &entries {
        let full_path = job.root.join(&entry.relative_path);
        if !full_path.exists() {
            missing.push(entry.relative_path.clone());
        }
    }
    Ok(WavListPayload {
        entries,
        missing_paths: missing,
    })
}
