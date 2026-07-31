use super::super::super::*;
use crate::app::controller::playback::telemetry::{log_audio_start_stage, stage_timer};
use std::sync::Arc;

impl AppController {
    fn apply_loaded_audio_handoff(
        &mut self,
        source: &SampleSource,
        handoff: &StagedAudioHandoff,
    ) -> Result<(), String> {
        let started_at = stage_timer();
        let relative_path = handoff.relative_path.as_path();
        let duration_seconds = handoff.decoded.duration_seconds;
        let sample_rate = handoff.decoded.sample_rate;
        self.sample_view.waveform.decoded = Some(Arc::clone(&handoff.decoded));
        self.sample_view.wav.loaded_wav = Some(handoff.relative_path.clone());
        self.set_ui_loaded_wav(Some(handoff.relative_path.clone()));
        self.sync_loaded_audio(
            source,
            relative_path,
            duration_seconds,
            sample_rate,
            handoff.decoded.channels,
            Arc::clone(&handoff.decoded.samples),
            Arc::clone(&handoff.bytes),
            handoff.audio_path.clone(),
        )?;
        log_audio_start_stage(
            "apply_loaded_audio_handoff",
            Some(&source.id),
            Some(relative_path),
            started_at,
            Some(if handoff.bytes.is_empty() {
                "file"
            } else {
                "bytes"
            }),
            None,
            Some(handoff.bytes.len()),
            Some(handoff.decoded.samples.len()),
        );
        self.ui.waveform.notice = None;
        if matches!(handoff.intent, AudioLoadIntent::Selection) {
            self.apply_loaded_sample_bpm(relative_path);
            self.apply_loaded_sample_loop_marker(source, relative_path);
        }
        Ok(())
    }

    /// Publish one staged audio load once waveform visuals for the same decode are ready.
    pub(crate) fn finalize_staged_audio_handoff(&mut self, cache_token: u64) {
        let started_at = stage_timer();
        if self.runtime.waveform.pending_render.is_some() {
            return;
        }
        let Some(staged) = self.runtime.jobs.staged_audio_handoff() else {
            return;
        };
        if !self.audio_target_matches_current(&staged.source_id, &staged.relative_path) {
            return;
        }
        if staged.decoded.cache_token != cache_token {
            return;
        }
        let source = SampleSource::new_with_id(staged.source_id.clone(), staged.root.clone());
        if let Err(err) = self.apply_loaded_audio_handoff(&source, &staged) {
            self.runtime.jobs.set_staged_audio_handoff(None);
            self.runtime.jobs.set_pending_playback(None);
            self.clear_browser_selection_transition(&staged.source_id, &staged.relative_path);
            self.set_status(err, StatusTone::Error);
            return;
        }
        self.runtime.jobs.set_staged_audio_handoff(None);
        self.ui.waveform.loading = None;
        self.clear_browser_selection_transition(&staged.source_id, &staged.relative_path);
        if matches!(staged.intent, AudioLoadIntent::Selection) {
            self.refresh_similarity_sort_for_loaded_sample();
        }
        self.maybe_trigger_pending_playback();
        log_audio_start_stage(
            "finalize_staged_audio_handoff",
            Some(&staged.source_id),
            Some(&staged.relative_path),
            started_at,
            Some(if staged.bytes.is_empty() {
                "file"
            } else {
                "bytes"
            }),
            None,
            Some(staged.bytes.len()),
            Some(staged.decoded.samples.len()),
        );
    }
}
