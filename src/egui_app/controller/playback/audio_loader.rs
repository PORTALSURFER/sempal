use super::*;
use crate::egui_app::controller::playback::audio_cache::FileMetadata;
use crate::waveform::{DecodedWaveform, WaveformRenderer};
use std::{
    fs,
    path::{Component, Path, PathBuf},
    sync::mpsc::{Receiver, Sender},
    thread,
};

pub(crate) struct AudioLoadJob {
    pub request_id: u64,
    pub source_id: SourceId,
    pub root: PathBuf,
    pub relative_path: PathBuf,
    pub stretch_ratio: Option<f64>,
}

pub(crate) struct AudioLoadOutcome {
    pub decoded: DecodedWaveform,
    pub bytes: Vec<u8>,
    pub metadata: FileMetadata,
    pub transients: Vec<f32>,
    pub stretched: bool,
}

#[derive(Debug)]
pub(crate) enum AudioLoadError {
    Missing(String),
    Failed(String),
}

pub(crate) struct AudioLoadResult {
    pub request_id: u64,
    pub source_id: SourceId,
    pub relative_path: PathBuf,
    pub result: Result<AudioLoadOutcome, AudioLoadError>,
}

pub(crate) fn spawn_audio_loader(
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
    ensure_safe_relative_path(&job.relative_path)?;
    let full_path = job.root.join(&job.relative_path);
    let metadata = fs::metadata(&full_path).map_err(|err| {
        let missing = err.kind() == std::io::ErrorKind::NotFound;
        if missing {
            AudioLoadError::Missing(format!("File missing: {} ({err})", full_path.display()))
        } else {
            AudioLoadError::Failed(format!(
                "Failed to read metadata for {}: {err}",
                full_path.display()
            ))
        }
    })?;
    let bytes = fs::read(&full_path).map_err(|err| {
        let missing = err.kind() == std::io::ErrorKind::NotFound;
        if missing {
            AudioLoadError::Missing(format!("File missing: {} ({err})", full_path.display()))
        } else {
            AudioLoadError::Failed(format!("Failed to read {}: {err}", full_path.display()))
        }
    })?;
    let bytes = crate::wav_sanitize::sanitize_wav_bytes(bytes);
    let modified_ns = metadata
        .modified()
        .map_err(|err| {
            AudioLoadError::Failed(format!(
                "Missing modified time for {}: {err}",
                full_path.display()
            ))
        })?
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map_err(|_| {
            AudioLoadError::Failed(format!(
                "File modified time is before epoch: {}",
                full_path.display()
            ))
        })?
        .as_nanos() as i64;
    let mut decoded = renderer
        .decode_from_bytes(&bytes)
        .map_err(|err| AudioLoadError::Failed(err.to_string()))?;

    let mut stretched = false;
    let mut final_bytes = bytes;

    if let Some(ratio) = job.stretch_ratio {
        let wsola = crate::audio::Wsola::new(decoded.sample_rate);
        let stretched_samples = wsola.stretch(&decoded.samples, decoded.channel_count(), ratio);
        match crate::egui_app::controller::playback::audio_samples::wav_bytes_from_samples(
            &stretched_samples,
            decoded.sample_rate,
            decoded.channels,
        ) {
            Ok(b) => {
                final_bytes = b;
                stretched = true;
                // Decode the stretched bytes to get the correct duration and cache token
                if let Ok(d) = renderer.decode_from_bytes(&final_bytes) {
                    decoded = d;
                }
            }
            Err(err) => {
                tracing::warn!("Failed to stretch audio in background: {err}");
            }
        }
    }

    let transients = crate::waveform::transients::detect_transients(
        &decoded,
        crate::egui_app::controller::library::wavs::waveform_rendering::DEFAULT_TRANSIENT_SENSITIVITY,
    );

    Ok(AudioLoadOutcome {
        decoded,
        bytes: final_bytes,
        metadata: FileMetadata {
            file_size: metadata.len(),
            modified_ns,
        },
        transients,
        stretched,
    })
}

fn ensure_safe_relative_path(path: &Path) -> Result<(), AudioLoadError> {
    let mut saw_component = false;
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(_) => {
                saw_component = true;
            }
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(AudioLoadError::Failed(format!(
                    "Invalid relative path: {}",
                    path.display()
                )));
            }
        }
    }
    if !saw_component {
        return Err(AudioLoadError::Failed(format!(
            "Invalid relative path: {}",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::ensure_safe_relative_path;
    use std::path::Path;

    #[test]
    fn ensure_safe_relative_path_rejects_parent_dir() {
        let err = ensure_safe_relative_path(Path::new("../escape.wav")).unwrap_err();
        assert!(matches!(err, super::AudioLoadError::Failed(_)));
    }

    #[test]
    fn ensure_safe_relative_path_accepts_normal_relative_paths() {
        ensure_safe_relative_path(Path::new("folder/./file.wav")).unwrap();
    }
}
