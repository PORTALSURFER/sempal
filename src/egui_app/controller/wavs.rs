use super::audio_cache::CacheKey;
use super::*;
use crate::egui_app::state::{FocusContext, WaveformView};
use crate::egui_app::view_model;
use crate::waveform::DecodedWaveform;
use std::path::{Path, PathBuf};

mod browser_search;
mod browser_actions;
mod browser_lists;
mod audio_loading;
mod missing_samples;
mod waveform_rendering;

pub(super) use browser_search::BrowserSearchCache;
pub(super) use waveform_rendering::WaveformRenderMeta;

/// Upper bound for waveform texture width to stay within GPU limits.
pub(super) const MAX_TEXTURE_WIDTH: u32 = 16_384;

impl EguiController {
    /// Reset all waveform and playback visuals.
    pub(super) fn clear_waveform_view(&mut self) {
        self.ui.waveform.image = None;
        self.ui.waveform.notice = None;
        self.ui.waveform.loading = None;
        self.decoded_waveform = None;
        self.ui.waveform.playhead = PlayheadState::default();
        self.ui.waveform.last_start_marker = None;
        self.ui.waveform.cursor = None;
        self.ui.waveform.selection = None;
        self.ui.waveform.selection_duration = None;
        self.ui.waveform.view = WaveformView::default();
        self.selection.clear();
        self.loaded_audio = None;
        self.loaded_wav = None;
        self.ui.loaded_wav = None;
        self.waveform_render_meta = None;
        if let Some(player) = self.player.as_ref() {
            player.borrow_mut().stop();
        }
        self.jobs.pending_audio = None;
        self.jobs.pending_playback = None;
    }

    /// Expose wav indices for a given triage flag column (used by virtualized rendering).
    pub fn browser_indices(&self, column: TriageFlagColumn) -> &[usize] {
        match column {
            TriageFlagColumn::Trash => &self.ui.browser.trash,
            TriageFlagColumn::Neutral => &self.ui.browser.neutral,
            TriageFlagColumn::Keep => &self.ui.browser.keep,
        }
    }

    /// Visible wav indices after applying the active sample browser filter.
    pub fn visible_browser_indices(&self) -> &[usize] {
        &self.ui.browser.visible
    }

    // Audio load queueing/polling moved to `audio_loading` submodule.

    /// Select a wav row based on its path.
    pub fn select_wav_by_path(&mut self, path: &Path) {
        self.select_wav_by_path_with_rebuild(path, true);
    }

    /// Select a wav row based on its path, optionally delaying the browser rebuild.
    pub fn select_wav_by_path_with_rebuild(&mut self, path: &Path, rebuild: bool) {
        if !self.wav_lookup.contains_key(path) {
            return;
        }
        // Selecting a browser wav should always clear any active collection selection so the
        // waveform view follows the browser selection.
        self.ui.collections.selected_sample = None;
        if self.current_source().is_none() {
            if let Some(source_id) = self
                .last_selected_browsable_source
                .clone()
                .filter(|id| self.sources.iter().any(|s| &s.id == id))
            {
                self.selected_source = Some(source_id);
                self.refresh_sources_ui();
            } else if let Some(first) = self.sources.first().cloned() {
                self.last_selected_browsable_source = Some(first.id.clone());
                self.selected_source = Some(first.id);
                self.refresh_sources_ui();
            }
        }
        let path_changed = self.selected_wav.as_deref() != Some(path);
        if path_changed {
            self.ui.waveform.last_start_marker = None;
        }
        self.selected_wav = Some(path.to_path_buf());
        let missing = self
            .wav_lookup
            .get(path)
            .and_then(|index| self.wav_entries.get(*index))
            .map(|entry| entry.missing)
            .unwrap_or(false);
        if missing {
            self.show_missing_waveform_notice(path);
            self.set_status(
                format!("File missing: {}", path.display()),
                StatusTone::Warning,
            );
            self.suppress_autoplay_once = false;
            if rebuild {
                self.rebuild_browser_lists();
            }
            return;
        }
        if let Some(source) = self.current_source() {
            let autoplay = self.feature_flags.autoplay_selection && !self.suppress_autoplay_once;
            self.suppress_autoplay_once = false;
            let pending_playback = if autoplay {
                Some(PendingPlayback {
                    source_id: source.id.clone(),
                    relative_path: path.to_path_buf(),
                    looped: self.ui.waveform.loop_enabled,
                    start_override: None,
                })
            } else {
                None
            };
            if let Err(err) = self.queue_audio_load_for(
                &source,
                path,
                AudioLoadIntent::Selection,
                pending_playback,
            ) {
                self.set_status(err, StatusTone::Error);
            }
        } else {
            self.suppress_autoplay_once = false;
        }
        if rebuild {
            self.rebuild_browser_lists();
        }
    }

    /// Map the current browser filter into a drop target tag for drag-and-drop retagging.
    pub fn triage_flag_drop_target(&self) -> TriageFlagColumn {
        match self.ui.browser.filter {
            TriageFlagFilter::All | TriageFlagFilter::Untagged => TriageFlagColumn::Neutral,
            TriageFlagFilter::Keep => TriageFlagColumn::Keep,
            TriageFlagFilter::Trash => TriageFlagColumn::Trash,
        }
    }

    /// Current tag of the selected wav, if any.
    pub fn selected_tag(&self) -> Option<SampleTag> {
        self.selected_row_index()
            .and_then(|idx| self.wav_entries.get(idx))
            .map(|entry| entry.tag)
    }

    /// Apply a new browser filter and refresh visible rows.
    pub fn set_browser_filter(&mut self, filter: TriageFlagFilter) {
        if self.ui.browser.filter != filter {
            self.ui.browser.filter = filter;
            self.rebuild_browser_lists();
        }
    }

    /// Request focus for the browser search input while keeping the browser context active.
    pub(crate) fn focus_browser_search(&mut self) {
        self.ui.browser.search_focus_requested = true;
        self.focus_browser_context();
    }

    /// Apply a fuzzy search query to the browser and refresh visible rows.
    pub fn set_browser_search(&mut self, query: impl Into<String>) {
        let query = query.into();
        if self.ui.browser.search_query == query {
            return;
        }
        self.ui.browser.search_query = query;
        self.rebuild_browser_lists();
    }

    /// Select a wav by absolute index into the full wav list.
    pub fn select_wav_by_index(&mut self, index: usize) {
        let path = match self.wav_entries.get(index) {
            Some(entry) => entry.relative_path.clone(),
            None => return,
        };
        self.select_wav_by_path(&path);
    }

    /// Select a wav coming from the sample browser and clear collection focus.
    pub fn select_from_browser(&mut self, path: &Path) {
        self.ui.collections.selected_sample = None;
        self.focus_browser_context();
        self.select_wav_by_path(path);
    }

    /// Retrieve a wav entry by absolute index.
    pub fn wav_entry(&self, index: usize) -> Option<&WavEntry> {
        self.wav_entries.get(index)
    }

    /// Retrieve a cached label for a wav entry by index.
    pub fn wav_label(&mut self, index: usize) -> Option<String> {
        self.label_for_ref(index).map(str::to_string)
    }

    pub(super) fn rebuild_wav_lookup(&mut self) {
        self.wav_lookup.clear();
        for (index, entry) in self.wav_entries.iter().enumerate() {
            self.wav_lookup.insert(entry.relative_path.clone(), index);
        }
    }

    pub(in crate::egui_app::controller) fn sync_browser_after_wav_entries_mutation(
        &mut self,
        source_id: &SourceId,
    ) {
        self.rebuild_wav_lookup();
        self.browser_search_cache.invalidate();
        self.rebuild_browser_lists();
        self.label_cache
            .insert(source_id.clone(), self.build_label_cache(&self.wav_entries));
    }

    pub(in crate::egui_app::controller) fn sync_browser_after_wav_entries_mutation_keep_search_cache(
        &mut self,
        source_id: &SourceId,
    ) {
        self.rebuild_wav_lookup();
        self.rebuild_browser_lists();
        self.label_cache
            .insert(source_id.clone(), self.build_label_cache(&self.wav_entries));
    }

    pub(in crate::egui_app::controller) fn invalidate_cached_audio_for_entry_updates(
        &mut self,
        source_id: &SourceId,
        updates: &[(WavEntry, WavEntry)],
    ) {
        for (old_entry, new_entry) in updates {
            self.invalidate_cached_audio(source_id, &old_entry.relative_path);
            self.invalidate_cached_audio(source_id, &new_entry.relative_path);
        }
    }

    pub(super) fn ensure_wav_cache_lookup(&mut self, source_id: &SourceId) {
        if self.wav_cache_lookup.contains_key(source_id) {
            return;
        }
        let Some(entries) = self.wav_cache.get(source_id) else {
            return;
        };
        let lookup = entries
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.relative_path.clone(), index))
            .collect();
        self.wav_cache_lookup.insert(source_id.clone(), lookup);
    }

    pub(super) fn rebuild_wav_cache_lookup(&mut self, source_id: &SourceId) {
        self.wav_cache_lookup.remove(source_id);
        self.ensure_wav_cache_lookup(source_id);
    }

    pub(super) fn set_sample_tag(
        &mut self,
        path: &Path,
        column: TriageFlagColumn,
    ) -> Result<(), String> {
        let target_tag = match column {
            TriageFlagColumn::Trash => SampleTag::Trash,
            TriageFlagColumn::Neutral => SampleTag::Neutral,
            TriageFlagColumn::Keep => SampleTag::Keep,
        };
        self.set_sample_tag_value(path, target_tag)
    }

    pub(super) fn set_sample_tag_value(
        &mut self,
        path: &Path,
        target_tag: SampleTag,
    ) -> Result<(), String> {
        let Some(source) = self.current_source() else {
            return Err("Select a source first".into());
        };
        self.set_sample_tag_for_source(&source, path, target_tag, true)
    }

    pub(super) fn set_sample_tag_for_source(
        &mut self,
        source: &SampleSource,
        path: &Path,
        target_tag: SampleTag,
        require_present: bool,
    ) -> Result<(), String> {
        let db = self.database_for(source).map_err(|err| err.to_string())?;
        let mut tagging = tagging_service::TaggingService::new(
            self.selected_source.as_ref(),
            &mut self.wav_entries,
            &self.wav_lookup,
            &mut self.wav_cache,
            &mut self.wav_cache_lookup,
        );
        tagging.apply_sample_tag(source, path, target_tag, require_present)?;
        let _ = db.set_tag(path, target_tag);
        if self.selected_source.as_ref() == Some(&source.id) {
            self.rebuild_browser_lists();
        }
        Ok(())
    }

    pub(super) fn load_waveform_for_selection(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Result<(), String> {
        if self.selected_wav.as_deref() != Some(relative_path) {
            self.selected_wav = Some(relative_path.to_path_buf());
        }
        if self.loaded_wav.as_deref() == Some(relative_path) {
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
        let decoded = self.renderer.decode_from_bytes(&bytes)?;
        let duration_seconds = decoded.duration_seconds;
        let sample_rate = decoded.sample_rate;
        let cache_key = CacheKey::new(&source.id, relative_path);
        self.audio_cache
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
        Ok(())
    }

    pub(super) fn load_collection_waveform(
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

    fn finish_waveform_load(
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
        self.jobs.pending_audio = None;
        match intent {
            AudioLoadIntent::Selection => {
                self.loaded_wav = Some(relative_path.to_path_buf());
                self.ui.loaded_wav = Some(relative_path.to_path_buf());
            }
            AudioLoadIntent::CollectionPreview => {
                self.loaded_wav = None;
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

    fn clear_waveform_selection(&mut self) {
        self.ui.waveform.playhead = PlayheadState::default();
        self.ui.waveform.selection = None;
        self.ui.waveform.selection_duration = None;
        self.selection.clear();
    }

    fn loaded_status_text(relative_path: &Path, duration_seconds: f32, sample_rate: u32) -> String {
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
        self.loaded_audio
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

    // waveform rendering helpers moved to `waveform_rendering` submodule.

    pub(super) fn invalidate_cached_audio(&mut self, source_id: &SourceId, relative_path: &Path) {
        let key = CacheKey::new(source_id, relative_path);
        self.audio_cache.invalidate(&key);
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
        self.loaded_audio = Some(LoadedAudio {
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

    pub(in crate::egui_app::controller) fn clear_loaded_audio_and_waveform_visuals(&mut self) {
        self.loaded_audio = None;
        self.decoded_waveform = None;
        self.ui.waveform.image = None;
        self.ui.waveform.playhead = PlayheadState::default();
        self.ui.waveform.selection = None;
        self.ui.waveform.selection_duration = None;
        self.selection.clear();
    }

    pub(in crate::egui_app::controller) fn reload_waveform_for_selection_if_active(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
    ) {
        self.invalidate_cached_audio(&source.id, relative_path);
        let loaded_matches = self.loaded_audio.as_ref().is_some_and(|audio| {
            audio.source_id == source.id && audio.relative_path == relative_path
        });
        let selected_matches = self.selected_source.as_ref() == Some(&source.id)
            && self.selected_wav.as_deref() == Some(relative_path);
        if selected_matches || loaded_matches {
            self.loaded_wav = None;
            self.ui.loaded_wav = None;
            if let Err(err) = self.load_waveform_for_selection(source, relative_path) {
                self.set_status(err, StatusTone::Warning);
            }
        }
    }
}
