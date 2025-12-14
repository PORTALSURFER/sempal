use super::audio_cache::CacheKey;
use super::*;
use std::path::Path;

impl EguiController {
    pub(in crate::egui_app::controller) fn poll_audio_loader(&mut self) {
        while let Ok(message) = self.runtime.jobs.audio_job_rx.try_recv() {
            let Some(pending) = self.runtime.jobs.pending_audio.clone() else {
                continue;
            };
            if message.request_id != pending.request_id
                || message.source_id != pending.source_id
                || message.relative_path != pending.relative_path
            {
                continue;
            }
            self.runtime.jobs.pending_audio = None;
            self.ui.waveform.loading = None;
            match message.result {
                Ok(outcome) => self.handle_audio_loaded(pending, outcome),
                Err(err) => self.handle_audio_load_error(pending, err),
            }
        }
    }

    fn handle_audio_loaded(&mut self, pending: PendingAudio, outcome: AudioLoadOutcome) {
        let source = SampleSource {
            id: pending.source_id.clone(),
            root: pending.root.clone(),
        };
        let AudioLoadOutcome {
            decoded,
            bytes,
            metadata,
        } = outcome;
        let duration_seconds = decoded.duration_seconds;
        let sample_rate = decoded.sample_rate;
        let cache_key = CacheKey::new(&source.id, &pending.relative_path);
        self.audio.cache
            .insert(cache_key, metadata, decoded.clone(), bytes.clone());
        if let Err(err) = self.finish_waveform_load(
            &source,
            &pending.relative_path,
            decoded,
            bytes,
            pending.intent,
        ) {
            self.runtime.jobs.pending_playback = None;
            self.set_status(err, StatusTone::Error);
            return;
        }
        let message =
            Self::loaded_status_text(&pending.relative_path, duration_seconds, sample_rate);
        self.set_status(message, StatusTone::Info);
        self.maybe_trigger_pending_playback();
    }

    fn handle_audio_load_error(&mut self, pending: PendingAudio, error: AudioLoadError) {
        let source = SampleSource {
            id: pending.source_id.clone(),
            root: pending.root.clone(),
        };
        if self.runtime.jobs.pending_playback.as_ref().is_some_and(|pending_play| {
            pending_play.source_id == pending.source_id
                && pending_play.relative_path == pending.relative_path
        }) {
            self.runtime.jobs.pending_playback = None;
        }
        match error {
            AudioLoadError::Missing(msg) => {
                self.mark_sample_missing(&source, &pending.relative_path);
                self.show_missing_waveform_notice(&pending.relative_path);
                self.set_status(msg, StatusTone::Warning);
            }
            AudioLoadError::Failed(msg) => {
                self.set_status(msg, StatusTone::Error);
            }
        }
    }

    fn maybe_trigger_pending_playback(&mut self) {
        let Some(pending) = self.runtime.jobs.pending_playback.clone() else {
            return;
        };
        let Some(audio) = self.wav_selection.loaded_audio.as_ref() else {
            return;
        };
        if audio.source_id != pending.source_id || audio.relative_path != pending.relative_path {
            return;
        }
        self.runtime.jobs.pending_playback = None;
        if let Err(err) = self.play_audio(pending.looped, pending.start_override) {
            self.set_status(err, StatusTone::Error);
        }
    }

    pub(in crate::egui_app::controller) fn queue_audio_load_for(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
        intent: AudioLoadIntent,
        pending_playback: Option<PendingPlayback>,
    ) -> Result<(), String> {
        let request_id = self.runtime.jobs.next_audio_request_id;
        self.runtime.jobs.next_audio_request_id = self.runtime.jobs.next_audio_request_id.wrapping_add(1).max(1);
        let pending = PendingAudio {
            request_id,
            source_id: source.id.clone(),
            root: source.root.clone(),
            relative_path: relative_path.to_path_buf(),
            intent,
        };
        let job = AudioLoadJob {
            request_id,
            source_id: source.id.clone(),
            root: source.root.clone(),
            relative_path: relative_path.to_path_buf(),
        };
        self.runtime.jobs.pending_audio = None;
        self.runtime.jobs.pending_playback = pending_playback;
        self.ui.waveform.loading = Some(relative_path.to_path_buf());
        self.ui.waveform.notice = None;
        self.waveform.render_meta = None;
        self.waveform.decoded = None;
        self.ui.waveform.image = None;
        self.wav_selection.loaded_audio = None;
        self.wav_selection.loaded_wav = None;
        self.ui.loaded_wav = None;
        self.stop_playback_if_active();
        self.clear_waveform_selection();
        self.set_status(format!("Loading {}", relative_path.display()), StatusTone::Busy);
        if self.try_use_cached_audio(source, relative_path, intent)? {
            self.maybe_trigger_pending_playback();
            return Ok(());
        }
        self.runtime.jobs
            .audio_job_tx
            .send(job)
            .map_err(|_| "Failed to queue audio load".to_string())?;
        self.runtime.jobs.pending_audio = Some(pending);
        Ok(())
    }

    pub(super) fn try_use_cached_audio(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
        intent: AudioLoadIntent,
    ) -> Result<bool, String> {
        let metadata = match self.current_file_metadata(source, relative_path) {
            Ok(meta) => meta,
            Err(_) => return Ok(false),
        };
        let key = CacheKey::new(&source.id, relative_path);
        let Some(hit) = self.audio.cache.get(&key, metadata) else {
            return Ok(false);
        };
        let duration_seconds = hit.decoded.duration_seconds;
        let sample_rate = hit.decoded.sample_rate;
        self.finish_waveform_load(source, relative_path, hit.decoded, hit.bytes, intent)?;
        let message = Self::loaded_status_text(relative_path, duration_seconds, sample_rate);
        self.set_status(message, StatusTone::Info);
        Ok(true)
    }
}
