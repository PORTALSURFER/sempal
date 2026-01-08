use super::EguiController;
use super::MIN_SELECTION_WIDTH;
use super::audio_samples::{DecodedSamples, crop_samples, decode_samples_from_bytes, write_wav};
use crate::analysis::audio::{detect_non_silent_ranges, downmix_to_mono_into};
use crate::sample_sources::SampleSource;
use crate::selection::SelectionRange;
use std::cmp::Ordering;
use std::path::{Path, PathBuf};

mod ops;

impl EguiController {
    /// Detect non-silent slice ranges for the loaded waveform and store them in UI state.
    pub(crate) fn detect_waveform_slices_from_silence(&mut self) -> Result<usize, String> {
        let audio = self
            .sample_view
            .wav
            .loaded_audio
            .as_ref()
            .ok_or_else(|| "Load a sample before slicing".to_string())?;
        let decoded = decode_samples_from_bytes(&audio.bytes)?;
        let mut mono = Vec::new();
        downmix_to_mono_into(&mut mono, &decoded.samples, decoded.channels);
        let total_frames = mono.len();
        if total_frames == 0 {
            return Err("No audio data to slice".into());
        }
        let mut slices = Vec::new();
        let use_transients = self.ui.waveform.transient_markers_enabled
            && self.ui.waveform.transient_snap_enabled
            && !self.ui.waveform.transients.is_empty();
        let transients = if use_transients {
            let mut positions = self.ui.waveform.transients.clone();
            positions.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
            positions
        } else {
            Vec::new()
        };
        let non_silent_ranges = detect_non_silent_ranges(&mono, decoded.sample_rate)
            .into_iter()
            .map(|(start, end)| {
                let start_norm = start as f32 / total_frames as f32;
                let end_norm = end as f32 / total_frames as f32;
                SelectionRange::new(start_norm, end_norm)
            })
            .filter(|range| range.width() >= MIN_SELECTION_WIDTH)
            .collect::<Vec<_>>();
        if use_transients {
            append_slices_from_transients(&mut slices, &non_silent_ranges, &transients);
        } else {
            slices.extend(non_silent_ranges);
        }
        self.ui.waveform.slices = slices;
        self.ui.waveform.selected_slices.clear();
        Ok(self.ui.waveform.slices.len())
    }

    /// Clear any detected slice ranges from the waveform view.
    pub(crate) fn clear_waveform_slices(&mut self) {
        self.ui.waveform.slices.clear();
        self.ui.waveform.selected_slices.clear();
    }

    /// Apply a manually painted slice, cutting it out of any overlapping slices.
    pub(crate) fn apply_painted_slice(&mut self, range: SelectionRange) -> bool {
        let Some(updated) =
            ops::apply_painted_slice(&self.ui.waveform.slices, range, MIN_SELECTION_WIDTH)
        else {
            return false;
        };
        self.ui.waveform.slices = updated;
        self.ui.waveform.selected_slices.clear();
        true
    }

    /// Update an existing slice range, cutting it out of any overlapping slices.
    pub(crate) fn update_slice_range(
        &mut self,
        index: usize,
        range: SelectionRange,
    ) -> Option<usize> {
        let updated = ops::update_slice_range(
            &self.ui.waveform.slices,
            &self.ui.waveform.selected_slices,
            index,
            range,
            MIN_SELECTION_WIDTH,
        )?;
        self.ui.waveform.slices = updated.slices;
        self.ui.waveform.selected_slices = updated.selected_indices;
        updated.new_index
    }

    /// Snap a slice paint position to BPM or transient markers when enabled.
    pub(crate) fn snap_slice_paint_position(&self, position: f32, snap_override: bool) -> f32 {
        let state = ops::SliceSnapState {
            bpm_snap_enabled: self.ui.waveform.bpm_snap_enabled,
            bpm_value: self.ui.waveform.bpm_value,
            duration_seconds: self
                .sample_view
                .wav
                .loaded_audio
                .as_ref()
                .map(|audio| audio.duration_seconds),
            transient_markers_enabled: self.ui.waveform.transient_markers_enabled,
            transient_snap_enabled: self.ui.waveform.transient_snap_enabled,
            transients: self.ui.waveform.transients.clone(),
        };
        ops::snap_slice_paint_position(&state, position, snap_override)
    }

    /// Export detected slices to new audio files and register them in the browser.
    pub(super) fn accept_waveform_slices(&mut self) -> Result<usize, String> {
        if self.ui.waveform.slices.is_empty() {
            return Err("No slices to export".into());
        }
        let (source, relative_path, decoded) = self.slice_export_context()?;
        let mut counter = 1usize;
        let exported = self.export_slice_batch(&source, &relative_path, &decoded, &mut counter)?;
        self.ui.waveform.slices.clear();
        self.ui.waveform.selected_slices.clear();
        Ok(exported)
    }

    /// Toggle the selection state for a slice index.
    pub(crate) fn toggle_slice_selection(&mut self, index: usize) -> bool {
        if index >= self.ui.waveform.slices.len() {
            return false;
        }
        if let Some(pos) = self
            .ui
            .waveform
            .selected_slices
            .iter()
            .position(|value| *value == index)
        {
            self.ui.waveform.selected_slices.swap_remove(pos);
            return false;
        }
        self.ui.waveform.selected_slices.push(index);
        self.ui.waveform.selected_slices.sort_unstable();
        true
    }

    /// Remove selected slice ranges from the waveform.
    pub(crate) fn delete_selected_slices(&mut self) -> usize {
        if self.ui.waveform.selected_slices.is_empty() {
            return 0;
        }
        let mut indices = self.ui.waveform.selected_slices.clone();
        indices.sort_unstable();
        indices.dedup();
        let mut removed = 0usize;
        for index in indices.into_iter().rev() {
            if index < self.ui.waveform.slices.len() {
                self.ui.waveform.slices.remove(index);
                removed += 1;
            }
        }
        self.ui.waveform.selected_slices.clear();
        removed
    }

    /// Merge selected slice ranges into a single range that spans them.
    pub(crate) fn merge_selected_slices(&mut self) -> Option<SelectionRange> {
        if self.ui.waveform.selected_slices.len() < 2 {
            return None;
        }
        let mut indices = self.ui.waveform.selected_slices.clone();
        indices.sort_unstable();
        indices.dedup();
        let mut min_start: f32 = 1.0;
        let mut max_end: f32 = 0.0;
        for &index in &indices {
            if let Some(slice) = self.ui.waveform.slices.get(index) {
                min_start = min_start.min(slice.start());
                max_end = max_end.max(slice.end());
            }
        }
        if max_end <= min_start {
            return None;
        }
        let merged = SelectionRange::new(min_start, max_end);
        self.ui.waveform.slices = self
            .ui
            .waveform
            .slices
            .iter()
            .copied()
            .filter(|slice| !ops::ranges_overlap(*slice, merged))
            .collect();
        self.ui.waveform.slices.push(merged);
        self.ui
            .waveform
            .slices
            .sort_by(|a, b| a.start().partial_cmp(&b.start()).unwrap_or(Ordering::Equal));
        let merged_index = self
            .ui
            .waveform
            .slices
            .iter()
            .position(|slice| *slice == merged)
            .unwrap_or(0);
        self.ui.waveform.selected_slices = vec![merged_index];
        Some(merged)
    }

    fn export_slice_batch(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
        decoded: &DecodedSamples,
        counter: &mut usize,
    ) -> Result<usize, String> {
        let mut exported = 0usize;
        for slice in self.ui.waveform.slices.clone() {
            self.export_single_slice(source, relative_path, decoded, slice, counter)?;
            exported += 1;
        }
        Ok(exported)
    }

    fn export_single_slice(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
        decoded: &DecodedSamples,
        slice: SelectionRange,
        counter: &mut usize,
    ) -> Result<(), String> {
        let samples = crop_samples(&decoded.samples, decoded.channels, slice)?;
        let target_rel = self.next_slice_path_in_dir(source, relative_path, counter);
        let target_abs = source.root.join(&target_rel);
        write_wav(&target_abs, &samples, decoded.sample_rate, decoded.channels)?;
        self.record_selection_entry(source, target_rel, None, true, true)?;
        Ok(())
    }

    fn slice_export_context(&self) -> Result<(SampleSource, PathBuf, DecodedSamples), String> {
        let audio = self
            .sample_view
            .wav
            .loaded_audio
            .as_ref()
            .ok_or_else(|| "Load a sample before exporting slices".to_string())?;
        let decoded = decode_samples_from_bytes(&audio.bytes)?;
        let source = self
            .library
            .sources
            .iter()
            .find(|s| s.id == audio.source_id)
            .cloned()
            .ok_or_else(|| "Source not available".to_string())?;
        Ok((source, audio.relative_path.clone(), decoded))
    }

    fn next_slice_path_in_dir(
        &self,
        source: &SampleSource,
        original: &Path,
        counter: &mut usize,
    ) -> PathBuf {
        let parent = original.parent().unwrap_or_else(|| Path::new(""));
        let stem = original
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("slice");
        let stem = strip_slice_suffix(stem);
        loop {
            let suffix = format!("slice{:03}", counter);
            let candidate = parent.join(format!("{stem}_{suffix}.wav"));
            let absolute = source.root.join(&candidate);
            if !absolute.exists() {
                *counter = counter.saturating_add(1);
                return candidate;
            }
            *counter = counter.saturating_add(1);
        }
    }
}

fn strip_slice_suffix(stem: &str) -> &str {
    if let Some((prefix, suffix)) = stem.rsplit_once("_slice")
        && !prefix.is_empty()
        && !suffix.is_empty()
        && suffix.chars().all(|c| c.is_ascii_digit())
    {
        return prefix;
    }
    stem
}

fn append_slices_from_transients(
    slices: &mut Vec<SelectionRange>,
    non_silent_ranges: &[SelectionRange],
    transients: &[f32],
) {
    if non_silent_ranges.is_empty() {
        return;
    }
    let mut points = Vec::with_capacity(transients.len() + 2);
    points.push(0.0);
    points.extend(
        transients
            .iter()
            .copied()
            .filter(|marker| *marker > 0.0 && *marker < 1.0),
    );
    points.push(1.0);
    points.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    for pair in points.windows(2) {
        let slice = SelectionRange::new(pair[0], pair[1]);
        if slice.width() < MIN_SELECTION_WIDTH {
            continue;
        }
        if non_silent_ranges
            .iter()
            .any(|range| ops::ranges_overlap(*range, slice))
        {
            slices.push(slice);
        }
    }
}

#[cfg(test)]
mod ops_tests;
#[cfg(test)]
mod slices_tests;
