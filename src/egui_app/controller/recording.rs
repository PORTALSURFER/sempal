use super::*;
use crate::audio::{AudioRecorder, RecordingOutcome};
use std::path::PathBuf;
use time::format_description::FormatItem;
use time::macros::format_description;

const RECORDINGS_DIR_NAME: &str = "recordings";
const RECORDING_FILE_PREFIX: &str = "recording_";
const RECORDING_FILE_EXT: &str = "wav";

impl EguiController {
    pub fn is_recording(&self) -> bool {
        self.audio.recorder.is_some()
    }

    pub fn start_recording(&mut self) -> Result<(), String> {
        if self.is_recording() {
            return Ok(());
        }
        if self.is_playing() {
            self.stop_playback_if_active();
        }
        let output_path = self.next_recording_path()?;
        let recorder = AudioRecorder::start(&self.settings.audio_input, output_path.clone())
            .map_err(|err| err.to_string())?;
        self.update_audio_input_status(recorder.resolved());
        self.audio.recorder = Some(recorder);
        self.set_status(
            format!("Recording to {}", output_path.display()),
            StatusTone::Busy,
        );
        Ok(())
    }

    pub fn stop_recording(&mut self) -> Result<Option<RecordingOutcome>, String> {
        let Some(recorder) = self.audio.recorder.take() else {
            return Ok(None);
        };
        let outcome = recorder.stop().map_err(|err| err.to_string())?;
        self.set_status(
            format!(
                "Recorded {:.2}s to {}",
                outcome.duration_seconds,
                outcome.path.display()
            ),
            StatusTone::Info,
        );
        Ok(Some(outcome))
    }

    pub fn stop_recording_and_load(&mut self) -> Result<(), String> {
        let Some(outcome) = self.stop_recording()? else {
            return Ok(());
        };
        let source = self.ensure_recordings_source(&outcome.path)?;
        let relative_path = outcome
            .path
            .strip_prefix(&source.root)
            .map_err(|_| "Failed to resolve recording path".to_string())?
            .to_path_buf();
        self.load_waveform_for_selection(&source, &relative_path)?;
        Ok(())
    }

    fn ensure_recordings_source(&mut self, recording_path: &PathBuf) -> Result<SampleSource, String> {
        let root = recording_path
            .parent()
            .ok_or_else(|| "Recording path missing parent".to_string())?
            .to_path_buf();
        if let Some(existing) = self.library.sources.iter().find(|s| s.root == root) {
            self.select_source(Some(existing.id.clone()));
            return Ok(existing.clone());
        }
        let source = match crate::sample_sources::library::lookup_source_id_for_root(&root) {
            Ok(Some(id)) => SampleSource::new_with_id(id, root.clone()),
            Ok(None) => SampleSource::new(root.clone()),
            Err(err) => {
                self.set_status(
                    format!("Could not check library history (continuing): {err}"),
                    StatusTone::Warning,
                );
                SampleSource::new(root.clone())
            }
        };
        SourceDatabase::open(&root)
            .map_err(|err| format!("Failed to create recordings database: {err}"))?;
        let _ = self.cache_db(&source);
        self.library.sources.push(source.clone());
        self.select_source(Some(source.id.clone()));
        self.persist_config("Failed to save config after adding recordings source")?;
        self.prepare_similarity_for_selected_source();
        Ok(source)
    }

    fn next_recording_path(&mut self) -> Result<PathBuf, String> {
        let root = crate::app_dirs::app_root_dir()
            .map_err(|err| format!("Failed to resolve app folder: {err}"))?;
        let recordings = root.join(RECORDINGS_DIR_NAME);
        std::fs::create_dir_all(&recordings).map_err(|err| {
            format!(
                "Failed to create recordings folder {}: {err}",
                recordings.display()
            )
        })?;
        let filename = format!(
            "{RECORDING_FILE_PREFIX}{}.{RECORDING_FILE_EXT}",
            formatted_timestamp()
        );
        Ok(recordings.join(filename))
    }
}

fn formatted_timestamp() -> String {
    const FORMAT: &[FormatItem<'_>] =
        format_description!("[year][month][day]_[hour][minute][second]");
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    now.format(&FORMAT).unwrap_or_else(|_| "unknown".into())
}
