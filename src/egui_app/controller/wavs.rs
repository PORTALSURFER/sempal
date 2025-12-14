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
mod selection_ops;
mod waveform_view;
mod waveform_rendering;
mod waveform_loading;

pub(super) use browser_search::BrowserSearchCache;
pub(super) use waveform_rendering::WaveformRenderMeta;

/// Upper bound for waveform texture width to stay within GPU limits.
pub(super) const MAX_TEXTURE_WIDTH: u32 = 16_384;

impl EguiController {
    /// Reset all waveform and playback visuals.
    pub(super) fn clear_waveform_view(&mut self) {
        waveform_view::clear_waveform_view(self);
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
        selection_ops::select_wav_by_path(self, path);
    }

    /// Select a wav row based on its path, optionally delaying the browser rebuild.
    pub fn select_wav_by_path_with_rebuild(&mut self, path: &Path, rebuild: bool) {
        selection_ops::select_wav_by_path_with_rebuild(self, path, rebuild);
    }

    /// Map the current browser filter into a drop target tag for drag-and-drop retagging.
    pub fn triage_flag_drop_target(&self) -> TriageFlagColumn {
        selection_ops::triage_flag_drop_target(self)
    }

    /// Current tag of the selected wav, if any.
    pub fn selected_tag(&self) -> Option<SampleTag> {
        selection_ops::selected_tag(self)
    }

    /// Apply a new browser filter and refresh visible rows.
    pub fn set_browser_filter(&mut self, filter: TriageFlagFilter) {
        browser_search::set_browser_filter(self, filter);
    }

    /// Request focus for the browser search input while keeping the browser context active.
    pub(crate) fn focus_browser_search(&mut self) {
        browser_search::focus_browser_search(self);
    }

    /// Apply a fuzzy search query to the browser and refresh visible rows.
    pub fn set_browser_search(&mut self, query: impl Into<String>) {
        browser_search::set_browser_search(self, query);
    }

    /// Select a wav by absolute index into the full wav list.
    pub fn select_wav_by_index(&mut self, index: usize) {
        selection_ops::select_wav_by_index(self, index);
    }

    /// Select a wav coming from the sample browser and clear collection focus.
    pub fn select_from_browser(&mut self, path: &Path) {
        selection_ops::select_from_browser(self, path);
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
        selection_ops::rebuild_wav_lookup(self);
    }

    pub(in crate::egui_app::controller) fn sync_browser_after_wav_entries_mutation(
        &mut self,
        source_id: &SourceId,
    ) {
        selection_ops::sync_browser_after_wav_entries_mutation(self, source_id);
    }

    pub(in crate::egui_app::controller) fn sync_browser_after_wav_entries_mutation_keep_search_cache(
        &mut self,
        source_id: &SourceId,
    ) {
        selection_ops::sync_browser_after_wav_entries_mutation_keep_search_cache(self, source_id);
    }

    pub(in crate::egui_app::controller) fn invalidate_cached_audio_for_entry_updates(
        &mut self,
        source_id: &SourceId,
        updates: &[(WavEntry, WavEntry)],
    ) {
        selection_ops::invalidate_cached_audio_for_entry_updates(self, source_id, updates);
    }

    pub(super) fn ensure_wav_cache_lookup(&mut self, source_id: &SourceId) {
        selection_ops::ensure_wav_cache_lookup(self, source_id);
    }

    pub(super) fn rebuild_wav_cache_lookup(&mut self, source_id: &SourceId) {
        selection_ops::rebuild_wav_cache_lookup(self, source_id);
    }

    pub(super) fn set_sample_tag(
        &mut self,
        path: &Path,
        column: TriageFlagColumn,
    ) -> Result<(), String> {
        selection_ops::set_sample_tag(self, path, column)
    }

    #[allow(dead_code)]
    pub(super) fn set_sample_tag_value(
        &mut self,
        path: &Path,
        target_tag: SampleTag,
    ) -> Result<(), String> {
        selection_ops::set_sample_tag_value(self, path, target_tag)
    }

    pub(super) fn set_sample_tag_for_source(
        &mut self,
        source: &SampleSource,
        path: &Path,
        target_tag: SampleTag,
        require_present: bool,
    ) -> Result<(), String> {
        selection_ops::set_sample_tag_for_source(self, source, path, target_tag, require_present)
    }

    // waveform loading helpers moved to `waveform_loading` submodule.
}
