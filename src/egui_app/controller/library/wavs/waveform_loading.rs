use super::*;
use crate::egui_app::state::WaveformView;

impl EguiController {
    pub(crate) fn load_waveform_for_selection(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Result<(), String> {
        if self.sample_view.wav.selected_wav.as_deref() != Some(relative_path) {
            self.sample_view.wav.selected_wav = Some(relative_path.to_path_buf());
        }
        if self.sample_view.wav.loaded_wav.as_deref() == Some(relative_path) {
            self.clear_waveform_selection();
            let message = self
                .loaded_audio_for(source, relative_path)
                .map(|audio| {
                    Self::loaded_status_text(
                        relative_path,
                        audio.duration_seconds,
                        audio.sample_rate,
                    )
                })
                .unwrap_or_else(|| format!("Loaded {}", relative_path.display()));
            self.set_status(message, StatusTone::Info);
            return Ok(());
        }
        if self.try_use_cached_audio(source, relative_path, AudioLoadIntent::Selection)? {
            return Ok(());
        }
        let metadata = match self.current_file_metadata(source, relative_path) {
            Ok(meta) => meta,
            Err(err) => {
                self.mark_sample_missing(source, relative_path);
                self.show_missing_waveform_notice(relative_path);
                return Err(err);
            }
        };
        let bytes = match self.read_waveform_bytes(source, relative_path) {
            Ok(bytes) => bytes,
            Err(err) => {
                self.mark_sample_missing(source, relative_path);
                self.show_missing_waveform_notice(relative_path);
                return Err(err);
            }
        };
        let decoded = self
            .sample_view
            .renderer
            .decode_from_bytes(&bytes)
            .map_err(|err| err.to_string())?;
        let duration_seconds = decoded.duration_seconds;
        let sample_rate = decoded.sample_rate;
        let cache_key = CacheKey::new(&source.id, relative_path);
        self.audio
            .cache
            .insert(cache_key, metadata, decoded.clone(), bytes.clone());
        self.finish_waveform_load(
            source,
            relative_path,
            decoded,
            bytes,
            AudioLoadIntent::Selection,
        )?;
        let message = Self::loaded_status_text(relative_path, duration_seconds, sample_rate);
        self.set_status(message, StatusTone::Info);
        self.refresh_similarity_sort_for_loaded_sample();
        Ok(())
    }

    pub(crate) fn load_collection_waveform(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Result<(), String> {
        self.queue_audio_load_for(
            source,
            relative_path,
            AudioLoadIntent::CollectionPreview,
            None,
        )
    }

    pub(crate) fn finish_waveform_load(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
        decoded: DecodedWaveform,
        bytes: Vec<u8>,
        intent: AudioLoadIntent,
    ) -> Result<(), String> {
        let duration_seconds = decoded.duration_seconds;
        let sample_rate = decoded.sample_rate;
        let channels = decoded.channels;
        self.apply_waveform_image(decoded);
        self.ui.waveform.view = WaveformView::default();
        self.ui.waveform.cursor = Some(0.0);
        self.ui.waveform.notice = None;
        self.ui.waveform.loading = None;
        self.clear_waveform_selection();
        self.clear_waveform_slices();
        self.runtime.jobs.set_pending_audio(None);
        match intent {
            AudioLoadIntent::Selection => {
                self.sample_view.wav.loaded_wav = Some(relative_path.to_path_buf());
                self.ui.loaded_wav = Some(relative_path.to_path_buf());
            }
            AudioLoadIntent::CollectionPreview => {
                self.sample_view.wav.loaded_wav = None;
                self.ui.loaded_wav = None;
            }
        }
        self.sync_loaded_audio(
            source,
            relative_path,
            duration_seconds,
            sample_rate,
            channels,
            bytes,
        )
    }

    pub(crate) fn clear_waveform_selection(&mut self) {
        self.ui.waveform.playhead = PlayheadState::default();
        self.ui.waveform.selection = None;
        self.ui.waveform.selection_duration = None;
        self.selection_state.range.clear();
    }

    pub(crate) fn loaded_status_text(
        relative_path: &Path,
        duration_seconds: f32,
        sample_rate: u32,
    ) -> String {
        let duration_label = Self::format_duration(duration_seconds);
        let rate_label = Self::format_sample_rate(sample_rate);
        format!(
            "Loaded {} ({duration_label} @ {rate_label})",
            relative_path.display()
        )
    }

    fn loaded_audio_for(
        &self,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Option<&LoadedAudio> {
        self.sample_view
            .wav
            .loaded_audio
            .as_ref()
            .filter(|audio| audio.source_id == source.id && audio.relative_path == relative_path)
    }

    fn format_duration(duration_seconds: f32) -> String {
        if !duration_seconds.is_finite() || duration_seconds <= 0.0 {
            return "0.00s".into();
        }
        if duration_seconds < 1.0 {
            return format!("{:.0} ms", duration_seconds * 1_000.0);
        }
        if duration_seconds < 60.0 {
            return format!("{:.2} s", duration_seconds);
        }
        let minutes = (duration_seconds / 60.0).floor() as u32;
        let seconds = duration_seconds - minutes as f32 * 60.0;
        format!("{minutes}m {seconds:05.2}s")
    }

    fn format_sample_rate(sample_rate: u32) -> String {
        if sample_rate == 0 {
            return "unknown".into();
        }
        if sample_rate >= 1_000 {
            return format!("{:.1} kHz", sample_rate as f32 / 1_000.0);
        }
        format!("{sample_rate} Hz")
    }

    pub(crate) fn invalidate_cached_audio(
        &mut self,
        source_id: &SourceId,
        relative_path: &Path,
    ) {
        let key = CacheKey::new(source_id, relative_path);
        self.audio.cache.invalidate(&key);
    }

    fn sync_loaded_audio(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
        duration_seconds: f32,
        sample_rate: u32,
        channels: u16,
        bytes: Vec<u8>,
    ) -> Result<(), String> {
        self.sample_view.wav.loaded_audio = Some(LoadedAudio {
            source_id: source.id.clone(),
            relative_path: relative_path.to_path_buf(),
            bytes: bytes.clone(),
            duration_seconds,
            sample_rate,
            channels,
        });
        match self.ensure_player() {
            Ok(Some(player)) => {
                let mut player = player.borrow_mut();
                player.stop();
                player.set_audio(bytes, duration_seconds);
            }
            Ok(None) => {}
            Err(err) => self.set_status(err, StatusTone::Warning),
        }
        Ok(())
    }

    pub(crate) fn clear_loaded_audio_and_waveform_visuals(&mut self) {
        self.sample_view.wav.loaded_audio = None;
        self.sample_view.waveform.decoded = None;
        self.ui.waveform.image = None;
        self.ui.waveform.playhead = PlayheadState::default();
        self.ui.waveform.selection = None;
        self.ui.waveform.selection_duration = None;
        self.selection_state.range.clear();
        self.clear_waveform_slices();
    }

    pub(crate) fn reload_waveform_for_selection_if_active(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
    ) {
        self.invalidate_cached_audio(&source.id, relative_path);
        let loaded_matches = self
            .sample_view
            .wav
            .loaded_audio
            .as_ref()
            .is_some_and(|audio| {
                audio.source_id == source.id && audio.relative_path == relative_path
            });
        let selected_matches = self.selection_state.ctx.selected_source.as_ref()
            == Some(&source.id)
            && self.sample_view.wav.selected_wav.as_deref() == Some(relative_path);
        if selected_matches || loaded_matches {
            let preserved_view = self.ui.waveform.view;
            self.sample_view.wav.loaded_wav = None;
            self.ui.loaded_wav = None;
            if let Err(err) = self.load_waveform_for_selection(source, relative_path) {
                self.set_status(err, StatusTone::Warning);
            } else {
                self.ui.waveform.view = preserved_view;
                self.refresh_waveform_image();
            }
        }
    }
}
