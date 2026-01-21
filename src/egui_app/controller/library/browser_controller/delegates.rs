use super::*;
use crate::egui_app::state::LoopCrossfadeSettings;
use tracing::warn;

impl EguiController {
    /// Apply a keep/trash/neutral tag to a single visible browser row.
    pub fn tag_browser_sample(&mut self, row: usize, tag: crate::sample_sources::Rating) -> Result<(), String> {
        self.browser().tag_browser_sample(row, tag)
    }

    /// Apply a keep/trash/neutral tag to multiple visible browser rows.
    pub fn tag_browser_samples(
        &mut self,
        rows: &[usize],
        tag: crate::sample_sources::Rating,
        primary_visible_row: usize,
    ) -> Result<(), String> {
        self.browser()
            .tag_browser_samples(rows, tag, primary_visible_row)
    }

    /// Apply or clear loop markers for multiple visible browser rows.
    pub fn set_loop_marker_browser_samples(
        &mut self,
        rows: &[usize],
        looped: bool,
        primary_visible_row: usize,
    ) -> Result<(), String> {
        self.browser()
            .set_loop_marker_browser_samples(rows, looped, primary_visible_row)
    }

    /// Update the stored BPM metadata for multiple visible browser rows.
    pub fn set_bpm_browser_samples(
        &mut self,
        rows: &[usize],
        bpm: f32,
        primary_visible_row: usize,
    ) -> Result<(), String> {
        self.browser()
            .set_bpm_browser_samples(rows, bpm, primary_visible_row)
    }

    /// Normalize a single visible browser row in-place (overwrites audio).
    pub fn normalize_browser_sample(&mut self, row: usize) -> Result<(), String> {
        self.browser().normalize_browser_sample(row)
    }

    /// Normalize multiple visible browser rows in-place (overwrites audio).
    pub fn normalize_browser_samples(&mut self, rows: &[usize]) -> Result<(), String> {
        self.browser().normalize_browser_samples(rows)
    }

    /// Create loop-crossfaded copies of browser rows and select the primary result.
    pub fn loop_crossfade_browser_samples(
        &mut self,
        rows: &[usize],
        settings: LoopCrossfadeSettings,
        primary_visible_row: usize,
    ) -> Result<(), String> {
        self.browser()
            .loop_crossfade_browser_samples(rows, settings, primary_visible_row)
    }

    /// Rename a single visible browser row on disk and refresh dependent state.
    pub fn rename_browser_sample(&mut self, row: usize, new_name: &str) -> Result<(), String> {
        self.browser().rename_browser_sample(row, new_name)
    }

    /// Delete the file for a single visible browser row and prune references.
    pub fn delete_browser_sample(&mut self, row: usize) -> Result<(), String> {
        self.browser().delete_browser_sample(row)
    }

    /// Delete files for multiple visible browser rows and prune references.
    pub fn delete_browser_samples(&mut self, rows: &[usize]) -> Result<(), String> {
        self.browser().delete_browser_samples(rows)
    }

    /// Remove dead-link browser rows (missing samples) from the library without deleting files.
    pub fn remove_dead_link_browser_sample(&mut self, row: usize) -> Result<(), String> {
        self.browser().remove_dead_link_browser_samples(&[row])
    }

    /// Remove dead-link browser rows (missing samples) from the library without deleting files.
    pub fn remove_dead_link_browser_samples(&mut self, rows: &[usize]) -> Result<(), String> {
        self.browser().remove_dead_link_browser_samples(rows)
    }

    pub(crate) fn resolve_browser_sample(
        &mut self,
        row: usize,
    ) -> Result<helpers::TriageSampleContext, String> {
        let source = if let Some(source) = self.current_source() {
            source
        } else {
            let fallback = self
                .selection_state
                .ctx
                .last_selected_browsable_source
                .as_ref()
                .and_then(|id| self.library.sources.iter().find(|s| &s.id == id))
                .cloned();
            fallback.ok_or_else(|| {
                warn!(row, "triage tag: no current source and no fallback");
                "Select a source first".to_string()
            })?
        };
        let index = self.visible_browser_index(row).ok_or_else(|| {
            warn!(row, "triage tag: visible row missing");
            "Sample not found".to_string()
        })?;
        let entry = self.wav_entry(index).cloned().ok_or_else(|| {
            warn!(row, index, "triage tag: wav entry missing");
            "Sample not found".to_string()
        })?;
        let absolute_path = source.root.join(&entry.relative_path);
        Ok(helpers::TriageSampleContext {
            source,
            entry,
            absolute_path,
        })
    }

    pub(crate) fn prune_cached_sample(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
    ) {
        if let Some(cache) = self.cache.wav.entries.get_mut(&source.id) {
            cache.clear();
        }
        if self.selection_state.ctx.selected_source.as_ref() == Some(&source.id) {
            self.wav_entries.clear();
            self.sync_after_wav_entries_changed();
            self.queue_wav_load();
        } else {
            self.ui_cache.browser.labels.remove(&source.id);
        }
        self.rebuild_missing_lookup_for_source(&source.id);
        self.clear_loaded_sample_if(source, relative_path);
    }

    pub(crate) fn clear_loaded_sample_if(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
    ) {
        self.invalidate_cached_audio(&source.id, relative_path);
        if self.selection_state.ctx.selected_source.as_ref() == Some(&source.id) {
            if self.sample_view.wav.selected_wav.as_deref() == Some(relative_path) {
                self.sample_view.wav.selected_wav = None;
                self.clear_focused_similarity_highlight();
            }
            if self.sample_view.wav.loaded_wav.as_deref() == Some(relative_path) {
                self.sample_view.wav.loaded_wav = None;
            }
            if self.ui.loaded_wav.as_deref() == Some(relative_path) {
                self.ui.loaded_wav = None;
            }
        }
        if let Some(audio) = self.sample_view.wav.loaded_audio.as_ref()
            && audio.source_id == source.id
            && audio.relative_path == relative_path
        {
            self.clear_loaded_audio_and_waveform_visuals();
        }
    }

    pub(crate) fn refresh_waveform_for_sample(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
    ) {
        self.reload_waveform_for_selection_if_active(source, relative_path);
    }

    pub(crate) fn refocus_after_filtered_removal(
        &mut self,
        primary_visible_row: usize,
    ) {
        if matches!(self.ui.browser.filter, TriageFlagFilter::All) {
            return;
        }
        if self.ui.browser.visible.len() == 0 || self.ui.browser.selected_visible.is_some() {
            return;
        }
        if self.random_navigation_mode_enabled() {
            self.focus_random_visible_sample();
            return;
        }
        let target_row = primary_visible_row.min(self.ui.browser.visible.len().saturating_sub(1));
        self.focus_browser_row_only(target_row);
    }
}
