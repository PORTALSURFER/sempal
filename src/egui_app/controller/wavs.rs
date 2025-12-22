use super::audio_cache::CacheKey;
use super::*;
use crate::egui_app::state::{FocusContext, WaveformView};
use crate::egui_app::view_model;
use crate::waveform::DecodedWaveform;
use std::path::{Path, PathBuf};

mod audio_loading;
mod browser_actions;
mod browser_lists;
mod browser_search;
mod feature_cache;
mod missing_samples;
mod prediction_filter;
mod selection_ops;
mod similar;
mod waveform_loading;
mod waveform_rendering;
mod waveform_view;

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

    /// Apply a predicted category filter and refresh visible rows.
    pub fn set_category_filter(&mut self, category: Option<String>) {
        prediction_filter::set_category_filter(self, category);
    }

    /// Apply a minimum confidence threshold for predicted category filtering and refresh visible rows.
    pub fn set_confidence_threshold(&mut self, threshold: f32) {
        prediction_filter::set_confidence_threshold(self, threshold);
    }

    /// Toggle whether to include `UNKNOWN` predicted samples in the list.
    pub fn set_include_unknowns(&mut self, include: bool) {
        prediction_filter::set_include_unknowns(self, include);
    }

    /// Toggle low-confidence review mode, filtering visible rows to questionable predictions.
    pub fn set_review_mode(&mut self, enabled: bool) {
        prediction_filter::set_review_mode(self, enabled);
    }

    /// Set the max confidence threshold used by low-confidence review mode.
    pub fn set_review_max_confidence(&mut self, value: f32) {
        prediction_filter::set_review_max_confidence(self, value);
    }

    /// Toggle whether review mode includes samples without predictions.
    pub fn set_review_include_unpredicted(&mut self, include: bool) {
        prediction_filter::set_review_include_unpredicted(self, include);
    }

    /// Filter the browser to show similar samples for the chosen visible row.
    pub fn find_similar_for_visible_row(&mut self, row: usize) -> Result<(), String> {
        similar::find_similar_for_visible_row(self, row)
    }

    /// Filter the browser to show similar samples for an external audio clip.
    pub fn find_similar_for_audio_path(&mut self, path: &Path) -> Result<(), String> {
        similar::find_similar_for_audio_path(self, path)
    }

    /// Clear any active similar-sounds filter.
    pub fn clear_similar_filter(&mut self) {
        similar::clear_similar_filter(self);
    }

    /// Build a library sample_id for the visible browser row.
    pub fn sample_id_for_visible_row(&self, row: usize) -> Result<String, String> {
        let source_id = self
            .selection_state
            .ctx
            .selected_source
            .clone()
            .ok_or_else(|| "No active source selected".to_string())?;
        let entry_index = self
            .ui
            .browser
            .visible
            .get(row)
            .copied()
            .ok_or_else(|| "Selected row is out of range".to_string())?;
        let entry = self
            .wav_entries
            .entries
            .get(entry_index)
            .ok_or_else(|| "Sample entry missing".to_string())?;
        Ok(super::analysis_jobs::build_sample_id(
            source_id.as_str(),
            &entry.relative_path,
        ))
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

    pub fn analysis_failure_for_entry(&self, index: usize) -> Option<&str> {
        let source_id = self.selection_state.ctx.selected_source.as_ref()?;
        let entry = self.wav_entries.entries.get(index)?;
        self.ui_cache
            .browser
            .analysis_failures
            .get(source_id)
            .and_then(|failures| failures.get(&entry.relative_path))
            .map(|s| s.as_str())
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
