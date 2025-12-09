use super::*;
use super::audio_cache::{CacheKey, FileMetadata};
use crate::egui_app::state::WaveformView;
use crate::waveform::DecodedWaveform;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Upper bound for waveform texture width to stay within GPU limits.
pub(super) const MAX_TEXTURE_WIDTH: u32 = 16_384;
const MIN_VIEW_WIDTH_BASE: f32 = 0.001;
const MIN_SAMPLES_PER_PIXEL: f32 = 1.0;
const MAX_ZOOM_MULTIPLIER: f32 = 64.0;

fn min_view_width_for(samples_len: usize, width_px: u32) -> f32 {
    if samples_len == 0 {
        return 1.0;
    }
    let samples = samples_len as f32;
    let pixels = width_px.max(1) as f32;
    (pixels * MIN_SAMPLES_PER_PIXEL / samples).clamp(MIN_VIEW_WIDTH_BASE, 1.0)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct WaveformRenderMeta {
    pub view_start: f32,
    pub view_end: f32,
    pub size: [u32; 2],
    pub samples_len: usize,
    pub texture_width: u32,
}

impl WaveformRenderMeta {
    fn matches(&self, other: &WaveformRenderMeta) -> bool {
        const EPS: f32 = 1e-4;
        self.samples_len == other.samples_len
            && self.size == other.size
            && self.texture_width == other.texture_width
            && (self.view_start - other.view_start).abs() < EPS
            && (self.view_end - other.view_end).abs() < EPS
    }
}

impl EguiController {
    pub(super) fn min_view_width(&self) -> f32 {
        if let Some(decoded) = self.decoded_waveform.as_ref() {
            min_view_width_for(decoded.samples.len(), self.waveform_size[0])
        } else {
            MIN_VIEW_WIDTH_BASE
        }
    }

    #[allow(dead_code)]
    pub(super) fn apply_view_bounds_with_min(&mut self, min_width: f32) -> WaveformView {
        let mut view = self.ui.waveform.view.clamp();
        let width = view.width().max(min_width);
        view.start = view.start.min(1.0 - width);
        view.end = (view.start + width).min(1.0);
        self.ui.waveform.view = view;
        view
    }
}

impl EguiController {
    /// Reset all waveform and playback visuals.
    pub(super) fn clear_waveform_view(&mut self) {
        self.ui.waveform.image = None;
        self.ui.waveform.notice = None;
        self.ui.waveform.loading = None;
        self.decoded_waveform = None;
        self.ui.waveform.playhead = PlayheadState::default();
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
        self.pending_audio = None;
        self.pending_playback = None;
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

    pub(super) fn poll_audio_loader(&mut self) {
        while let Ok(message) = self.audio_job_rx.try_recv() {
            let Some(pending) = self.pending_audio.clone() else {
                continue;
            };
            if message.request_id != pending.request_id
                || message.source_id != pending.source_id
                || message.relative_path != pending.relative_path
            {
                continue;
            }
            self.pending_audio = None;
            self.ui.waveform.loading = None;
            match message.result {
                Ok(outcome) => self.handle_audio_loaded(pending, outcome),
                Err(err) => self.handle_audio_load_error(pending, err),
            }
        }
    }

    fn handle_audio_loaded(&mut self, pending: PendingAudio, outcome: AudioLoadOutcome) {
        let Some(source) = self
            .sources
            .iter()
            .find(|s| s.id == pending.source_id)
            .cloned()
        else {
            return;
        };
        let AudioLoadOutcome {
            decoded,
            bytes,
            metadata,
        } = outcome;
        let duration_seconds = decoded.duration_seconds;
        let sample_rate = decoded.sample_rate;
        let cache_key = CacheKey::new(&source.id, &pending.relative_path);
        self.audio_cache
            .insert(cache_key, metadata, decoded.clone(), bytes.clone());
        if let Err(err) = self.finish_waveform_load(
            &source,
            &pending.relative_path,
            decoded,
            bytes,
            pending.intent,
        ) {
            self.pending_playback = None;
            self.set_status(err, StatusTone::Error);
            return;
        }
        let message =
            Self::loaded_status_text(&pending.relative_path, duration_seconds, sample_rate);
        self.set_status(message, StatusTone::Info);
        self.maybe_trigger_pending_playback();
    }

    fn handle_audio_load_error(&mut self, pending: PendingAudio, error: AudioLoadError) {
        let Some(source) = self
            .sources
            .iter()
            .find(|s| s.id == pending.source_id)
            .cloned()
        else {
            return;
        };
        if self.pending_playback.as_ref().is_some_and(|pending_play| {
            pending_play.source_id == pending.source_id
                && pending_play.relative_path == pending.relative_path
        }) {
            self.pending_playback = None;
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

    fn maybe_trigger_pending_playback(&mut self) {
        let Some(pending) = self.pending_playback.clone() else {
            return;
        };
        let Some(audio) = self.loaded_audio.as_ref() else {
            return;
        };
        if audio.source_id != pending.source_id || audio.relative_path != pending.relative_path {
            return;
        }
        self.pending_playback = None;
        if let Err(err) = self.play_audio(pending.looped, pending.start_override) {
            self.set_status(err, StatusTone::Error);
        }
    }

    pub(super) fn queue_audio_load_for(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
        intent: AudioLoadIntent,
        pending_playback: Option<PendingPlayback>,
    ) -> Result<(), String> {
        let request_id = self.next_audio_request_id;
        self.next_audio_request_id = self.next_audio_request_id.wrapping_add(1).max(1);
        let pending = PendingAudio {
            request_id,
            source_id: source.id.clone(),
            relative_path: relative_path.to_path_buf(),
            intent,
        };
        let job = AudioLoadJob {
            request_id,
            source_id: source.id.clone(),
            root: source.root.clone(),
            relative_path: relative_path.to_path_buf(),
        };
        self.pending_audio = None;
        self.pending_playback = pending_playback;
        self.ui.waveform.loading = Some(relative_path.to_path_buf());
        self.ui.waveform.notice = None;
        self.waveform_render_meta = None;
        self.decoded_waveform = None;
        self.ui.waveform.image = None;
        self.loaded_audio = None;
        self.loaded_wav = None;
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
        self.audio_job_tx
            .send(job)
            .map_err(|_| "Failed to queue audio load".to_string())?;
        self.pending_audio = Some(pending);
        Ok(())
    }

    fn try_use_cached_audio(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
        intent: AudioLoadIntent,
    ) -> Result<bool, String> {
        let metadata = match self.current_file_metadata(source, relative_path) {
            Ok(meta) => meta,
            Err(_) => return Ok(false),
        };
        let key = CacheKey::new(&source.id, relative_path);
        let Some(hit) = self.audio_cache.get(&key, metadata) else {
            return Ok(false);
        };
        let duration_seconds = hit.decoded.duration_seconds;
        let sample_rate = hit.decoded.sample_rate;
        self.finish_waveform_load(source, relative_path, hit.decoded, hit.bytes, intent)?;
        let message = Self::loaded_status_text(relative_path, duration_seconds, sample_rate);
        self.set_status(message, StatusTone::Info);
        Ok(true)
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
        self.ui.waveform.notice = None;
        self.ui.waveform.loading = None;
        self.clear_waveform_selection();
        self.pending_audio = None;
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

    pub(crate) fn refresh_waveform_image(&mut self) {
        let Some(decoded) = self.decoded_waveform.as_ref() else {
            return;
        };
        let [width, height] = self.waveform_size;
        let min_view_width = min_view_width_for(decoded.samples.len(), width);
        let mut view = self.ui.waveform.view.clamp();
        let width_clamped = view.width().max(min_view_width);
        view.start = view.start.min(1.0 - width_clamped);
        view.end = (view.start + width_clamped).min(1.0);
        let view = view;
        let max_zoom = (1.0 / min_view_width).min(MAX_ZOOM_MULTIPLIER);
        let zoom_scale = (1.0 / width_clamped).min(max_zoom).max(1.0);
        let target = (width as f32 * zoom_scale).ceil().max(width as f32) as usize;

        let samples = &decoded.samples;
        if samples.is_empty() {
            self.ui.waveform.image = None;
            return;
        }
        let total_samples = samples.len();
        let start_idx = ((view.start * total_samples as f32).floor() as usize)
            .min(total_samples.saturating_sub(1));
        let mut end_idx =
            ((view.end * total_samples as f32).ceil() as usize).clamp(start_idx + 1, total_samples);
        if end_idx <= start_idx {
            end_idx = (start_idx + 1).min(total_samples);
        }
        let upper_width = MAX_TEXTURE_WIDTH as usize;
        let lower_bound = width.min(MAX_TEXTURE_WIDTH) as usize;
        let effective_width = target.min(upper_width).max(lower_bound) as u32;
        let desired_meta = WaveformRenderMeta {
            view_start: view.start,
            view_end: view.end,
            size: [width, height],
            samples_len: samples.len(),
            texture_width: effective_width,
        };
        if self
            .waveform_render_meta
            .as_ref()
            .is_some_and(|meta| meta.matches(&desired_meta))
        {
            return;
        }
        let color_image = self.renderer.render_color_image_with_size(
            &samples[start_idx..end_idx],
            effective_width,
            height,
        );
        self.ui.waveform.image = Some(WaveformImage {
            image: color_image,
            view_start: view.start,
            view_end: view.end,
        });
        self.ui.waveform.view = view;
        self.waveform_render_meta = Some(desired_meta);
    }

    fn read_waveform_bytes(
        &self,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Result<Vec<u8>, String> {
        let full_path = source.root.join(relative_path);
        fs::read(&full_path).map_err(|err| format!("Failed to read {}: {err}", full_path.display()))
    }

    fn current_file_metadata(
        &self,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Result<FileMetadata, String> {
        let full_path = source.root.join(relative_path);
        let metadata = fs::metadata(&full_path)
            .map_err(|err| format!("Failed to read {}: {err}", full_path.display()))?;
        let modified_ns = metadata
            .modified()
            .map_err(|err| format!("Missing modified time for {}: {err}", full_path.display()))?
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .map_err(|_| "File modified time is before epoch".to_string())?
            .as_nanos() as i64;
        Ok(FileMetadata {
            file_size: metadata.len(),
            modified_ns,
        })
    }

    pub(super) fn invalidate_cached_audio(
        &mut self,
        source_id: &SourceId,
        relative_path: &Path,
    ) {
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
        self.invalidate_cached_audio(&source.id, relative_path);
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
