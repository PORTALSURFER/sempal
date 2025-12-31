use super::audio_samples::{crop_samples, decode_samples_from_bytes, write_wav, DecodedSamples};
use super::EguiController;
use super::MIN_SELECTION_WIDTH;
use crate::analysis::audio::{detect_non_silent_ranges, downmix_to_mono_into};
use crate::selection::SelectionRange;
use crate::sample_sources::SampleSource;
use std::cmp::Ordering;
use std::path::{Path, PathBuf};

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
        for (start, end) in detect_non_silent_ranges(&mono, decoded.sample_rate) {
            let start_norm = start as f32 / total_frames as f32;
            let end_norm = end as f32 / total_frames as f32;
            if use_transients {
                append_slices_from_transients(
                    &mut slices,
                    start_norm,
                    end_norm,
                    &transients,
                );
            } else {
                let range = SelectionRange::new(start_norm, end_norm);
                if range.width() >= MIN_SELECTION_WIDTH {
                    slices.push(range);
                }
            }
        }
        self.ui.waveform.slices = slices;
        Ok(self.ui.waveform.slices.len())
    }

    /// Clear any detected slice ranges from the waveform view.
    pub(crate) fn clear_waveform_slices(&mut self) {
        self.ui.waveform.slices.clear();
    }

    /// Export detected slices to new audio files and register them in the browser.
    pub(super) fn accept_waveform_slices(&mut self) -> Result<usize, String> {
        if self.ui.waveform.slices.is_empty() {
            return Err("No slices to export".into());
        }
        let (source, relative_path, decoded) = self.slice_export_context()?;
        let mut counter = 1usize;
        let exported =
            self.export_slice_batch(&source, &relative_path, &decoded, &mut counter)?;
        self.ui.waveform.slices.clear();
        Ok(exported)
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
        write_wav(
            &target_abs,
            &samples,
            decoded.sample_rate,
            decoded.channels,
        )?;
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
    start: f32,
    end: f32,
    transients: &[f32],
) {
    let range = SelectionRange::new(start, end);
    if range.width() < MIN_SELECTION_WIDTH {
        return;
    }
    if transients.is_empty() {
        slices.push(range);
        return;
    }
    let mut points = Vec::new();
    points.push(range.start());
    points.extend(
        transients
            .iter()
            .copied()
            .filter(|marker| *marker > range.start() && *marker < range.end()),
    );
    points.push(range.end());
    if points.len() < 2 {
        slices.push(range);
        return;
    }
    points.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    for pair in points.windows(2) {
        let slice = SelectionRange::new(pair[0], pair[1]);
        if slice.width() >= MIN_SELECTION_WIDTH {
            slices.push(slice);
        }
    }
}

#[cfg(test)]
mod slices_tests;
