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
mod waveform_loading;

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
        self.waveform.decoded = None;
        self.ui.waveform.playhead = PlayheadState::default();
        self.ui.waveform.last_start_marker = None;
        self.ui.waveform.cursor = None;
        self.ui.waveform.selection = None;
        self.ui.waveform.selection_duration = None;
        self.ui.waveform.view = WaveformView::default();
        self.selection_state.range.clear();
        self.wav_selection.loaded_audio = None;
        self.wav_selection.loaded_wav = None;
        self.ui.loaded_wav = None;
        self.waveform.render_meta = None;
        if let Some(player) = self.audio.player.as_ref() {
            player.borrow_mut().stop();
        }
        self.runtime.jobs.pending_audio = None;
        self.runtime.jobs.pending_playback = None;
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
        if !self.wav_entries.lookup.contains_key(path) {
            return;
        }
        // Selecting a browser wav should always clear any active collection selection so the
        // waveform view follows the browser selection.
        self.ui.collections.selected_sample = None;
        if self.current_source().is_none() {
            if let Some(source_id) = self
                .selection_state.ctx
                .last_selected_browsable_source
                .clone()
                .filter(|id| self.sources.iter().any(|s| &s.id == id))
            {
                self.selection_state.ctx.selected_source = Some(source_id);
                self.refresh_sources_ui();
            } else if let Some(first) = self.sources.first().cloned() {
                self.selection_state.ctx.last_selected_browsable_source = Some(first.id.clone());
                self.selection_state.ctx.selected_source = Some(first.id);
                self.refresh_sources_ui();
            }
        }
        let path_changed = self.wav_selection.selected_wav.as_deref() != Some(path);
        if path_changed {
            self.ui.waveform.last_start_marker = None;
        }
        self.wav_selection.selected_wav = Some(path.to_path_buf());
        let missing = self
            .wav_entries
            .lookup
            .get(path)
            .and_then(|index| self.wav_entries.entries.get(*index))
            .map(|entry| entry.missing)
            .unwrap_or(false);
        if missing {
            self.show_missing_waveform_notice(path);
            self.set_status(
                format!("File missing: {}", path.display()),
                StatusTone::Warning,
            );
            self.selection_state.suppress_autoplay_once = false;
            if rebuild {
                self.rebuild_browser_lists();
            }
            return;
        }
        if let Some(source) = self.current_source() {
            let autoplay = self.settings.feature_flags.autoplay_selection && !self.selection_state.suppress_autoplay_once;
            self.selection_state.suppress_autoplay_once = false;
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
            self.selection_state.suppress_autoplay_once = false;
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
            .and_then(|idx| self.wav_entries.entries.get(idx))
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
        let path = match self.wav_entries.entries.get(index) {
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
        self.wav_entries.entries.get(index)
    }

    /// Retrieve a cached label for a wav entry by index.
    pub fn wav_label(&mut self, index: usize) -> Option<String> {
        self.label_for_ref(index).map(str::to_string)
    }

    pub(super) fn rebuild_wav_lookup(&mut self) {
        self.wav_entries.lookup.clear();
        for (index, entry) in self.wav_entries.entries.iter().enumerate() {
            self.wav_entries.lookup.insert(entry.relative_path.clone(), index);
        }
    }

    pub(in crate::egui_app::controller) fn sync_browser_after_wav_entries_mutation(
        &mut self,
        source_id: &SourceId,
    ) {
        self.rebuild_wav_lookup();
        self.browser_cache.search.invalidate();
        self.rebuild_browser_lists();
        self.browser_cache
            .labels
            .insert(source_id.clone(), self.build_label_cache(&self.wav_entries.entries));
    }

    pub(in crate::egui_app::controller) fn sync_browser_after_wav_entries_mutation_keep_search_cache(
        &mut self,
        source_id: &SourceId,
    ) {
        self.rebuild_wav_lookup();
        self.rebuild_browser_lists();
        self.browser_cache
            .labels
            .insert(source_id.clone(), self.build_label_cache(&self.wav_entries.entries));
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
        if self.cache.wav.lookup.contains_key(source_id) {
            return;
        }
        let Some(entries) = self.cache.wav.entries.get(source_id) else {
            return;
        };
        let lookup = entries
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.relative_path.clone(), index))
            .collect();
        self.cache.wav.lookup.insert(source_id.clone(), lookup);
    }

    pub(super) fn rebuild_wav_cache_lookup(&mut self, source_id: &SourceId) {
        self.cache.wav.lookup.remove(source_id);
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
            self.selection_state.ctx.selected_source.as_ref(),
            &mut self.wav_entries.entries,
            &self.wav_entries.lookup,
            &mut self.cache.wav.entries,
            &mut self.cache.wav.lookup,
        );
        tagging.apply_sample_tag(source, path, target_tag, require_present)?;
        let _ = db.set_tag(path, target_tag);
        if self.selection_state.ctx.selected_source.as_ref() == Some(&source.id) {
            self.rebuild_browser_lists();
        }
        Ok(())
    }

    // waveform loading helpers moved to `waveform_loading` submodule.
}
