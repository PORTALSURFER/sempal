use crate::egui_app::controller::library::collection_items_helpers::read_samples_for_normalization;
use super::super::*;
use std::path::{Path, PathBuf};

pub(crate) struct SelectionTarget {
    pub(crate) source: SampleSource,
    pub(crate) relative_path: PathBuf,
    pub(crate) absolute_path: PathBuf,
    pub(crate) selection: SelectionRange,
}

#[derive(Clone)]
pub(crate) struct SelectionEditBuffer {
    pub(crate) samples: Vec<f32>,
    pub(crate) channels: usize,
    pub(crate) sample_rate: u32,
    pub(crate) spec_channels: u16,
    pub(crate) start_frame: usize,
    pub(crate) end_frame: usize,
}

pub(crate) fn load_selection_buffer(
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

pub(crate) fn selection_frame_bounds(
    total_frames: usize,
    bounds: SelectionRange,
) -> (usize, usize) {
    let start_frame = ((bounds.start() * total_frames as f32).floor() as usize)
        .min(total_frames.saturating_sub(1));
    let mut end_frame = ((bounds.end() * total_frames as f32).ceil() as usize).min(total_frames);
    if end_frame <= start_frame {
        end_frame = (start_frame + 1).min(total_frames);
    }
    (start_frame, end_frame)
}

pub(crate) fn write_selection_wav(
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

pub(crate) fn next_crop_relative_path(
    relative_path: &Path,
    root: &Path,
) -> Result<PathBuf, String> {
    let parent = relative_path.parent().unwrap_or(Path::new(""));
    let stem = relative_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("sample");
    let stem = stem.trim();
    let stem = if stem.is_empty() { "sample" } else { stem };
    let stem = strip_crop_suffix(stem);
    let ext = relative_path.extension().and_then(|e| e.to_str());

    for idx in 1..=999u32 {
        let file_name = match ext {
            Some(ext) if !ext.is_empty() => format!("{stem}_crop{idx:03}.{ext}"),
            _ => format!("{stem}_crop{idx:03}"),
        };
        let candidate = parent.join(file_name);
        if !root.join(&candidate).exists() {
            return Ok(candidate);
        }
    }
    Err("Could not find available crop filename".into())
}

fn strip_crop_suffix(stem: &str) -> &str {
    let Some((prefix, suffix)) = stem.rsplit_once("_crop") else {
        return stem;
    };
    if suffix.len() == 3 && suffix.chars().all(|c| c.is_ascii_digit()) && !prefix.is_empty() {
        prefix
    } else {
        stem
    }
}
