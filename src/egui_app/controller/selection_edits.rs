use super::collection_items_helpers::{file_metadata, read_samples_for_normalization};
use super::*;
use crate::egui_app::state::{DestructiveEditPrompt, DestructiveSelectionEdit};
use hound::SampleFormat;
use std::{path::PathBuf, time::Duration};

#[path = "selection_normalize.rs"]
mod selection_normalize;
#[path = "selection_smooth.rs"]
mod selection_smooth;

use selection_normalize::normalize_selection;
use selection_smooth::smooth_selection;

/// Direction of a fade applied over the active selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FadeDirection {
    /// Fade from full level at the left edge to silence at the right edge.
    LeftToRight,
    /// Fade from silence at the left edge to full level at the right edge.
    RightToLeft,
}

/// Result of a destructive edit request.
pub(crate) enum SelectionEditRequest {
    Applied,
    Prompted,
}

impl DestructiveSelectionEdit {
    fn title(&self) -> &'static str {
        match self {
            DestructiveSelectionEdit::CropSelection => "Crop selection",
            DestructiveSelectionEdit::TrimSelection => "Trim selection",
            DestructiveSelectionEdit::FadeLeftToRight => "Fade selection (left to right)",
            DestructiveSelectionEdit::FadeRightToLeft => "Fade selection (right to left)",
            DestructiveSelectionEdit::MuteSelection => "Mute selection",
            DestructiveSelectionEdit::NormalizeSelection => "Normalize selection",
            DestructiveSelectionEdit::SmoothSelection => "Smooth selection edges",
        }
    }

    fn warning(&self) -> &'static str {
        match self {
            DestructiveSelectionEdit::CropSelection => {
                "This will overwrite the file with only the selected region."
            }
            DestructiveSelectionEdit::TrimSelection => {
                "This will remove the selected region and close the gap in the source file."
            }
            DestructiveSelectionEdit::FadeLeftToRight => {
                "This will overwrite the selection with a fade down to silence."
            }
            DestructiveSelectionEdit::FadeRightToLeft => {
                "This will overwrite the selection with a fade up from silence."
            }
            DestructiveSelectionEdit::MuteSelection => {
                "This will overwrite the selection with silence."
            }
            DestructiveSelectionEdit::NormalizeSelection => {
                "This will overwrite the selection with a normalized version and short fades."
            }
            DestructiveSelectionEdit::SmoothSelection => {
                "This will overwrite the selection with softened edges to reduce clicks."
            }
        }
    }
}

impl EguiController {
    /// Request a destructive edit, showing a confirmation unless yolo mode is enabled.
    pub(crate) fn request_destructive_selection_edit(
        &mut self,
        edit: DestructiveSelectionEdit,
    ) -> Result<SelectionEditRequest, String> {
        if let Err(err) = self.selection_target() {
            self.set_status(err.clone(), StatusTone::Error);
            return Err(err);
        }
        if self.controls.destructive_yolo_mode {
            self.ui.waveform.pending_destructive = None;
            self.apply_selection_edit_kind(edit)?;
            return Ok(SelectionEditRequest::Applied);
        }
        self.ui.waveform.pending_destructive = Some(prompt_for_edit(edit));
        Ok(SelectionEditRequest::Prompted)
    }

    /// Apply the pending destructive edit after user confirmation.
    pub(crate) fn apply_confirmed_destructive_edit(&mut self, edit: DestructiveSelectionEdit) {
        self.ui.waveform.pending_destructive = None;
        let _ = self.apply_selection_edit_kind(edit);
    }

    /// Clear any pending destructive edit prompt without applying it.
    pub(crate) fn clear_destructive_prompt(&mut self) {
        self.ui.waveform.pending_destructive = None;
    }

    /// Crop the loaded sample to the active selection range and refresh caches/exports.
    pub(crate) fn crop_waveform_selection(&mut self) -> Result<(), String> {
        let result = self.apply_selection_edit("Cropped selection", crop_buffer);
        if let Err(err) = &result {
            self.set_status(err.clone(), StatusTone::Error);
        }
        result
    }

    /// Remove the selected span from the loaded sample.
    pub(crate) fn trim_waveform_selection(&mut self) -> Result<(), String> {
        let result = self.apply_selection_edit("Trimmed selection", trim_buffer);
        if let Err(err) = &result {
            self.set_status(err.clone(), StatusTone::Error);
        }
        result
    }

    /// Fade the selected span down to silence using the given direction.
    pub(crate) fn fade_waveform_selection(
        &mut self,
        direction: FadeDirection,
    ) -> Result<(), String> {
        let result = self.apply_selection_edit("Applied fade", |buffer| {
            apply_directional_fade(
                &mut buffer.samples,
                buffer.channels,
                buffer.start_frame,
                buffer.end_frame,
                direction,
            );
            Ok(())
        });
        if let Err(err) = &result {
            self.set_status(err.clone(), StatusTone::Error);
        }
        result
    }

    /// Normalize the active selection and apply short fades at the edges.
    pub(crate) fn normalize_waveform_selection(&mut self) -> Result<(), String> {
        let result = self.apply_selection_edit("Normalized selection", |buffer| {
            normalize_selection(buffer, Duration::from_millis(5))
        });
        if let Err(err) = &result {
            self.set_status(err.clone(), StatusTone::Error);
        }
        result
    }

    /// Smooth the selection edges with short raised-cosine crossfades.
    pub(crate) fn smooth_waveform_selection(&mut self) -> Result<(), String> {
        let result = self.apply_selection_edit("Smoothed selection", |buffer| {
            smooth_selection(buffer, Duration::from_millis(8))
        });
        if let Err(err) = &result {
            self.set_status(err.clone(), StatusTone::Error);
        }
        result
    }

    /// Silence the selected span without applying fades.
    pub(crate) fn mute_waveform_selection(&mut self) -> Result<(), String> {
        let result = self.apply_selection_edit("Muted selection", mute_buffer);
        if let Err(err) = &result {
            self.set_status(err.clone(), StatusTone::Error);
        }
        result
    }

    fn apply_selection_edit_kind(&mut self, edit: DestructiveSelectionEdit) -> Result<(), String> {
        match edit {
            DestructiveSelectionEdit::CropSelection => self.crop_waveform_selection(),
            DestructiveSelectionEdit::TrimSelection => self.trim_waveform_selection(),
            DestructiveSelectionEdit::FadeLeftToRight => {
                self.fade_waveform_selection(FadeDirection::LeftToRight)
            }
            DestructiveSelectionEdit::FadeRightToLeft => {
                self.fade_waveform_selection(FadeDirection::RightToLeft)
            }
            DestructiveSelectionEdit::MuteSelection => self.mute_waveform_selection(),
            DestructiveSelectionEdit::NormalizeSelection => self.normalize_waveform_selection(),
            DestructiveSelectionEdit::SmoothSelection => self.smooth_waveform_selection(),
        }
    }

    fn apply_selection_edit<F>(&mut self, action_label: &str, mut edit: F) -> Result<(), String>
    where
        F: FnMut(&mut SelectionEditBuffer) -> Result<(), String>,
    {
        let context = self.selection_target()?;
        let mut buffer = load_selection_buffer(&context.absolute_path, context.selection)?;
        edit(&mut buffer)?;
        if buffer.samples.is_empty() {
            return Err("No audio data after edit".into());
        }
        let spec = hound::WavSpec {
            channels: buffer.spec_channels,
            sample_rate: buffer.sample_rate.max(1),
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };
        write_selection_wav(&context.absolute_path, &buffer.samples, spec)?;
        let (file_size, modified_ns) = file_metadata(&context.absolute_path)?;
        let tag = self.sample_tag_for(&context.source, &context.relative_path)?;
        let entry = WavEntry {
            relative_path: context.relative_path.clone(),
            file_size,
            modified_ns,
            tag,
            missing: false,
        };
        self.update_cached_entry(&context.source, &context.relative_path, entry);
        self.refresh_waveform_for_sample(&context.source, &context.relative_path);
        self.reexport_collections_for_sample(&context.source.id, &context.relative_path);
        self.set_status(
            format!("{} {}", action_label, context.relative_path.display()),
            StatusTone::Info,
        );
        Ok(())
    }

    fn selection_target(&self) -> Result<SelectionTarget, String> {
        let selection = self
            .ui
            .waveform
            .selection
            .ok_or_else(|| "Make a selection first".to_string())?;
        if selection.width() <= 0.0 {
            return Err("Selection is empty".into());
        }
        let audio = self
            .loaded_audio
            .as_ref()
            .ok_or_else(|| "Load a sample to edit it".to_string())?;
        let source = self
            .sources
            .iter()
            .find(|s| s.id == audio.source_id)
            .cloned()
            .ok_or_else(|| "Source not available for loaded sample".to_string())?;
        let relative_path = audio.relative_path.clone();
        let absolute_path = source.root.join(&relative_path);
        Ok(SelectionTarget {
            source,
            relative_path,
            absolute_path,
            selection,
        })
    }
}

struct SelectionTarget {
    source: SampleSource,
    relative_path: PathBuf,
    absolute_path: PathBuf,
    selection: SelectionRange,
}

#[derive(Clone)]
struct SelectionEditBuffer {
    samples: Vec<f32>,
    channels: usize,
    sample_rate: u32,
    spec_channels: u16,
    start_frame: usize,
    end_frame: usize,
}

fn load_selection_buffer(
    absolute_path: &Path,
    selection: SelectionRange,
) -> Result<SelectionEditBuffer, String> {
    let (samples, spec) = read_samples_for_normalization(absolute_path)?;
    let channels = spec.channels.max(1) as usize;
    if samples.is_empty() {
        return Err("No audio data available".into());
    }
    let total_frames = samples.len() / channels;
    let (start_frame, end_frame) = selection_frame_bounds(total_frames, selection);
    Ok(SelectionEditBuffer {
        samples,
        channels,
        sample_rate: spec.sample_rate.max(1),
        spec_channels: spec.channels.max(1),
        start_frame,
        end_frame,
    })
}

fn selection_frame_bounds(total_frames: usize, bounds: SelectionRange) -> (usize, usize) {
    let start_frame = ((bounds.start() * total_frames as f32).floor() as usize)
        .min(total_frames.saturating_sub(1));
    let mut end_frame = ((bounds.end() * total_frames as f32).ceil() as usize).min(total_frames);
    if end_frame <= start_frame {
        end_frame = (start_frame + 1).min(total_frames);
    }
    (start_frame, end_frame)
}

fn crop_buffer(buffer: &mut SelectionEditBuffer) -> Result<(), String> {
    let cropped = slice_frames(
        &buffer.samples,
        buffer.channels,
        buffer.start_frame,
        buffer.end_frame,
    );
    if cropped.is_empty() {
        return Err("Selection has no audio to crop".into());
    }
    buffer.samples = cropped;
    Ok(())
}

fn trim_buffer(buffer: &mut SelectionEditBuffer) -> Result<(), String> {
    let total_frames = buffer.samples.len() / buffer.channels;
    if buffer.start_frame == 0 && buffer.end_frame >= total_frames {
        return Err("Cannot trim the entire file; crop instead".into());
    }
    let prefix_end = buffer.start_frame * buffer.channels;
    let suffix_start = buffer.end_frame * buffer.channels;
    let mut trimmed = Vec::with_capacity(
        buffer
            .samples
            .len()
            .saturating_sub(suffix_start - prefix_end),
    );
    trimmed.extend_from_slice(&buffer.samples[..prefix_end]);
    trimmed.extend_from_slice(&buffer.samples[suffix_start..]);
    if trimmed.is_empty() {
        return Err("Trim removed all audio; crop instead".into());
    }
    buffer.samples = trimmed;
    Ok(())
}

fn mute_buffer(buffer: &mut SelectionEditBuffer) -> Result<(), String> {
    apply_muted_selection(
        &mut buffer.samples,
        buffer.channels,
        buffer.start_frame,
        buffer.end_frame,
    );
    Ok(())
}

fn slice_frames(
    samples: &[f32],
    channels: usize,
    start_frame: usize,
    end_frame: usize,
) -> Vec<f32> {
    let mut cropped = Vec::with_capacity((end_frame - start_frame) * channels);
    for frame in start_frame..end_frame {
        let offset = frame * channels;
        cropped.extend_from_slice(&samples[offset..offset + channels]);
    }
    cropped
}

fn apply_directional_fade(
    samples: &mut [f32],
    channels: usize,
    start_frame: usize,
    end_frame: usize,
    direction: FadeDirection,
) {
    let channels = channels.max(1);
    let total_frames = samples.len() / channels;
    let (clamped_start, clamped_end) = clamped_selection_span(total_frames, start_frame, end_frame);
    if clamped_end <= clamped_start {
        return;
    }
    apply_fade_ramp(samples, channels, clamped_start, clamped_end, direction);
    match direction {
        FadeDirection::LeftToRight => {
            apply_muted_selection(samples, channels, clamped_end, total_frames);
        }
        FadeDirection::RightToLeft => {
            apply_muted_selection(samples, channels, 0, clamped_start);
        }
    }
}

fn clamped_selection_span(
    total_frames: usize,
    start_frame: usize,
    end_frame: usize,
) -> (usize, usize) {
    let clamped_start = start_frame.min(total_frames);
    let clamped_end = end_frame.min(total_frames);
    (clamped_start, clamped_end)
}

fn apply_fade_ramp(
    samples: &mut [f32],
    channels: usize,
    clamped_start: usize,
    clamped_end: usize,
    direction: FadeDirection,
) {
    let frame_count = clamped_end - clamped_start;
    let denom = (frame_count.saturating_sub(1)).max(1) as f32;
    for i in 0..frame_count {
        let progress = i as f32 / denom;
        let factor = fade_factor(frame_count, progress, direction);
        let frame = clamped_start + i;
        for ch in 0..channels {
            let idx = frame * channels + ch;
            if let Some(sample) = samples.get_mut(idx) {
                *sample *= factor;
            }
        }
    }
}

fn fade_factor(frame_count: usize, progress: f32, direction: FadeDirection) -> f32 {
    if frame_count == 1 {
        return 0.0;
    }
    let factor = match direction {
        FadeDirection::LeftToRight => 1.0 - progress,
        FadeDirection::RightToLeft => progress,
    };
    factor.clamp(0.0, 1.0)
}

fn apply_muted_selection(
    samples: &mut [f32],
    channels: usize,
    start_frame: usize,
    end_frame: usize,
) {
    if end_frame <= start_frame {
        return;
    }
    let channels = channels.max(1);
    let total_frames = samples.len() / channels;
    let clamped_start = start_frame.min(total_frames);
    let clamped_end = end_frame.min(total_frames);
    for frame in clamped_start..clamped_end {
        let offset = frame * channels;
        let frame_end = (offset + channels).min(samples.len());
        for sample in &mut samples[offset..frame_end] {
            *sample = 0.0;
        }
    }
}

fn write_selection_wav(
    target: &PathBuf,
    samples: &[f32],
    spec: hound::WavSpec,
) -> Result<(), String> {
    let mut writer = hound::WavWriter::create(target, spec)
        .map_err(|err| format!("Failed to write wav: {err}"))?;
    for sample in samples {
        writer
            .write_sample(*sample)
            .map_err(|err| format!("Failed to write sample: {err}"))?;
    }
    writer
        .finalize()
        .map_err(|err| format!("Failed to finalize wav: {err}"))
}

#[cfg(test)]
#[path = "selection_edits_tests.rs"]
mod selection_edits_tests;

fn prompt_for_edit(edit: DestructiveSelectionEdit) -> DestructiveEditPrompt {
    DestructiveEditPrompt {
        edit,
        title: edit.title().to_string(),
        message: edit.warning().to_string(),
    }
}
