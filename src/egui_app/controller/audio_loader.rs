use super::*;
use crate::waveform::{DecodedWaveform, WaveformRenderer};
use std::{
    fs,
    path::PathBuf,
    sync::mpsc::{Receiver, Sender},
    thread,
};

pub(super) struct AudioLoadJob {
    pub request_id: u64,
    pub source_id: SourceId,
    pub root: PathBuf,
    pub relative_path: PathBuf,
}

pub(super) struct AudioLoadOutcome {
    pub decoded: DecodedWaveform,
    pub bytes: Vec<u8>,
}

#[derive(Debug)]
pub(super) enum AudioLoadError {
    Missing(String),
    Failed(String),
}

pub(super) struct AudioLoadResult {
    pub request_id: u64,
    pub source_id: SourceId,
    pub relative_path: PathBuf,
    pub result: Result<AudioLoadOutcome, AudioLoadError>,
}

pub(super) fn spawn_audio_loader(
    renderer: WaveformRenderer,
) -> (Sender<AudioLoadJob>, Receiver<AudioLoadResult>) {
    let (tx, rx) = std::sync::mpsc::channel::<AudioLoadJob>();
    let (result_tx, result_rx) = std::sync::mpsc::channel::<AudioLoadResult>();
    thread::spawn(move || {
        while let Ok(job) = rx.recv() {
            let outcome = load_audio(&renderer, &job);
            let _ = result_tx.send(AudioLoadResult {
                request_id: job.request_id,
                source_id: job.source_id.clone(),
                relative_path: job.relative_path.clone(),
                result: outcome,
            });
        }
    });
    (tx, result_rx)
}

fn load_audio(
    renderer: &WaveformRenderer,
    job: &AudioLoadJob,
) -> Result<AudioLoadOutcome, AudioLoadError> {
    let full_path = job.root.join(&job.relative_path);
    let bytes = fs::read(&full_path).map_err(|err| {
        let missing = err.kind() == std::io::ErrorKind::NotFound;
        if missing {
            AudioLoadError::Missing(format!("File missing: {} ({err})", full_path.display()))
        } else {
            AudioLoadError::Failed(format!("Failed to read {}: {err}", full_path.display()))
        }
    })?;
    let decoded = renderer
        .decode_from_bytes(&bytes)
        .map_err(AudioLoadError::Failed)?;
    Ok(AudioLoadOutcome { decoded, bytes })
}
