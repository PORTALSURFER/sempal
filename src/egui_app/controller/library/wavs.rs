use crate::egui_app::controller::playback::audio_cache::CacheKey;
use crate::egui_app::controller::library::wav_io;
use super::*;
use crate::egui_app::view_model;
use std::collections::HashMap;
use crate::waveform::DecodedWaveform;
use std::path::{Path, PathBuf};

mod audio_loading;
mod browser_actions;
mod browser_history;
mod browser_lists;
mod browser_search;
pub(crate) mod browser_search_worker;
mod feature_cache;
mod selection_ops;
mod similar;
mod waveform_loading;
pub mod waveform_rendering;

mod waveform_view;

pub(crate) use browser_search::BrowserSearchCache;
pub(crate) use waveform_rendering::WaveformRenderMeta;

/// Upper bound for waveform texture width to stay within GPU limits.
pub(crate) const MAX_TEXTURE_WIDTH: u32 = 16_384;

impl EguiController {
    /// Reset all waveform and playback visuals.
    pub(crate) fn clear_waveform_view(&mut self) {
        waveform_view::clear_waveform_view(self);
    }

    /// Clear near-duplicate highlights for the focused sample.
    pub(crate) fn clear_focused_similarity_highlight(&mut self) {
        self.ui.browser.focused_similarity = None;
    }

    /// Refresh near-duplicate highlights for the focused sample.
    pub(crate) fn refresh_focused_similarity_highlight(
        &mut self,
        sample_id: &str,
        anchor_index: Option<usize>,
    ) {
        self.ui.browser.focused_similarity = match similar::build_focused_similarity_highlight(
            self,
            sample_id,
            anchor_index,
        ) {
            Ok(highlight) => highlight,
            Err(_) => None,
        };
    }

    /// Expose wav indices for a given triage flag column (used by virtualized rendering).
    pub fn browser_indices(&self, column: TriageFlagColumn) -> &[usize] {
        match column {
            TriageFlagColumn::Trash => &self.ui.browser.trash,
            TriageFlagColumn::Neutral => &self.ui.browser.neutral,
            TriageFlagColumn::Keep => &self.ui.browser.keep,
        }
    }

    /// Resolve the stored BPM metadata for a sample path when available.
    pub(crate) fn bpm_value_for_path(&mut self, path: &Path) -> Option<f32> {
        let source = self.current_source()?;
        if let Some(cache) = self.ui_cache.browser.bpm_values.get(&source.id) {
            if let Some(cached) = cache.get(path) {
                return *cached;
            }
        }
        let db = self.database_for(&source).ok()?;
        let sample_id = analysis_jobs::build_sample_id(source.id.as_str(), path);
        let bpm = db.bpm_for_sample_id(&sample_id).ok().flatten();
        let cache = self
            .ui_cache
            .browser
            .bpm_values
            .entry(source.id.clone())
            .or_insert_with(HashMap::new);
        cache.insert(path.to_path_buf(), bpm);
        bpm
    }

    /// Visible wav indices after applying the active sample browser filter.
    pub fn visible_browser_rows(&self) -> &crate::egui_app::state::VisibleRows {
        &self.ui.browser.visible
    }

    /// Visible row count after applying the active sample browser filter.
    pub fn visible_browser_len(&self) -> usize {
        self.ui.browser.visible.len()
    }

    /// Map a visible row to the absolute wav index.
    pub fn visible_browser_index(&self, row: usize) -> Option<usize> {
        self.ui.browser.visible.get(row)
    }

    pub(crate) fn wav_entries_len(&self) -> usize {
        self.wav_entries.total
    }

    pub(crate) fn ensure_wav_page_loaded(&mut self, index: usize) -> Result<(), String> {
        if self.wav_entries.entry(index).is_some() {
            return Ok(());
        }
        let Some(source) = self.current_source() else {
            return Err("No active source selected".to_string());
        };
        let page_size = self.wav_entries.page_size.max(1);
        let page_index = index / page_size;
        let offset = page_index * page_size;
        let db = self.database_for(&source).map_err(|err| err.to_string())?;
        let entries = db
            .list_files_page(page_size, offset)
            .map_err(|err| err.to_string())?;
        self.wav_entries.insert_page(page_index, entries);
        Ok(())
    }

    pub(crate) fn wav_index_for_path(&mut self, path: &Path) -> Option<usize> {
        let normalized = path.to_string_lossy().replace('\\', "/");
        if let Some(index) = self.wav_entries.lookup.get(Path::new(&normalized)).copied() {
            return Some(index);
        }
        let source = self.current_source()?;
        let db = self.database_for(&source).ok()?;
        let index = db.index_for_path(path).ok().flatten()?;
        self.wav_entries.insert_lookup(path.to_path_buf(), index);
        Some(index)
    }

    /// Normalize a wav file and return updated metadata + tag.
    pub(crate) fn normalize_and_save_for_path(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
        absolute_path: &Path,
    ) -> Result<(u64, i64, crate::sample_sources::Rating), String> {
        let (mut samples, spec) = wav_io::read_samples_for_normalization(absolute_path)?;
        if samples.is_empty() {
            return Err("No audio data to normalize".into());
        }
        // Use optimized SIMD/parallel normalization in-place.
        crate::analysis::audio::normalize_peak_in_place(&mut samples);

        let target_spec = hound::WavSpec {
            channels: spec.channels.max(1),
            sample_rate: spec.sample_rate.max(1),
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        wav_io::write_normalized_wav(absolute_path, &samples, target_spec)?;

        let (file_size, modified_ns) = wav_io::file_metadata(absolute_path)?;
        let tag = self.sample_tag_for(source, relative_path)?;
        Ok((file_size, modified_ns, tag))
    }

    /// Resolve the tag for a wav entry, falling back to the database.
    pub(crate) fn sample_tag_for(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Result<crate::sample_sources::Rating, String> {
        if let Some(cache) = self.cache.wav.entries.get(&source.id) {
            if let Some(index) = cache.lookup.get(relative_path).copied()
                && let Some(entry) = cache.entry(index)
            {
                return Ok(entry.tag);
            }
        }
        if self.selection_state.ctx.selected_source.as_ref() == Some(&source.id)
            && let Some(index) = self.wav_index_for_path(relative_path)
            && let Some(entry) = self.wav_entries.entry(index)
        {
            return Ok(entry.tag);
        }
        let db = self
            .database_for(source)
            .map_err(|err| format!("Database unavailable: {err}"))?;
        db.tag_for_path(relative_path)
            .map_err(|err| format!("Failed to read database: {err}"))?
            .ok_or_else(|| "Sample not found in database".to_string())
    }

    /// Resolve the loop marker state for a wav entry.
    pub(crate) fn sample_looped_for(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Result<bool, String> {
        if let Some(cache) = self.cache.wav.entries.get(&source.id) {
            if let Some(index) = cache.lookup.get(relative_path).copied()
                && let Some(entry) = cache.entry(index)
            {
                return Ok(entry.looped);
            }
        }
        if self.selection_state.ctx.selected_source.as_ref() == Some(&source.id)
            && let Some(index) = self.wav_index_for_path(relative_path)
            && let Some(entry) = self.wav_entries.entry(index)
        {
            return Ok(entry.looped);
        }
        let db = self
            .database_for(source)
            .map_err(|err| format!("Database unavailable: {err}"))?;
        db.looped_for_path(relative_path)
            .map_err(|err| format!("Failed to read database: {err}"))?
            .ok_or_else(|| "Sample not found in database".to_string())
    }

    /// Resolve the last played timestamp for a wav entry, if available.
    pub(crate) fn sample_last_played_for(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Result<Option<i64>, String> {
        if let Some(cache) = self.cache.wav.entries.get(&source.id) {
            if let Some(index) = cache.lookup.get(relative_path).copied()
                && let Some(entry) = cache.entry(index)
            {
                return Ok(entry.last_played_at);
            }
        }
        if self.selection_state.ctx.selected_source.as_ref() == Some(&source.id)
            && let Some(index) = self.wav_index_for_path(relative_path)
            && let Some(entry) = self.wav_entries.entry(index)
        {
            return Ok(entry.last_played_at);
        }
        let db = self
            .database_for(source)
            .map_err(|err| format!("Database unavailable: {err}"))?;
        db.last_played_at_for_path(relative_path)
            .map_err(|err| format!("Failed to read database: {err}"))
    }

    /// Persist a rename or path change in the per-source database.
    pub(crate) fn rewrite_db_entry_for_source(
        &mut self,
        source: &SampleSource,
        old_relative: &Path,
        new_relative: &Path,
        file_size: u64,
        modified_ns: i64,
        tag: crate::sample_sources::Rating,
    ) -> Result<(), String> {
        let db = self
            .database_for(source)
            .map_err(|err| format!("Database unavailable: {err}"))?;
        let last_played_at = db
            .last_played_at_for_path(old_relative)
            .map_err(|err| format!("Failed to load playback age: {err}"))?;
        let looped = db
            .looped_for_path(old_relative)
            .map_err(|err| format!("Failed to load loop marker: {err}"))?
            .unwrap_or(false);
        let mut batch = db
            .write_batch()
            .map_err(|err| format!("Failed to start database update: {err}"))?;
        batch
            .remove_file(old_relative)
            .map_err(|err| format!("Failed to drop old entry: {err}"))?;
        batch
            .upsert_file(new_relative, file_size, modified_ns)
            .map_err(|err| format!("Failed to register renamed file: {err}"))?;
        batch
            .set_tag(new_relative, tag)
            .map_err(|err| format!("Failed to copy tag: {err}"))?;
        batch
            .set_looped(new_relative, looped)
            .map_err(|err| format!("Failed to copy loop marker: {err}"))?;
        if let Some(last_played_at) = last_played_at {
            batch
                .set_last_played_at(new_relative, last_played_at)
                .map_err(|err| format!("Failed to copy playback age: {err}"))?;
        }
        batch
            .commit()
            .map_err(|err| format!("Failed to save rename: {err}"))
    }

    /// Upsert file metadata into the source database.
    pub(crate) fn upsert_metadata_for_source(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
        file_size: u64,
        modified_ns: i64,
    ) -> Result<(), String> {
        let db = self
            .database_for(source)
            .map_err(|err| format!("Database unavailable: {err}"))?;
        db.upsert_file(relative_path, file_size, modified_ns)
            .map_err(|err| format!("Failed to refresh metadata: {err}"))
    }

    /// Validate and sanitize a renamed file while preserving its extension.
    pub(crate) fn name_with_preserved_extension(
        &self,
        current_relative: &Path,
        new_name: &str,
    ) -> Result<String, String> {
        let trimmed = new_name.trim();
        if trimmed.is_empty() {
            return Err("Name cannot be empty".into());
        }
        let Some(ext) = current_relative.extension().and_then(|ext| ext.to_str()) else {
            return Ok(trimmed.to_string());
        };
        let ext_lower = ext.to_ascii_lowercase();
        let should_strip_suffix = |suffix: &str| -> bool {
            let suffix_lower = suffix.to_ascii_lowercase();
            suffix_lower == ext_lower
                || matches!(
                    suffix_lower.as_str(),
                    "wav" | "wave" | "flac" | "aif" | "aiff" | "mp3" | "ogg" | "opus"
                )
        };
        let stem = if let Some((stem, suffix)) = trimmed.rsplit_once('.') {
            if !stem.is_empty() && should_strip_suffix(suffix) {
                stem
            } else {
                trimmed
            }
        } else {
            trimmed
        };
        let stem = stem.trim_end_matches('.');
        if stem.trim().is_empty() {
            return Err("Name cannot be empty".into());
        }
        Ok(format!("{stem}.{ext}"))
    }

    /// Validate that a new file name is safe and available in its parent folder.
    pub(crate) fn validate_new_sample_name_in_parent(
        &self,
        relative_path: &Path,
        root: &Path,
        new_name: &str,
    ) -> Result<PathBuf, String> {
        let trimmed = new_name.trim();
        if trimmed.is_empty() {
            return Err("Name cannot be empty".into());
        }
        if trimmed.contains(['/', '\\']) {
            return Err("Name cannot contain path separators".into());
        }
        let parent = relative_path.parent().unwrap_or(Path::new(""));
        let new_relative = parent.join(trimmed);
        let new_absolute = root.join(&new_relative);
        if new_absolute.exists() {
            return Err(format!(
                "A file named {} already exists",
                new_relative.display()
            ));
        }
        Ok(new_relative)
    }

    /// Update all cached structures after a file path or metadata change.
    pub(crate) fn update_cached_entry(
        &mut self,
        source: &SampleSource,
        old_path: &Path,
        new_entry: WavEntry,
    ) {
        self.update_selection_paths(source, old_path, &new_entry.relative_path);
        self.invalidate_cached_audio(&source.id, old_path);
        if let Some(missing) = self.library.missing.wavs.get_mut(&source.id) {
            let removed = missing.remove(old_path);
            if removed && new_entry.missing {
                missing.insert(new_entry.relative_path.clone());
            }
        }
        if old_path == new_entry.relative_path {
            let mut updated = false;
            if self.selection_state.ctx.selected_source.as_ref() == Some(&source.id) {
                updated |= self.wav_entries.update_entry(old_path, new_entry.clone());
            }
            if let Some(cache) = self.cache.wav.entries.get_mut(&source.id) {
                updated |= cache.update_entry(old_path, new_entry.clone());
            }
            if updated && self.selection_state.ctx.selected_source.as_ref() == Some(&source.id) {
                self.rebuild_browser_lists();
            }
            return;
        }
        if let Ok(db) = self.database_for(source)
            && matches!(db.index_for_path(old_path), Ok(Some(_)))
        {
            let _ = self.rewrite_db_entry_for_source(
                source,
                old_path,
                &new_entry.relative_path,
                new_entry.file_size,
                new_entry.modified_ns,
                new_entry.tag,
            );
        }
        let mut updated = false;
        if self.selection_state.ctx.selected_source.as_ref() == Some(&source.id) {
            if let Some(index) = self.wav_entries.lookup.get(old_path).copied()
                && let Some(slot) = self.wav_entries.entry_mut(index)
            {
                *slot = new_entry.clone();
                self.wav_entries.lookup.remove(old_path);
                self.wav_entries
                    .insert_lookup(new_entry.relative_path.clone(), index);
                updated = true;
            }
            if self.ui.browser.last_focused_path.as_deref() == Some(old_path) {
                self.ui.browser.last_focused_path = Some(new_entry.relative_path.clone());
            }
        }
        if let Some(cache) = self.cache.wav.entries.get_mut(&source.id) {
            if let Some(index) = cache.lookup.get(old_path).copied()
                && let Some(slot) = cache.entry_mut(index)
            {
                *slot = new_entry.clone();
                cache.lookup.remove(old_path);
                cache.insert_lookup(new_entry.relative_path.clone(), index);
                updated = true;
            }
        }
        if updated {
            if self.selection_state.ctx.selected_source.as_ref() == Some(&source.id) {
                self.ui_cache.browser.search.invalidate();
                self.rebuild_browser_lists();
            }
            if old_path != new_entry.relative_path {
                self.ui_cache.browser.labels.remove(&source.id);
            }
        } else {
            self.invalidate_wav_entries_for_source_preserve_folders(source);
        }
        self.invalidate_cached_audio(&source.id, &new_entry.relative_path);
    }

    /// Invalidate caches after inserting a new entry for a source.
    pub(crate) fn insert_cached_entry(
        &mut self,
        source: &SampleSource,
        entry: WavEntry,
    ) {
        self.invalidate_wav_entries_for_source(source);
        self.invalidate_cached_audio(&source.id, &entry.relative_path);
    }

    /// Rewrite selection paths when a file is renamed or moved.
    pub(crate) fn update_selection_paths(
        &mut self,
        source: &SampleSource,
        old_path: &Path,
        new_path: &Path,
    ) {
        if self.selection_state.ctx.selected_source.as_ref() == Some(&source.id) {
            if !self.ui.browser.selected_paths.is_empty() {
                let mut updated = Vec::with_capacity(self.ui.browser.selected_paths.len());
                let mut replaced = false;
                for path in self.ui.browser.selected_paths.iter() {
                    if path == old_path {
                        replaced = true;
                        if !updated.iter().any(|candidate| candidate == new_path) {
                            updated.push(new_path.to_path_buf());
                        }
                    } else {
                        updated.push(path.clone());
                    }
                }
                if replaced {
                    self.ui.browser.selected_paths = updated;
                }
            }
            if self.sample_view.wav.selected_wav.as_deref() == Some(old_path) {
                self.sample_view.wav.selected_wav = Some(new_path.to_path_buf());
            }
            if self.sample_view.wav.loaded_wav.as_deref() == Some(old_path) {
                self.sample_view.wav.loaded_wav = Some(new_path.to_path_buf());
                self.ui.loaded_wav = Some(new_path.to_path_buf());
            } else if self.ui.loaded_wav.as_deref() == Some(old_path) {
                self.ui.loaded_wav = Some(new_path.to_path_buf());
            }
        }
        if let Some(audio) = self.sample_view.wav.loaded_audio.as_mut()
            && audio.source_id == source.id
            && audio.relative_path == old_path
        {
            audio.relative_path = new_path.to_path_buf();
        }
    }

    pub(crate) fn for_each_wav_entry(
        &mut self,
        mut visit: impl FnMut(usize, &WavEntry),
    ) -> Result<(), String> {
        let Some(source) = self.current_source() else {
            return Ok(());
        };
        let db = self.database_for(&source).map_err(|err| err.to_string())?;
        let total = self.wav_entries.total;
        let page_size = self.wav_entries.page_size.max(1);
        let page_count = (total + page_size - 1) / page_size;
        for page_index in 0..page_count {
            let offset = page_index * page_size;
            if let Some(page) = self.wav_entries.pages.get(&page_index) {
                for (idx, entry) in page.iter().enumerate() {
                    visit(offset + idx, entry);
                }
                continue;
            }
            let entries = db
                .list_files_page(page_size, offset)
                .map_err(|err| err.to_string())?;
            self.wav_entries.insert_page(page_index, entries);
            let page = self
                .wav_entries
                .pages
                .get(&page_index)
                .ok_or_else(|| "Failed to cache wav entries page".to_string())?;
            for (idx, entry) in page.iter().enumerate() {
                visit(offset + idx, entry);
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn set_wav_entries_for_tests(&mut self, entries: Vec<WavEntry>) {
        let entries_for_db = entries.clone();
        self.wav_entries.clear();
        self.wav_entries.total = entries.len();
        self.wav_entries.insert_page(0, entries);
        self.rebuild_wav_lookup();
        if let Some(source) = self.current_source() {
            if let Ok(conn) = crate::sample_sources::SourceDatabase::open_connection(&source.root) {
                let _ = conn.execute("DELETE FROM wav_files", []);
            }
            if let Ok(db) = self.database_for(&source)
                && let Ok(mut batch) = db.write_batch()
            {
                for entry in &entries_for_db {
                    let hash = entry.content_hash.as_deref().unwrap_or("test");
                    let _ = batch.upsert_file_with_hash_and_tag(
                        &entry.relative_path,
                        entry.file_size,
                        entry.modified_ns,
                        hash,
                        entry.tag,
                        entry.missing,
                    );
                    if let Some(last_played_at) = entry.last_played_at {
                        let _ = batch.set_last_played_at(&entry.relative_path, last_played_at);
                    }
                }
                let _ = batch.commit();
            }
        }
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
    pub fn selected_tag(&mut self) -> Option<crate::sample_sources::Rating> {
        selection_ops::selected_tag(self)
    }

    /// Apply a new browser filter and refresh visible rows.
    pub fn set_browser_filter(&mut self, filter: TriageFlagFilter) {
        browser_search::set_browser_filter(self, filter);
    }

    /// Apply a new sample browser sort mode and refresh visible rows.
    pub fn set_browser_sort(&mut self, sort: SampleBrowserSort) {
        browser_search::set_browser_sort(self, sort);
    }

    /// Request focus for the browser search input while keeping the browser context active.
    pub(crate) fn focus_browser_search(&mut self) {
        browser_search::focus_browser_search(self);
    }

    /// Apply a fuzzy search query to the browser and refresh visible rows.
    pub fn set_browser_search(&mut self, query: impl Into<String>) {
        browser_search::set_browser_search(self, query);
    }

    /// Filter the browser to show similar samples for the chosen visible row.
    pub fn find_similar_for_visible_row(&mut self, row: usize) -> Result<(), String> {
        similar::find_similar_for_visible_row(self, row)
    }

    pub(crate) fn refresh_similarity_sort_for_loaded_sample(&mut self) {
        if let Err(err) = similar::refresh_similarity_sort_for_loaded(self) {
            similar::disable_similarity_sort(self);
            self.set_status(err, StatusTone::Warning);
        }
    }

    /// Sort the browser by similarity to the loaded sample.
    pub fn enable_loaded_similarity_sort(&mut self) -> Result<(), String> {
        similar::enable_loaded_similarity_sort(self)
    }

    /// Disable similarity-based sorting and restore list order.
    pub fn disable_similarity_sort(&mut self) {
        similar::disable_similarity_sort(self);
    }

    /// Filter the browser to show near-duplicate samples for the chosen visible row.
    pub fn find_duplicates_for_visible_row(&mut self, row: usize) -> Result<(), String> {
        similar::find_duplicates_for_visible_row(self, row)
    }

    /// Filter the browser to show similar samples for a specific library sample_id.
    pub fn find_similar_for_sample_id(&mut self, sample_id: &str) -> Result<(), String> {
        similar::find_similar_for_sample_id(self, sample_id)
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
    pub fn sample_id_for_visible_row(&mut self, row: usize) -> Result<String, String> {
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
            .ok_or_else(|| "Selected row is out of range".to_string())?;
        let entry = self
            .wav_entry(entry_index)
            .ok_or_else(|| "Sample entry missing".to_string())?;
        Ok(analysis_jobs::build_sample_id(
            source_id.as_str(),
            &entry.relative_path,
        ))
    }

    /// Build a library sample_id for the currently selected wav.
    pub fn selected_sample_id(&self) -> Option<String> {
        let source_id = self.selection_state.ctx.selected_source.as_ref()?;
        let path = self.sample_view.wav.selected_wav.as_ref()?;
        Some(analysis_jobs::build_sample_id(
            source_id.as_str(),
            path,
        ))
    }

    /// Focus the sample browser on a library sample_id without autoplay.
    pub fn focus_sample_from_map(&mut self, sample_id: &str) -> Result<(), String> {
        let (source_id, relative_path) = analysis_jobs::parse_sample_id(sample_id)?;
        let source_id = SourceId::from_string(source_id);
        if self.selection_state.ctx.selected_source.as_ref() != Some(&source_id) {
            self.select_source(Some(source_id.clone()));
        }
        self.focus_browser_context();
        self.ui.browser.autoscroll = true;
        self.ui.browser.selected_paths.clear();
        self.ui.browser.selection_anchor_visible = None;
        self.selection_state.suppress_autoplay_once = true;
        self.select_wav_by_path(&relative_path);
        if let Some(row) = self.visible_row_for_path(&relative_path) {
            self.ui.browser.selection_anchor_visible = Some(row);
        }
        Ok(())
    }

    /// Load waveform/audio for a given library sample_id without requiring browser selection.
    pub fn preview_sample_by_id(&mut self, sample_id: &str) -> Result<(), String> {
        let (source_id, relative_path) = analysis_jobs::parse_sample_id(sample_id)?;
        let source = self
            .library
            .sources
            .iter()
            .find(|source| source.id.as_str() == source_id)
            .map(|source| SampleSource {
                id: source.id.clone(),
                root: source.root.clone(),
            })
            .ok_or_else(|| format!("Unknown source for sample_id: {sample_id}"))?;
        self.load_waveform_for_selection(&source, &relative_path)
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
    pub fn wav_entry(&mut self, index: usize) -> Option<&WavEntry> {
        self.ensure_wav_page_loaded(index).ok()?;
        self.wav_entries.entry(index)
    }

    /// Return the last analysis failure message for a wav entry, if any.
    pub fn analysis_failure_for_entry(&mut self, index: usize) -> Option<&str> {
        let source_id = self.selection_state.ctx.selected_source.clone()?;
        let path = self
            .wav_entry(index)
            .map(|entry| entry.relative_path.clone())?;
        self.ui_cache
            .browser
            .analysis_failures
            .get(&source_id)
            .and_then(|failures| failures.get(&path))
            .map(|s| s.as_str())
    }

    /// Retrieve a cached label for a wav entry by index.
    pub fn wav_label(&mut self, index: usize) -> Option<String> {
        self.label_for_ref(index).map(str::to_string)
    }

    pub(crate) fn rebuild_wav_lookup(&mut self) {
        selection_ops::rebuild_wav_lookup(self);
    }

    pub(crate) fn invalidate_cached_audio_for_entry_updates(
        &mut self,
        source_id: &SourceId,
        updates: &[(WavEntry, WavEntry)],
    ) {
        selection_ops::invalidate_cached_audio_for_entry_updates(self, source_id, updates);
    }

    pub(crate) fn set_sample_tag(
        &mut self,
        path: &Path,
        column: TriageFlagColumn,
    ) -> Result<(), String> {
        selection_ops::set_sample_tag(self, path, column)
    }

    pub(crate) fn set_sample_tag_for_source(
        &mut self,
        source: &SampleSource,
        path: &Path,
        target_tag: crate::sample_sources::Rating,
        require_present: bool,
    ) -> Result<(), String> {
        selection_ops::set_sample_tag_for_source(self, source, path, target_tag, require_present)
    }

    /// Update the loop marker for a sample path within a specific source.
    pub(crate) fn set_sample_looped_for_source(
        &mut self,
        source: &SampleSource,
        path: &Path,
        looped: bool,
        require_present: bool,
    ) -> Result<(), String> {
        selection_ops::set_sample_looped_for_source(self, source, path, looped, require_present)
    }

    // waveform loading helpers moved to `waveform_loading` submodule.
}
