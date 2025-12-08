use super::collection_items_helpers::{file_metadata, read_samples_for_normalization};
use super::*;
use hound::SampleFormat;
use std::path::PathBuf;

/// Direction of a fade applied over the active selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FadeDirection {
    /// Fade from full level at the left edge to silence at the right edge.
    LeftToRight,
    /// Fade from silence at the left edge to full level at the right edge.
    RightToLeft,
}

impl EguiController {
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

    /// Fade the selection toward silence from left to right.
    pub(crate) fn fade_waveform_selection_left_to_right(&mut self) -> Result<(), String> {
        self.fade_waveform_selection(FadeDirection::LeftToRight)
    }

    /// Fade the selection toward silence from right to left.
    pub(crate) fn fade_waveform_selection_right_to_left(&mut self) -> Result<(), String> {
        self.fade_waveform_selection(FadeDirection::RightToLeft)
    }

    /// Silence the selected span with short fades at both edges to avoid clicks.
    pub(crate) fn mute_waveform_selection(&mut self) -> Result<(), String> {
        let result = self.apply_selection_edit("Muted selection", |buffer| mute_buffer(buffer, 0.005));
        if let Err(err) = &result {
            self.set_status(err.clone(), StatusTone::Error);
        }
        result
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
    absolute_path: &PathBuf,
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
    let mut trimmed =
        Vec::with_capacity(buffer.samples.len().saturating_sub(suffix_start - prefix_end));
    trimmed.extend_from_slice(&buffer.samples[..prefix_end]);
    trimmed.extend_from_slice(&buffer.samples[suffix_start..]);
    if trimmed.is_empty() {
        return Err("Trim removed all audio; crop instead".into());
    }
    buffer.samples = trimmed;
    Ok(())
}

fn mute_buffer(buffer: &mut SelectionEditBuffer, fade_seconds: f32) -> Result<(), String> {
    apply_muted_selection(
        &mut buffer.samples,
        buffer.channels,
        buffer.start_frame,
        buffer.end_frame,
        buffer.sample_rate,
        fade_seconds,
    );
    Ok(())
}

fn slice_frames(samples: &[f32], channels: usize, start_frame: usize, end_frame: usize) -> Vec<f32> {
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
    if end_frame <= start_frame {
        return;
    }
    let frame_count = end_frame - start_frame;
    let denom = (frame_count.saturating_sub(1)).max(1) as f32;
    for i in 0..frame_count {
        let progress = i as f32 / denom;
        let factor = match direction {
            FadeDirection::LeftToRight => 1.0 - progress,
            FadeDirection::RightToLeft => progress,
        }
        .clamp(0.0, 1.0);
        let frame = start_frame + i;
        for ch in 0..channels {
            let idx = frame * channels + ch;
            if let Some(sample) = samples.get_mut(idx) {
                *sample *= factor;
            }
        }
    }
}

fn apply_muted_selection(
    samples: &mut [f32],
    channels: usize,
    start_frame: usize,
    end_frame: usize,
    sample_rate: u32,
    fade_seconds: f32,
) {
    if end_frame <= start_frame {
        return;
    }
    let frame_count = end_frame - start_frame;
    let fade_frames = (sample_rate.max(1) as f32 * fade_seconds)
        .round()
        .clamp(1.0, frame_count as f32) as usize;
    let fade_len = fade_frames
        .min((frame_count / 2).max(1))
        .max(1);
    let tail_start = start_frame + frame_count - fade_len;
    let denom = fade_len as f32;
    for i in 0..frame_count {
        let frame = start_frame + i;
        let factor = if i < fade_len {
            1.0 - ((i as f32 + 1.0) / denom)
        } else if frame >= tail_start {
            let tail = frame - tail_start;
            (tail as f32 + 1.0) / denom
        } else {
            0.0
        }
        .clamp(0.0, 1.0);
        for ch in 0..channels {
            let idx = frame * channels + ch;
            if let Some(sample) = samples.get_mut(idx) {
                *sample *= factor;
            }
        }
    }
}

fn write_selection_wav(target: &PathBuf, samples: &[f32], spec: hound::WavSpec) -> Result<(), String> {
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
mod tests {
    use super::*;

    #[test]
    fn slice_frames_keeps_requested_range() {
        let samples = vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6];
        let sliced = slice_frames(&samples, 2, 1, 3);
        assert_eq!(sliced, vec![0.3, 0.4, 0.5, 0.6]);
    }

    #[test]
    fn trim_removes_target_span() {
        let mut buffer = SelectionEditBuffer {
            samples: vec![1.0_f32; 8],
            channels: 1,
            sample_rate: 48_000,
            spec_channels: 1,
            start_frame: 2,
            end_frame: 6,
        };
        trim_buffer(&mut buffer).unwrap();
        assert_eq!(buffer.samples.len(), 4);
    }

    #[test]
    fn directional_fade_zeroes_expected_side() {
        let mut samples = vec![1.0_f32; 6];
        apply_directional_fade(&mut samples, 1, 0, 6, FadeDirection::LeftToRight);
        assert!(samples[5].abs() < 1e-6);
        let mut samples = vec![1.0_f32; 6];
        apply_directional_fade(&mut samples, 1, 0, 6, FadeDirection::RightToLeft);
        assert!(samples[0].abs() < 1e-6);
    }

    #[test]
    fn mute_applies_fades() {
        let mut samples = vec![1.0_f32; 10];
        apply_muted_selection(&mut samples, 1, 0, 10, 1000, 0.005);
        assert!(samples[0] < 1.0);
        assert!(samples[9] > 0.0);
        assert!(samples[4].abs() < 1e-6);
        assert!(samples[5] < 0.3);
    }

    #[test]
    fn crop_keeps_only_selection_frames() {
        let mut buffer = SelectionEditBuffer {
            samples: vec![0.0, 1.0, 2.0, 3.0],
            channels: 1,
            sample_rate: 44_100,
            spec_channels: 1,
            start_frame: 1,
            end_frame: 3,
        };
        crop_buffer(&mut buffer).unwrap();
        assert_eq!(buffer.samples, vec![1.0, 2.0]);
    }
}
