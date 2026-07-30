//! Audio-related background-job handlers.

use super::helpers::{pending_audio_matches, pending_recording_waveform_matches};
use super::*;
use crate::app::controller::library::wavs::FinishWaveformLoadShared;
use crate::app::controller::playback::recording::waveform_loader::RecordingWaveformLoadResult;
use crate::app::controller::state::runtime::WavLoadResult;
use std::sync::Arc;

impl AppController {
    /// Apply one completed wav-page load to the active source browser.
    pub(super) fn handle_wav_loaded_message(&mut self, message: WavLoadResult) {
        if Some(&message.source_id) != self.selection_state.ctx.selected_source.as_ref() {
            return;
        }
        match message.result {
            Ok(entries) => {
                self.apply_wav_entries_with_params(
                    super::super::super::ui::loading::ApplyWavEntriesParams {
                        entries,
                        total: message.total,
                        page_size: self.wav_entries.page_size,
                        page_index: message.page_index,
                        from_cache: false,
                        source_id: Some(message.source_id.clone()),
                        elapsed: Some(message.elapsed),
                    },
                );
                self.cache.wav.insert_page(
                    message.source_id.clone(),
                    message.total,
                    self.wav_entries.page_size,
                    message.page_index,
                    self.wav_entries
                        .pages
                        .get(&message.page_index)
                        .cloned()
                        .unwrap_or_default(),
                );
            }
            Err(err) => self.handle_wav_load_error(&message.source_id, err),
        }
        self.runtime.jobs.clear_wav_load_pending();
        self.clear_progress_task(ProgressTaskKind::WavLoad);
    }

    /// Apply one completed audio-load worker message if it still matches the current request.
    pub(super) fn handle_audio_loaded_message(&mut self, message: AudioLoadResult) {
        match message {
            AudioLoadResult::Primary {
                request_id,
                source_id,
                relative_path,
                result,
            } => {
                let Some(pending) = self.runtime.jobs.pending_audio() else {
                    return;
                };
                if !pending_audio_matches(&pending, request_id, &source_id, &relative_path)
                    || !self.audio_target_matches_current(&source_id, &relative_path)
                {
                    return;
                }
                self.runtime.jobs.set_pending_audio(None);
                match result {
                    Ok(outcome) => self.handle_audio_loaded(pending, outcome),
                    Err(err) => self.handle_audio_load_error(pending, err),
                }
            }
            AudioLoadResult::Transients(result) => {
                self.handle_audio_transients_loaded(result);
            }
            AudioLoadResult::Visual(result) => {
                self.handle_audio_visual_loaded(result);
            }
        }
    }

    /// Apply one completed recording-waveform refresh if it still matches the active target.
    pub(super) fn handle_recording_waveform_loaded_message(
        &mut self,
        message: RecordingWaveformLoadResult,
    ) {
        let Some(pending) = self.runtime.jobs.pending_recording_waveform() else {
            return;
        };
        if !pending_recording_waveform_matches(
            &pending,
            message.request_id,
            &message.source_id,
            &message.relative_path,
        ) {
            return;
        }
        self.runtime.jobs.set_pending_recording_waveform(None);
        let target_matches = match self.audio.recording_target.as_ref() {
            Some(target) => {
                target.source_id == pending.source_id
                    && target.relative_path == pending.relative_path
                    && target.absolute_path == pending.absolute_path
            }
            None => {
                return;
            }
        };
        if !target_matches {
            return;
        }
        let now = Instant::now();
        if let Ok(update) = message.result {
            match update {
                RecordingWaveformUpdate::NoChange { file_len } => {
                    if let Some(target) = self.audio.recording_target.as_mut() {
                        target.last_file_len = file_len;
                    }
                }
                RecordingWaveformUpdate::Updated {
                    decoded,
                    bytes,
                    file_len,
                } => {
                    if let Some(source) = self
                        .library
                        .sources
                        .iter()
                        .find(|source| source.id == pending.source_id)
                        .cloned()
                    {
                        if let Some(bytes) = bytes {
                            let _ = self.finish_waveform_load_shared(FinishWaveformLoadShared {
                                source: &source,
                                relative_path: &pending.relative_path,
                                decoded: Arc::new(decoded),
                                bytes: bytes.into(),
                                audio_path: None,
                                intent: AudioLoadIntent::Selection,
                                preserve_selections: false,
                                transients: None,
                            });
                            if let Some(target) = self.audio.recording_target.as_mut() {
                                target.loaded_once = true;
                            }
                        } else {
                            self.apply_waveform_image(decoded, None);
                        }
                    }
                    if let Some(target) = self.audio.recording_target.as_mut() {
                        target.last_file_len = file_len;
                    }
                }
            }
        }
        if let Some(target) = self.audio.recording_target.as_mut() {
            target.last_refresh_at = Some(now);
        }
    }
}
