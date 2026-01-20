use super::*;
use crate::egui_app::controller::playback::audio_samples::{
    decode_samples_from_bytes, wav_bytes_from_samples,
};
use crate::egui_app::state::WaveformView;

impl EguiController {
    pub(crate) fn load_waveform_for_selection(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Result<(), String> {
        let is_refresh = self.sample_view.wav.loaded_wav.as_deref() == Some(relative_path);
        if self.sample_view.wav.selected_wav.as_deref() != Some(relative_path) {
            self.sample_view.wav.selected_wav = Some(relative_path.to_path_buf());
        }
        if is_refresh {
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
        let (decoded, bytes, stretched) = self.prepare_loaded_audio(
            source,
            relative_path,
            None,
            bytes,
            AudioLoadIntent::Selection,
        )?;
        let duration_seconds = decoded.duration_seconds;
        let sample_rate = decoded.sample_rate;
        let cache_key = CacheKey::new(&source.id, relative_path);
        if !stretched {
            self.audio
                .cache
                .insert(cache_key, metadata, decoded.clone(), bytes.clone());
        }
        self.finish_waveform_load(
            source,
            relative_path,
            decoded,
            bytes,
            AudioLoadIntent::Selection,
            is_refresh,
            None,
        )?;
        self.maybe_trigger_pending_playback();
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
        preserve_selections: bool,
        transients: Option<Vec<f32>>,
    ) -> Result<(), String> {
        let duration_seconds = decoded.duration_seconds;
        let sample_rate = decoded.sample_rate;
        self.apply_waveform_image(decoded, transients);
        if !preserve_selections {
            self.ui.waveform.view = WaveformView::default();
            self.ui.waveform.cursor = Some(0.0);
            self.clear_waveform_selection();
        }
        self.ui.waveform.notice = None;
        self.ui.waveform.loading = None;
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
            bytes,
        )?;
        if matches!(intent, AudioLoadIntent::Selection) {
            self.apply_loaded_sample_bpm(source, relative_path);
            self.apply_loaded_sample_loop_marker(source, relative_path);
        }
        Ok(())
    }

    pub(crate) fn prepare_loaded_audio(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
        decoded: Option<DecodedWaveform>,
        bytes: Vec<u8>,
        intent: AudioLoadIntent,
    ) -> Result<(DecodedWaveform, Vec<u8>, bool), String> {
        let original_decoded = match decoded {
            Some(decoded) => decoded,
            None => self
                .sample_view
                .renderer
                .decode_from_bytes(&bytes)
                .map_err(|err| err.to_string())?,
        };

        if matches!(intent, AudioLoadIntent::Selection) {
            if let Some(ratio) = self.stretch_ratio_for_sample(source, relative_path) {
                let stretched = self.stretch_wav_bytes(&bytes, ratio)?;
                // Decode the stretched bytes to get the correct duration
                let stretched_decoded = self
                    .sample_view
                    .renderer
                    .decode_from_bytes(&stretched)
                    .map_err(|err| err.to_string())?;
                return Ok((stretched_decoded, stretched, true));
            }
        }

        Ok((original_decoded, bytes, false))
    }

    pub(crate) fn stretch_ratio_for_sample(
        &self,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Option<f64> {
        if !self.ui.waveform.bpm_stretch_enabled {
            return None;
        }
        let target_bpm = self.ui.waveform.bpm_value?;
        if !target_bpm.is_finite() || target_bpm <= 0.0 {
            return None;
        }
        let source_bpm = self.load_sample_bpm_metadata(source, relative_path)?;
        if !source_bpm.is_finite() || source_bpm <= 0.0 {
            return None;
        }
        let ratio = target_bpm as f64 / source_bpm as f64;
        if !ratio.is_finite() || (ratio - 1.0).abs() < 1e-3 {
            return None;
        }
        Some(ratio.clamp(0.5, 2.0))
    }

    fn stretch_wav_bytes(&self, bytes: &[u8], ratio: f64) -> Result<Vec<u8>, String> {
        let decoded = decode_samples_from_bytes(bytes)?;
        let channels = decoded.channels.max(1) as usize;
        let wsola = crate::audio::Wsola::new(decoded.sample_rate);
        let stretched = wsola.stretch(&decoded.samples, channels, ratio);
        wav_bytes_from_samples(&stretched, decoded.sample_rate, decoded.channels)
    }

    pub(crate) fn clear_waveform_selection(&mut self) {
        self.ui.waveform.playhead = PlayheadState::default();
        self.ui.waveform.selection = None;
        self.ui.waveform.selection_duration = None;
        self.ui.waveform.edit_selection = None;
        self.selection_state.range.clear();
        self.selection_state.edit_range.clear();
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
        bytes: Vec<u8>,
    ) -> Result<(), String> {
        self.sample_view.wav.loaded_audio = Some(LoadedAudio {
            source_id: source.id.clone(),
            root: source.root.clone(),
            relative_path: relative_path.to_path_buf(),
            bytes: bytes.clone(),
            duration_seconds,
            sample_rate,
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
        self.update_loaded_sample_duration(source, relative_path, duration_seconds, sample_rate);
        Ok(())
    }

    fn update_loaded_sample_duration(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
        duration_seconds: f32,
        sample_rate: u32,
    ) {
        if !duration_seconds.is_finite() || duration_seconds <= 0.0 {
            return;
        }
        let mut content_hash = None;
        if let Some(index) = self.wav_index_for_path(relative_path) {
            if let Some(entry) = self.wav_entry(index) {
                content_hash = entry.content_hash.clone();
            }
        }
        let sample_id = analysis_jobs::build_sample_id(source.id.as_str(), relative_path);
        let conn = match analysis_jobs::open_source_db(&source.root) {
            Ok(conn) => conn,
            Err(err) => {
                tracing::warn!(
                    "Failed to open source DB for duration update ({}): {err}",
                    relative_path.display()
                );
                return;
            }
        };
        if let Err(err) = analysis_jobs::update_sample_duration(
            &conn,
            &sample_id,
            content_hash.as_deref(),
            duration_seconds,
            sample_rate,
        ) {
            tracing::warn!(
                "Failed to store duration metadata for {}: {err}",
                relative_path.display()
            );
        }
        self.update_cached_duration_for_path(
            &source.id,
            relative_path,
            duration_seconds,
            sample_rate,
        );
    }

    fn apply_loaded_sample_bpm(&mut self, source: &SampleSource, relative_path: &Path) {
        if self.ui.waveform.bpm_lock_enabled || self.ui.waveform.bpm_stretch_enabled {
            return;
        }
        let bpm = self.load_sample_bpm_metadata(source, relative_path);
        if let Some(bpm) = bpm {
            self.set_waveform_bpm_input(Some(bpm));
        }
    }

    fn apply_loaded_sample_loop_marker(&mut self, source: &SampleSource, relative_path: &Path) {
        let looped = match self.database_for(source) {
            Ok(db) => match db.looped_for_path(relative_path) {
                Ok(Some(looped)) => looped,
                Ok(None) => return,
                Err(err) => {
                    tracing::warn!("Failed to load loop marker for {}: {err}", relative_path.display());
                    return;
                }
            },
            Err(err) => {
                tracing::warn!("Failed to access database for loop marker: {err}");
                return;
            }
        };
        self.ui.waveform.loop_enabled = looped;
    }

    fn load_sample_bpm_metadata(
        &self,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Option<f32> {
        let sample_id = analysis_jobs::build_sample_id(source.id.as_str(), relative_path);
        let conn = match analysis_jobs::open_source_db(&source.root) {
            Ok(conn) => conn,
            Err(err) => {
                tracing::warn!("Failed to open source DB for BPM load: {err}");
                return None;
            }
        };
        match analysis_jobs::sample_bpm(&conn, &sample_id) {
            Ok(Some(bpm)) => Some(bpm),
            Ok(None) => None,
            Err(err) => {
                tracing::warn!("Failed to load BPM metadata for {sample_id}: {err}");
                None
            }
        }
    }

    /// Update the waveform BPM input fields to match stored metadata.
    pub(crate) fn set_waveform_bpm_input(&mut self, bpm: Option<f32>) {
        let bpm = bpm.filter(|value| value.is_finite() && *value > 0.0);
        self.ui.waveform.bpm_value = bpm;
        if let Some(value) = bpm {
            let rounded = value.round();
            if (value - rounded).abs() < 0.01 {
                self.ui.waveform.bpm_input = format!("{rounded:.0}");
            } else {
                self.ui.waveform.bpm_input = format!("{value:.2}");
            }
        } else {
            self.ui.waveform.bpm_input.clear();
        }
    }

    pub(crate) fn clear_loaded_audio_and_waveform_visuals(&mut self) {
        self.sample_view.wav.loaded_audio = None;
        self.sample_view.waveform.decoded = None;
        self.ui.waveform.image = None;
        self.ui.waveform.playhead = PlayheadState::default();
        self.ui.waveform.selection = None;
        self.ui.waveform.selection_duration = None;
        self.ui.waveform.edit_selection = None;
        self.selection_state.range.clear();
        self.selection_state.edit_range.clear();
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
