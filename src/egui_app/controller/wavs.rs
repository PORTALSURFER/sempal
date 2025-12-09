use super::*;
use crate::egui_app::state::WaveformView;
use crate::waveform::DecodedWaveform;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

impl EguiController {
    /// Reset all waveform and playback visuals.
    pub(super) fn clear_waveform_view(&mut self) {
        self.ui.waveform.image = None;
        self.ui.waveform.notice = None;
        self.decoded_waveform = None;
        self.ui.waveform.playhead = PlayheadState::default();
        self.ui.waveform.selection = None;
        self.ui.waveform.selection_duration = None;
        self.ui.waveform.view = WaveformView::default();
        self.selection.clear();
        self.loaded_audio = None;
        self.loaded_wav = None;
        self.ui.loaded_wav = None;
        if let Some(player) = self.player.as_ref() {
            player.borrow_mut().stop();
        }
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

    /// Select a wav row based on its path.
    pub fn select_wav_by_path(&mut self, path: &Path) {
        self.select_wav_by_path_with_rebuild(path, true);
    }

    /// Select a wav row based on its path, optionally delaying the browser rebuild.
    pub fn select_wav_by_path_with_rebuild(&mut self, path: &Path, rebuild: bool) {
        if !self.wav_lookup.contains_key(path) {
            return;
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
            if let Err(err) = self.load_waveform_for_selection(&source, path) {
                self.set_status(err, StatusTone::Error);
            } else if self.feature_flags.autoplay_selection && !self.suppress_autoplay_once {
                let _ = self.play_audio(self.ui.waveform.loop_enabled, None);
            } else {
                self.suppress_autoplay_once = false;
            }
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
        self.label_for(index)
    }

    pub(super) fn rebuild_wav_lookup(&mut self) {
        self.wav_lookup.clear();
        for (index, entry) in self.wav_entries.iter().enumerate() {
            self.wav_lookup.insert(entry.relative_path.clone(), index);
        }
    }

    pub(super) fn rebuild_browser_lists(&mut self) {
        if self.ui.collections.selected_sample.is_some() {
            self.ui.browser.autoscroll = false;
        }
        self.prune_browser_selection();
        let highlight_selection = self.ui.collections.selected_sample.is_none();
        let focused_index = if highlight_selection {
            self.selected_row_index()
        } else {
            None
        };
        let loaded_index = if highlight_selection {
            self.loaded_row_index()
        } else {
            None
        };
        self.reset_browser_ui();

        for i in 0..self.wav_entries.len() {
            let tag = self.wav_entries[i].tag;
            let flags = RowFlags {
                focused: Some(i) == focused_index,
                loaded: Some(i) == loaded_index,
            };
            self.push_browser_row(i, tag, flags);
        }
        let visible_len = self.ui.browser.visible.len();
        if let Some(anchor) = self.ui.browser.selection_anchor_visible {
            if anchor >= visible_len {
                self.ui.browser.selection_anchor_visible = self.ui.browser.selected_visible;
            }
        }
    }

    pub(super) fn selected_row_index(&self) -> Option<usize> {
        self.selected_wav
            .as_ref()
            .and_then(|path| self.wav_lookup.get(path).copied())
    }

    pub(super) fn loaded_row_index(&self) -> Option<usize> {
        self.loaded_wav
            .as_ref()
            .and_then(|path| self.wav_lookup.get(path).copied())
    }

    fn reset_browser_ui(&mut self) {
        let autoscroll = self.ui.browser.autoscroll;
        let collections_selected = self.ui.collections.selected_sample.is_some();
        self.ui.browser.trash.clear();
        self.ui.browser.neutral.clear();
        self.ui.browser.keep.clear();
        self.ui.browser.visible.clear();
        self.ui.browser.selected_visible = None;
        if collections_selected {
            self.ui.browser.selected = None;
        }
        self.ui.browser.loaded = None;
        self.ui.browser.loaded_visible = None;
        self.ui.browser.autoscroll = autoscroll && !collections_selected;
        self.ui.loaded_wav = None;
    }

    fn push_browser_row(&mut self, entry_index: usize, tag: SampleTag, flags: RowFlags) {
        let target = match tag {
            SampleTag::Trash => &mut self.ui.browser.trash,
            SampleTag::Neutral => &mut self.ui.browser.neutral,
            SampleTag::Keep => &mut self.ui.browser.keep,
        };
        let row_index = target.len();
        target.push(entry_index);
        if self.browser_filter_accepts(tag) {
            let visible_row = self.ui.browser.visible.len();
            self.ui.browser.visible.push(entry_index);
            if flags.focused {
                self.ui.browser.selected_visible = Some(visible_row);
            }
            if flags.loaded {
                self.ui.browser.loaded_visible = Some(visible_row);
            }
        }
        if flags.focused {
            self.ui.browser.selected = Some(view_model::sample_browser_index_for(tag, row_index));
        }
        if flags.loaded {
            self.ui.browser.loaded = Some(view_model::sample_browser_index_for(tag, row_index));
            if let Some(path) = self.wav_entries.get(entry_index) {
                self.ui.loaded_wav = Some(path.relative_path.clone());
            }
        }
    }

    fn prune_browser_selection(&mut self) {
        self.ui
            .browser
            .selected_paths
            .retain(|path| self.wav_lookup.contains_key(path));
        if let Some(path) = self.selected_wav.clone() {
            if !self.wav_lookup.contains_key(&path) {
                self.selected_wav = None;
                self.ui.browser.selected = None;
                self.ui.browser.selected_visible = None;
                self.clear_waveform_view();
            }
        }
    }

    fn browser_filter_accepts(&self, tag: SampleTag) -> bool {
        match self.ui.browser.filter {
            TriageFlagFilter::All => true,
            TriageFlagFilter::Keep => matches!(tag, SampleTag::Keep),
            TriageFlagFilter::Trash => matches!(tag, SampleTag::Trash),
            TriageFlagFilter::Untagged => matches!(tag, SampleTag::Neutral),
        }
    }

    pub(super) fn focused_browser_row(&self) -> Option<usize> {
        self.ui.browser.selected_visible
    }

    pub(super) fn focused_browser_path(&self) -> Option<PathBuf> {
        let row = self.focused_browser_row()?;
        self.browser_path_for_visible(row)
    }

    fn browser_path_for_visible(&self, visible_row: usize) -> Option<PathBuf> {
        let index = self.ui.browser.visible.get(visible_row).copied()?;
        self.wav_entries
            .get(index)
            .map(|entry| entry.relative_path.clone())
    }

    fn visible_row_for_path(&self, path: &Path) -> Option<usize> {
        let entry_index = self.wav_lookup.get(path)?;
        self.ui
            .browser
            .visible
            .iter()
            .position(|idx| idx == entry_index)
    }

    fn set_single_browser_selection(&mut self, path: &Path) {
        self.ui.browser.selected_paths.clear();
        self.ui.browser.selected_paths.push(path.to_path_buf());
    }

    fn toggle_browser_selection(&mut self, path: &Path) {
        if let Some(pos) = self
            .ui
            .browser
            .selected_paths
            .iter()
            .position(|p| p == path)
        {
            self.ui.browser.selected_paths.remove(pos);
        } else {
            self.ui.browser.selected_paths.push(path.to_path_buf());
        }
    }

    fn extend_browser_selection_to(&mut self, target_visible: usize) {
        if self.ui.browser.visible.is_empty() {
            return;
        }
        let anchor = self
            .ui
            .browser
            .selection_anchor_visible
            .unwrap_or_else(|| self.ui.browser.selected_visible.unwrap_or(target_visible));
        let start = anchor.min(target_visible);
        let end = anchor.max(target_visible);
        for row in start..=end {
            if let Some(path) = self.browser_path_for_visible(row) {
                if !self.ui.browser.selected_paths.iter().any(|p| p == &path) {
                    self.ui.browser.selected_paths.push(path);
                }
            }
        }
    }

    pub fn focus_browser_row(&mut self, visible_row: usize) {
        self.apply_browser_selection(visible_row, SelectionAction::Replace);
    }

    /// Focus a browser row without mutating the multi-selection set.
    pub fn focus_browser_row_only(&mut self, visible_row: usize) {
        let Some(path) = self.browser_path_for_visible(visible_row) else {
            return;
        };
        self.ui.collections.selected_sample = None;
        self.focus_browser_context();
        self.ui.browser.autoscroll = true;
        self.ui.browser.selection_anchor_visible = Some(visible_row);
        self.select_wav_by_path_with_rebuild(&path, true);
    }

    pub fn toggle_browser_row_selection(&mut self, visible_row: usize) {
        self.apply_browser_selection(visible_row, SelectionAction::Toggle);
    }

    pub fn extend_browser_selection_to_row(&mut self, visible_row: usize) {
        self.apply_browser_selection(visible_row, SelectionAction::Extend);
    }

    pub fn toggle_focused_selection(&mut self) {
        let Some(path) = self.selected_wav.clone() else {
            return;
        };
        if let Some(row) = self.ui.browser.selected_visible {
            if self.ui.browser.selection_anchor_visible.is_none() {
                self.ui.browser.selection_anchor_visible = Some(row);
            }
        }
        self.toggle_browser_selection(&path);
        self.rebuild_browser_lists();
    }

    /// Clear the multi-selection set.
    pub fn clear_browser_selection(&mut self) {
        if self.ui.browser.selected_paths.is_empty() {
            return;
        }
        self.ui.browser.selected_paths.clear();
        self.ui.browser.selection_anchor_visible = None;
        self.rebuild_browser_lists();
    }

    fn apply_browser_selection(&mut self, visible_row: usize, action: SelectionAction) {
        let Some(path) = self.browser_path_for_visible(visible_row) else {
            return;
        };
        self.ui.collections.selected_sample = None;
        self.focus_browser_context();
        self.ui.browser.autoscroll = true;
        match action {
            SelectionAction::Replace => {
                self.ui.browser.selection_anchor_visible = Some(visible_row);
                self.set_single_browser_selection(&path);
            }
            SelectionAction::Toggle => {
                let anchor = self
                    .ui
                    .browser
                    .selection_anchor_visible
                    .or(self.ui.browser.selected_visible)
                    .unwrap_or(visible_row);
                self.ui.browser.selection_anchor_visible = Some(anchor);
                if self.ui.browser.selected_paths.is_empty() && anchor != visible_row {
                    if let Some(anchor_path) = self.browser_path_for_visible(anchor) {
                        if !self
                            .ui
                            .browser
                            .selected_paths
                            .iter()
                            .any(|p| p == &anchor_path)
                        {
                            self.ui.browser.selected_paths.push(anchor_path);
                        }
                    }
                }
                self.toggle_browser_selection(&path);
            }
            SelectionAction::Extend => {
                let anchor = self
                    .ui
                    .browser
                    .selection_anchor_visible
                    .or(self.ui.browser.selected_visible)
                    .unwrap_or(visible_row);
                self.ui.browser.selection_anchor_visible = Some(anchor);
                self.extend_browser_selection_to(visible_row);
            }
        }
        self.select_wav_by_path_with_rebuild(&path, false);
        self.rebuild_browser_lists();
    }

    pub fn action_rows_from_primary(&self, primary_visible_row: usize) -> Vec<usize> {
        let mut rows: Vec<usize> = self
            .ui
            .browser
            .selected_paths
            .iter()
            .filter_map(|path| self.visible_row_for_path(path))
            .collect();
        if rows.is_empty() {
            rows.push(primary_visible_row);
        } else if !rows.contains(&primary_visible_row) {
            rows.push(primary_visible_row);
        }
        rows.sort_unstable();
        rows.dedup();
        rows
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
        self.apply_tag_to_caches(source, path, target_tag, require_present)?;
        let _ = db.set_tag(path, target_tag);
        if self.selected_source.as_ref() == Some(&source.id) {
            self.rebuild_browser_lists();
        }
        Ok(())
    }

    fn apply_tag_to_caches(
        &mut self,
        source: &SampleSource,
        path: &Path,
        target_tag: SampleTag,
        require_present: bool,
    ) -> Result<(), String> {
        if self.selected_source.as_ref() == Some(&source.id) {
            if let Some(index) = self.wav_lookup.get(path).copied() {
                if let Some(entry) = self.wav_entries.get_mut(index) {
                    entry.tag = target_tag;
                }
            } else if require_present {
                return Err("Sample not found".into());
            }
        }
        if let Some(cache) = self.wav_cache.get_mut(&source.id) {
            if let Some(entry) = cache.iter_mut().find(|entry| entry.relative_path == path) {
                entry.tag = target_tag;
            }
        }
        Ok(())
    }

    fn label_for(&mut self, index: usize) -> Option<String> {
        let source_id = self.selected_source.clone()?;
        if let Some(cache) = self.label_cache.get(&source_id) {
            if let Some(label) = cache.get(index) {
                return Some(label.clone());
            }
        }
        let labels: Vec<String> = self
            .wav_entries
            .iter()
            .map(|entry| view_model::sample_display_label(&entry.relative_path))
            .collect();
        let result = labels.get(index).cloned();
        self.label_cache.insert(source_id, labels);
        result
    }

    pub(super) fn build_label_cache(&self, entries: &[WavEntry]) -> Vec<String> {
        entries
            .iter()
            .map(|entry| view_model::sample_display_label(&entry.relative_path))
            .collect()
    }

    pub(super) fn load_waveform_for_selection(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Result<(), String> {
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
        let channels = decoded.channels;
        self.apply_waveform_image(decoded);
        self.ui.waveform.view = WaveformView::default();
        self.ui.waveform.notice = None;
        self.clear_waveform_selection();
        self.loaded_wav = Some(relative_path.to_path_buf());
        self.ui.loaded_wav = Some(relative_path.to_path_buf());
        self.sync_loaded_audio(
            source,
            relative_path,
            duration_seconds,
            sample_rate,
            channels,
            bytes,
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
        let channels = decoded.channels;
        self.apply_waveform_image(decoded);
        self.ui.waveform.view = WaveformView::default();
        self.ui.waveform.notice = None;
        self.clear_waveform_selection();
        self.loaded_wav = None;
        self.ui.loaded_wav = None;
        self.sync_loaded_audio(
            source,
            relative_path,
            duration_seconds,
            sample_rate,
            channels,
            bytes,
        )?;
        let message = Self::loaded_status_text(relative_path, duration_seconds, sample_rate);
        self.set_status(message, StatusTone::Info);
        Ok(())
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

    fn apply_waveform_image(&mut self, decoded: DecodedWaveform) {
        self.decoded_waveform = Some(decoded);
        self.refresh_waveform_image();
    }

    /// Update the waveform render target to match the current view size.
    pub fn update_waveform_size(&mut self, width: u32, height: u32) {
        let width = width.max(1);
        let height = height.max(1);
        if self.waveform_size == [width, height] {
            return;
        }
        self.waveform_size = [width, height];
        self.refresh_waveform_image();
    }

    fn refresh_waveform_image(&mut self) {
        let Some(decoded) = self.decoded_waveform.as_ref() else {
            return;
        };
        let [width, height] = self.waveform_size;
        let color_image =
            self.renderer
                .render_color_image_with_size(&decoded.samples, width, height);
        self.ui.waveform.image = Some(WaveformImage { image: color_image });
    }

    fn read_waveform_bytes(
        &self,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Result<Vec<u8>, String> {
        let full_path = source.root.join(relative_path);
        fs::read(&full_path).map_err(|err| format!("Failed to read {}: {err}", full_path.display()))
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

    pub(super) fn rebuild_missing_lookup_for_source(&mut self, source_id: &SourceId) {
        let mut missing = HashSet::new();
        if let Some(cache) = self.wav_cache.get(source_id) {
            for entry in cache {
                if entry.missing {
                    missing.insert(entry.relative_path.clone());
                }
            }
        } else if self.selected_source.as_ref() == Some(source_id) {
            for entry in &self.wav_entries {
                if entry.missing {
                    missing.insert(entry.relative_path.clone());
                }
            }
        }
        self.missing_wavs.insert(source_id.clone(), missing);
    }

    pub(super) fn mark_sample_missing(&mut self, source: &SampleSource, relative_path: &Path) {
        match self.database_for(source) {
            Ok(db) => {
                let _ = db.set_missing(relative_path, true);
            }
            Err(SourceDbError::InvalidRoot(_)) => {
                self.mark_source_missing(&source.id, "Source folder missing");
            }
            Err(err) => {
                self.set_status(
                    format!("Failed to update missing flag: {err}"),
                    StatusTone::Warning,
                );
            }
        }
        if let Some(cache) = self.wav_cache.get_mut(&source.id) {
            if let Some(entry) = cache
                .iter_mut()
                .find(|entry| entry.relative_path == relative_path)
            {
                entry.missing = true;
            }
        }
        if self.selected_source.as_ref() == Some(&source.id) {
            if let Some(index) = self.wav_lookup.get(relative_path).copied() {
                if let Some(entry) = self.wav_entries.get_mut(index) {
                    entry.missing = true;
                }
            }
        }
        self.missing_wavs
            .entry(source.id.clone())
            .or_insert_with(HashSet::new)
            .insert(relative_path.to_path_buf());
    }

    pub(super) fn ensure_missing_lookup_for_source(
        &mut self,
        source: &SampleSource,
    ) -> Result<(), String> {
        if self.missing_wavs.contains_key(&source.id) {
            return Ok(());
        }
        if self.missing_sources.contains(&source.id) {
            self.missing_wavs
                .entry(source.id.clone())
                .or_insert_with(HashSet::new);
            return Ok(());
        }
        let db = match self.database_for(source) {
            Ok(db) => db,
            Err(err) => {
                if matches!(err, SourceDbError::InvalidRoot(_)) {
                    self.mark_source_missing(&source.id, "Source folder missing");
                }
                return Err(err.to_string());
            }
        };
        let paths = db
            .list_missing_paths()
            .map_err(|err| format!("Failed to read missing files: {err}"))?;
        self.missing_wavs
            .insert(source.id.clone(), paths.into_iter().collect());
        Ok(())
    }

    pub(super) fn sample_missing(&mut self, source_id: &SourceId, relative_path: &Path) -> bool {
        if self.missing_sources.contains(source_id) {
            return true;
        }
        if self.selected_source.as_ref() == Some(source_id) {
            if let Some(index) = self.wav_lookup.get(relative_path) {
                if let Some(entry) = self.wav_entries.get(*index) {
                    return entry.missing;
                }
            }
        }
        if let Some(cache) = self.wav_cache.get(source_id) {
            if let Some(entry) = cache
                .iter()
                .find(|entry| entry.relative_path == relative_path)
            {
                return entry.missing;
            }
        }
        if let Some(set) = self.missing_wavs.get(source_id) {
            return set.contains(relative_path);
        }
        if let Some(source) = self.sources.iter().find(|s| &s.id == source_id).cloned() {
            if let Err(err) = self.ensure_missing_lookup_for_source(&source) {
                self.set_status(err, StatusTone::Warning);
                return true;
            }
            if let Some(set) = self.missing_wavs.get(source_id) {
                return set.contains(relative_path);
            }
        }
        false
    }

    pub(super) fn show_missing_waveform_notice(&mut self, relative_path: &Path) {
        let message = format!("File missing: {}", relative_path.display());
        self.clear_waveform_view();
        self.ui.waveform.notice = Some(message);
    }
}

#[derive(Clone, Copy)]
enum SelectionAction {
    Replace,
    Toggle,
    Extend,
}
