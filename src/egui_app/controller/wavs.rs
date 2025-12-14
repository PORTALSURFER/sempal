use super::audio_cache::{CacheKey, FileMetadata};
use super::*;
use crate::egui_app::state::{FocusContext, WaveformView};
use crate::egui_app::view_model;
use crate::waveform::DecodedWaveform;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

mod browser_search;
mod browser_actions;
mod audio_loading;

pub(super) use browser_search::BrowserSearchCache;

/// Upper bound for waveform texture width to stay within GPU limits.
pub(super) const MAX_TEXTURE_WIDTH: u32 = 16_384;
const MIN_VIEW_WIDTH_BASE: f32 = 0.001;
const MIN_SAMPLES_PER_PIXEL: f32 = 1.0;
const MAX_ZOOM_MULTIPLIER: f32 = 64.0;

fn min_view_width_for_frames(frame_count: usize, width_px: u32) -> f32 {
    if frame_count == 0 {
        return 1.0;
    }
    let samples = frame_count as f32;
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
    pub channel_view: crate::waveform::WaveformChannelView,
    pub channels: u16,
}

impl WaveformRenderMeta {
    /// Check whether two render targets describe the same view and layout.
    pub(super) fn matches(&self, other: &WaveformRenderMeta) -> bool {
        let width = (self.view_end - self.view_start)
            .abs()
            .max((other.view_end - other.view_start).abs())
            .max(1e-6);
        let eps = (width * 0.01).max(1e-5);
        self.samples_len == other.samples_len
            && self.size == other.size
            && self.texture_width == other.texture_width
            && self.channel_view == other.channel_view
            && self.channels == other.channels
            && (self.view_start - other.view_start).abs() < eps
            && (self.view_end - other.view_end).abs() < eps
    }
}

impl EguiController {
    pub(super) fn min_view_width(&self) -> f32 {
        if let Some(decoded) = self.decoded_waveform.as_ref() {
            min_view_width_for_frames(decoded.frame_count(), self.waveform_size[0])
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

    pub(super) fn rebuild_browser_lists(&mut self) {
        if self.ui.collections.selected_sample.is_some() {
            self.ui.browser.autoscroll = false;
        }
        self.prune_browser_selection();
        let allow_highlight = matches!(
            self.ui.focus.context,
            FocusContext::SampleBrowser | FocusContext::Waveform | FocusContext::None
        );
        let highlight_selection = self.ui.collections.selected_sample.is_none() && allow_highlight;
        let focused_index = highlight_selection
            .then_some(self.selected_row_index())
            .flatten();
        let loaded_index = highlight_selection
            .then_some(self.loaded_row_index())
            .flatten();
        self.reset_browser_ui();

        for i in 0..self.wav_entries.len() {
            let tag = self.wav_entries[i].tag;
            let flags = RowFlags {
                focused: Some(i) == focused_index,
                loaded: Some(i) == loaded_index,
            };
            self.push_browser_row(i, tag, flags);
        }
        let (visible, selected_visible, loaded_visible) =
            self.build_visible_rows(focused_index, loaded_index);
        self.ui.browser.visible = visible;
        self.ui.browser.selected_visible = selected_visible;
        self.ui.browser.loaded_visible = loaded_visible;
        let visible_len = self.ui.browser.visible.len();
        if let Some(anchor) = self.ui.browser.selection_anchor_visible
            && anchor >= visible_len
        {
            self.ui.browser.selection_anchor_visible = self.ui.browser.selected_visible;
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
        if let Some(path) = self.selected_wav.clone()
            && !self.wav_lookup.contains_key(&path)
        {
            self.selected_wav = None;
            self.ui.browser.selected = None;
            self.ui.browser.selected_visible = None;
            self.clear_waveform_view();
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

    fn apply_waveform_image(&mut self, decoded: DecodedWaveform) {
        // Force a rerender whenever decoded samples change, even if the view metadata is
        // identical to the previous render.
        self.waveform_render_meta = None;
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
        let total_frames = decoded.frame_count();
        let min_view_width = min_view_width_for_frames(total_frames, width);
        let mut view = self.ui.waveform.view.clamp();
        let width_clamped = view.width().max(min_view_width);
        view.start = view.start.min(1.0 - width_clamped);
        view.end = (view.start + width_clamped).min(1.0);
        let view = view;
        let max_zoom = (1.0 / min_view_width).min(MAX_ZOOM_MULTIPLIER);
        let zoom_scale = (1.0 / width_clamped).min(max_zoom).max(1.0);
        let target = (width as f32 * zoom_scale).ceil().max(width as f32) as usize;

        if (decoded.samples.is_empty() && decoded.peaks.is_none()) || total_frames == 0 {
            self.ui.waveform.image = None;
            return;
        }
        let start_frame = ((view.start * total_frames as f32).floor() as usize)
            .min(total_frames.saturating_sub(1));
        let mut end_frame =
            ((view.end * total_frames as f32).ceil() as usize).clamp(start_frame + 1, total_frames);
        if end_frame <= start_frame {
            end_frame = (start_frame + 1).min(total_frames);
        }
        let frames_in_view = end_frame.saturating_sub(start_frame).max(1);
        let upper_width = frames_in_view.min(MAX_TEXTURE_WIDTH as usize);
        let lower_bound = width.min(MAX_TEXTURE_WIDTH) as usize;
        let effective_width = target.min(upper_width).max(lower_bound) as u32;
        let desired_meta = WaveformRenderMeta {
            view_start: view.start,
            view_end: view.end,
            size: [width, height],
            samples_len: total_frames,
            texture_width: effective_width,
            channel_view: self.ui.waveform.channel_view,
            channels: decoded.channels,
        };
        if self
            .waveform_render_meta
            .as_ref()
            .is_some_and(|meta| meta.matches(&desired_meta))
        {
            return;
        }
        let color_image = self.renderer.render_color_image_for_view_with_size(
            decoded,
            view.start,
            view.end,
            self.ui.waveform.channel_view,
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
        let bytes =
            fs::read(&full_path).map_err(|err| format!("Failed to read {}: {err}", full_path.display()))?;
        Ok(crate::wav_sanitize::sanitize_wav_bytes(bytes))
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
        if let Some(cache) = self.wav_cache.get_mut(&source.id)
            && let Some(entry) = cache
                .iter_mut()
                .find(|entry| entry.relative_path == relative_path)
        {
            entry.missing = true;
        }
        if self.selected_source.as_ref() == Some(&source.id)
            && let Some(index) = self.wav_lookup.get(relative_path).copied()
            && let Some(entry) = self.wav_entries.get_mut(index)
        {
            entry.missing = true;
        }
        self.missing_wavs
            .entry(source.id.clone())
            .or_default()
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
            self.missing_wavs.entry(source.id.clone()).or_default();
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
        if self.selected_source.as_ref() == Some(source_id)
            && let Some(index) = self.wav_lookup.get(relative_path)
            && let Some(entry) = self.wav_entries.get(*index)
        {
            return entry.missing;
        }
        if self.wav_cache.contains_key(source_id) {
            self.ensure_wav_cache_lookup(source_id);
            if let Some(index) = self
                .wav_cache_lookup
                .get(source_id)
                .and_then(|lookup| lookup.get(relative_path))
                .copied()
                && let Some(cache) = self.wav_cache.get(source_id)
                && let Some(entry) = cache.get(index)
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
