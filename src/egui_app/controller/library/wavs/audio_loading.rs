use crate::egui_app::controller::playback::audio_cache::CacheKey;
use super::*;
use std::path::Path;

impl EguiController {
    pub(crate) fn handle_audio_loaded(
        &mut self,
        pending: PendingAudio,
        outcome: AudioLoadOutcome,
    ) {
        let source = SampleSource {
            id: pending.source_id.clone(),
            root: pending.root.clone(),
        };
        let AudioLoadOutcome {
            decoded,
            bytes,
            metadata,
        } = outcome;
        let (decoded, bytes, stretched) = match self.prepare_loaded_audio(
            &source,
            &pending.relative_path,
            Some(decoded),
            bytes,
            pending.intent,
        ) {
            Ok(result) => result,
            Err(err) => {
                self.runtime.jobs.set_pending_playback(None);
                self.set_status(err, StatusTone::Error);
                return;
            }
        };
        let duration_seconds = decoded.duration_seconds;
        let sample_rate = decoded.sample_rate;
        let cache_key = CacheKey::new(&source.id, &pending.relative_path);
        if !stretched {
            self.audio
                .cache
                .insert(cache_key, metadata, decoded.clone(), bytes.clone());
        }
        let preserve_selections = self.sample_view.wav.loaded_wav.as_deref() == Some(&pending.relative_path);
        if let Err(err) = self.finish_waveform_load(
            &source,
            &pending.relative_path,
            decoded,
            bytes,
            pending.intent,
            preserve_selections,
        ) {
            self.runtime.jobs.set_pending_playback(None);
            self.set_status(err, StatusTone::Error);
            return;
        }
        let message =
            Self::loaded_status_text(&pending.relative_path, duration_seconds, sample_rate);
        self.set_status(message, StatusTone::Info);
        if matches!(pending.intent, AudioLoadIntent::Selection) {
            self.refresh_similarity_sort_for_loaded_sample();
        }
        self.maybe_trigger_pending_playback();
    }

    pub(crate) fn handle_audio_load_error(
        &mut self,
        pending: PendingAudio,
        error: AudioLoadError,
    ) {
        let source = SampleSource {
            id: pending.source_id.clone(),
            root: pending.root.clone(),
        };
        if self
            .runtime
            .jobs
            .pending_playback()
            .as_ref()
            .is_some_and(|pending_play| {
                pending_play.source_id == pending.source_id
                    && pending_play.relative_path == pending.relative_path
            })
        {
            self.runtime.jobs.set_pending_playback(None);
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

    pub(crate) fn maybe_trigger_pending_playback(&mut self) {
        let Some(pending) = self.runtime.jobs.pending_playback() else {
            return;
        };
        let Some(audio) = self.sample_view.wav.loaded_audio.as_ref() else {
            return;
        };
        if audio.source_id != pending.source_id || audio.relative_path != pending.relative_path {
            return;
        }
        self.runtime.jobs.set_pending_playback(None);
        if let Err(err) = self.play_audio(pending.looped, pending.start_override) {
            self.set_status(err, StatusTone::Error);
        }
    }

    pub(crate) fn queue_audio_load_for(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
        intent: AudioLoadIntent,
        pending_playback: Option<PendingPlayback>,
    ) -> Result<(), String> {
        let request_id = self.runtime.jobs.next_audio_request_id();
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
        self.runtime.jobs.set_pending_audio(None);
        self.runtime.jobs.set_pending_playback(pending_playback);
        self.ui.waveform.loading = Some(relative_path.to_path_buf());
        self.ui.waveform.notice = None;
        self.sample_view.waveform.render_meta = None;
        self.sample_view.waveform.decoded = None;
        self.ui.waveform.image = None;
        self.sample_view.wav.loaded_audio = None;
        self.sample_view.wav.loaded_wav = None;
        self.ui.loaded_wav = None;
        self.stop_playback_if_active();
        self.clear_waveform_selection();
        self.set_status(
            format!("Loading {}", relative_path.display()),
            StatusTone::Busy,
        );
        if self.try_use_cached_audio(source, relative_path, intent)? {
            self.maybe_trigger_pending_playback();
            return Ok(());
        }
        self.runtime
            .jobs
            .send_audio_job(job)
            .map_err(|()| "Failed to queue audio load".to_string())?;
        self.runtime.jobs.set_pending_audio(Some(pending));
        Ok(())
    }

    pub(crate) fn try_use_cached_audio(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
        intent: AudioLoadIntent,
    ) -> Result<bool, String> {
        if matches!(intent, AudioLoadIntent::Selection)
            && self.stretch_ratio_for_sample(source, relative_path).is_some()
        {
            return Ok(false);
        }
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
        let preserve_selections = self.sample_view.wav.loaded_wav.as_deref() == Some(relative_path);
        self.finish_waveform_load(source, relative_path, hit.decoded, hit.bytes, intent, preserve_selections)?;
        let message = Self::loaded_status_text(relative_path, duration_seconds, sample_rate);
        self.set_status(message, StatusTone::Info);
        if matches!(intent, AudioLoadIntent::Selection) {
            self.refresh_similarity_sort_for_loaded_sample();
        }
        Ok(true)
    }
}
