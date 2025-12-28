use super::collection_items_helpers::{file_metadata, read_samples_for_normalization};
use super::undo;
use super::*;
use crate::egui_app::state::{LoopCrossfadePrompt, LoopCrossfadeSettings, LoopCrossfadeUnit};
use hound::SampleFormat;
use std::path::{Path, PathBuf};

impl EguiController {
    /// Open the loop crossfade prompt for a visible browser row.
    pub fn request_loop_crossfade_prompt_for_browser_row(
        &mut self,
        row: usize,
    ) -> Result<(), String> {
        let ctx = self.resolve_browser_sample(row)?;
        self.ui.loop_crossfade_prompt = Some(LoopCrossfadePrompt {
            source_id: ctx.source.id,
            relative_path: ctx.entry.relative_path,
            settings: LoopCrossfadeSettings::default(),
        });
        Ok(())
    }

    /// Apply the pending loop crossfade prompt.
    pub fn apply_loop_crossfade_prompt(&mut self) -> Result<(), String> {
        let Some(prompt) = self.ui.loop_crossfade_prompt.clone() else {
            return Ok(());
        };
        self.ui.loop_crossfade_prompt = None;
        let source = loop_crossfade_source(self, &prompt.source_id)?;
        let absolute_path = source.root.join(&prompt.relative_path);
        let new_relative = self.apply_loop_crossfade_for_sample(
            &source,
            &prompt.relative_path,
            &absolute_path,
            &prompt.settings,
        )?;
        self.select_from_browser(&new_relative);
        Ok(())
    }

    /// Clear any pending loop crossfade prompt.
    pub fn clear_loop_crossfade_prompt(&mut self) {
        self.ui.loop_crossfade_prompt = None;
    }

    /// Apply a loop crossfade copy for a single sample path.
    pub(in crate::egui_app::controller) fn apply_loop_crossfade_for_sample(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
        absolute_path: &Path,
        settings: &LoopCrossfadeSettings,
    ) -> Result<PathBuf, String> {
        let (mut samples, spec) = read_samples_for_normalization(absolute_path)?;
        let (channels, total_frames) = loop_crossfade_layout(&samples, spec.channels)?;
        let fade_frames =
            loop_crossfade_frames(settings, spec.sample_rate.max(1), total_frames)?;
        apply_loop_crossfade(&mut samples, channels, total_frames, fade_frames)?;
        let suffix = loop_crossfade_suffix(settings);
        let new_relative =
            next_loop_crossfade_relative_path(relative_path, &source.root, &suffix);
        let new_absolute = source.root.join(&new_relative);
        let tag = self.sample_tag_for(source, relative_path)?;
        write_loop_crossfade_wav(&new_absolute, &samples, loop_crossfade_spec(&spec))?;
        register_loop_crossfade_entry(self, source, &new_relative, &new_absolute, tag)?;
        maybe_capture_loop_crossfade_undo(
            self,
            source,
            &new_relative,
            &new_absolute,
            tag,
        );
        self.set_status(
            format!("Created loop crossfade {}", new_relative.display()),
            StatusTone::Info,
        );
        Ok(new_relative)
    }
}

fn loop_crossfade_source(
    controller: &EguiController,
    source_id: &SourceId,
) -> Result<SampleSource, String> {
    controller
        .library
        .sources
        .iter()
        .find(|s| &s.id == source_id)
        .cloned()
        .ok_or_else(|| "Source not available".to_string())
}

fn loop_crossfade_frames(
    settings: &LoopCrossfadeSettings,
    sample_rate: u32,
    total_frames: usize,
) -> Result<usize, String> {
    let frames = match settings.unit {
        LoopCrossfadeUnit::Milliseconds => {
            let ms = settings.depth_ms.max(1) as f32;
            ((sample_rate as f32 * ms / 1000.0).round() as usize).max(1)
        }
        LoopCrossfadeUnit::Samples => settings.depth_samples.max(1) as usize,
    };
    let max_frames = total_frames / 2;
    if max_frames == 0 {
        return Err("Sample is too short for a loop crossfade".into());
    }
    Ok(frames.min(max_frames))
}

fn loop_crossfade_layout(samples: &[f32], channels: u16) -> Result<(usize, usize), String> {
    if samples.is_empty() {
        return Err("No audio data to crossfade".into());
    }
    let channels = channels.max(1) as usize;
    let total_frames = samples.len() / channels;
    if total_frames < 2 {
        return Err("Sample is too short to crossfade".into());
    }
    Ok((channels, total_frames))
}

fn apply_loop_crossfade(
    samples: &mut [f32],
    channels: usize,
    total_frames: usize,
    fade_frames: usize,
) -> Result<(), String> {
    let fade_frames = fade_frames.min(total_frames / 2);
    if fade_frames == 0 {
        return Err("Crossfade depth is too short for this sample".into());
    }
    let channels = channels.max(1);
    let cut_frame = find_crossfade_cut_frame(samples, channels, total_frames, fade_frames);
    let mut output = vec![0.0; samples.len()];
    for frame in 0..total_frames {
        let src_frame = (cut_frame + frame) % total_frames;
        for ch in 0..channels {
            let out_idx = frame * channels + ch;
            let src_idx = src_frame * channels + ch;
            output[out_idx] = samples[src_idx];
        }
    }
    let denom = (fade_frames.saturating_sub(1)).max(1) as f32;
    for frame in 0..fade_frames {
        let progress = if fade_frames == 1 {
            0.5
        } else {
            frame as f32 / denom
        };
        let (from_gain, to_gain) = equal_power_gains(progress);
        for ch in 0..channels {
            let tail_idx = (total_frames - fade_frames + frame) * channels + ch;
            let head_idx = frame * channels + ch;
            let head = output[head_idx];
            let tail = output[tail_idx];
            output[tail_idx] = tail * from_gain + head * to_gain;
        }
    }
    let offset_frames = fade_frames / 2;
    if offset_frames == 0 {
        samples.copy_from_slice(&output);
        return Ok(());
    }
    let mut shifted = vec![0.0; samples.len()];
    for frame in 0..total_frames {
        let src_frame = (offset_frames + frame) % total_frames;
        for ch in 0..channels {
            let out_idx = frame * channels + ch;
            let src_idx = src_frame * channels + ch;
            shifted[out_idx] = output[src_idx];
        }
    }
    samples.copy_from_slice(&shifted);
    Ok(())
}

fn equal_power_gains(progress: f32) -> (f32, f32) {
    let t = progress.clamp(0.0, 1.0);
    let angle = t * std::f32::consts::FRAC_PI_2;
    (angle.cos(), angle.sin())
}

fn find_crossfade_cut_frame(
    samples: &[f32],
    channels: usize,
    total_frames: usize,
    fade_frames: usize,
) -> usize {
    let nominal = total_frames.saturating_sub(fade_frames);
    let search_window = fade_frames.min(1024).min(nominal);
    let min_cut = nominal.saturating_sub(search_window);
    let max_cut = nominal.max(1);
    let mut best_frame = nominal.max(1);
    let mut best_score = f32::INFINITY;
    for frame in min_cut.max(1)..=max_cut {
        let prev = frame - 1;
        let mut score = 0.0;
        for ch in 0..channels {
            let a = samples[prev * channels + ch];
            let b = samples[frame * channels + ch];
            score += (b - a).abs();
        }
        if score < best_score {
            best_score = score;
            best_frame = frame;
        }
    }
    best_frame
}

fn loop_crossfade_suffix(settings: &LoopCrossfadeSettings) -> String {
    match settings.unit {
        LoopCrossfadeUnit::Milliseconds => format!("fade{}ms", settings.depth_ms.max(1)),
        LoopCrossfadeUnit::Samples => format!("fade{}samp", settings.depth_samples.max(1)),
    }
}

fn loop_crossfade_spec(spec: &hound::WavSpec) -> hound::WavSpec {
    hound::WavSpec {
        channels: spec.channels.max(1),
        sample_rate: spec.sample_rate.max(1),
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    }
}

fn next_loop_crossfade_relative_path(
    relative_path: &Path,
    root: &Path,
    suffix: &str,
) -> PathBuf {
    let parent = relative_path.parent().unwrap_or_else(|| Path::new(""));
    let stem = relative_path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("sample");
    let ext = relative_path.extension().and_then(|s| s.to_str());
    let mut counter = 0;
    loop {
        let name = if counter == 0 {
            match ext {
                Some(ext) => format!("{stem}_{suffix}.{ext}"),
                None => format!("{stem}_{suffix}"),
            }
        } else {
            match ext {
                Some(ext) => format!("{stem}_{suffix}_{counter}.{ext}"),
                None => format!("{stem}_{suffix}_{counter}"),
            }
        };
        let candidate = parent.join(name);
        if !root.join(&candidate).exists() {
            return candidate;
        }
        counter += 1;
    }
}

fn write_loop_crossfade_wav(
    path: &Path,
    samples: &[f32],
    spec: hound::WavSpec,
) -> Result<(), String> {
    let mut writer =
        hound::WavWriter::create(path, spec).map_err(|err| format!("Failed to write wav: {err}"))?;
    for sample in samples {
        writer
            .write_sample(*sample)
            .map_err(|err| format!("Failed to write sample: {err}"))?;
    }
    writer
        .finalize()
        .map_err(|err| format!("Failed to finalize wav: {err}"))
}

fn register_loop_crossfade_entry(
    controller: &mut EguiController,
    source: &SampleSource,
    relative_path: &Path,
    absolute_path: &Path,
    tag: SampleTag,
) -> Result<(u64, i64), String> {
    let (file_size, modified_ns) = file_metadata(absolute_path)?;
    let db = controller
        .database_for(source)
        .map_err(|err| format!("Database unavailable: {err}"))?;
    db.upsert_file(relative_path, file_size, modified_ns)
        .map_err(|err| format!("Failed to sync database entry: {err}"))?;
    db.set_tag(relative_path, tag)
        .map_err(|err| format!("Failed to sync tag: {err}"))?;
    controller.insert_cached_entry(
        source,
        WavEntry {
            relative_path: relative_path.to_path_buf(),
            file_size,
            modified_ns,
            content_hash: None,
            tag,
            missing: false,
        },
    );
    controller.enqueue_similarity_for_new_sample(source, relative_path, file_size, modified_ns);
    Ok((file_size, modified_ns))
}

fn maybe_capture_loop_crossfade_undo(
    controller: &mut EguiController,
    source: &SampleSource,
    relative_path: &Path,
    absolute_path: &Path,
    tag: SampleTag,
) {
    let Ok(backup) = undo::OverwriteBackup::capture_before(absolute_path) else {
        return;
    };
    if backup.capture_after(absolute_path).is_ok() {
        controller.push_undo_entry(loop_crossfade_undo_entry(
            format!("Loop crossfaded {}", relative_path.display()),
            source.id.clone(),
            relative_path.to_path_buf(),
            absolute_path.to_path_buf(),
            tag,
            backup,
        ));
    }
}

fn loop_crossfade_undo_entry(
    label: String,
    source_id: SourceId,
    relative_path: PathBuf,
    absolute_path: PathBuf,
    tag: SampleTag,
    backup: undo::OverwriteBackup,
) -> undo::UndoEntry<EguiController> {
    let after = backup.after.clone();
    let backup_dir = backup.dir.clone();
    let undo_source_id = source_id.clone();
    let redo_source_id = source_id;
    let undo_relative = relative_path.clone();
    let redo_relative = relative_path;
    let undo_absolute = absolute_path.clone();
    let redo_absolute = absolute_path;
    undo::UndoEntry::<EguiController>::new(
        label,
        move |controller: &mut EguiController| {
            undo_loop_crossfade(controller, &undo_source_id, &undo_relative, &undo_absolute)
        },
        move |controller: &mut EguiController| {
            redo_loop_crossfade(
                controller,
                &redo_source_id,
                &redo_relative,
                &redo_absolute,
                tag,
                &after,
            )
        },
    )
    .with_cleanup_dir(backup_dir)
}

fn undo_loop_crossfade(
    controller: &mut EguiController,
    source_id: &SourceId,
    relative_path: &Path,
    absolute_path: &Path,
) -> Result<(), String> {
    let source = loop_crossfade_source(controller, source_id)?;
    let db = controller
        .database_for(&source)
        .map_err(|err| format!("Database unavailable: {err}"))?;
    let _ = std::fs::remove_file(absolute_path);
    let _ = db.remove_file(relative_path);
    controller.prune_cached_sample(&source, relative_path);
    Ok(())
}

fn redo_loop_crossfade(
    controller: &mut EguiController,
    source_id: &SourceId,
    relative_path: &Path,
    absolute_path: &Path,
    tag: SampleTag,
    after: &Path,
) -> Result<(), String> {
    let source = loop_crossfade_source(controller, source_id)?;
    let db = controller
        .database_for(&source)
        .map_err(|err| format!("Database unavailable: {err}"))?;
    if let Some(parent) = absolute_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::copy(after, absolute_path)
        .map_err(|err| format!("Failed to restore loop crossfade: {err}"))?;
    let (file_size, modified_ns) = file_metadata(absolute_path)?;
    db.upsert_file(relative_path, file_size, modified_ns)
        .map_err(|err| format!("Failed to sync database entry: {err}"))?;
    db.set_tag(relative_path, tag)
        .map_err(|err| format!("Failed to sync tag: {err}"))?;
    controller.insert_cached_entry(
        &source,
        WavEntry {
            relative_path: relative_path.to_path_buf(),
            file_size,
            modified_ns,
            content_hash: None,
            tag,
            missing: false,
        },
    );
    controller.refresh_waveform_for_sample(&source, relative_path);
    controller.reexport_collections_for_sample(&source.id, relative_path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{apply_loop_crossfade, find_crossfade_cut_frame};

    #[test]
    fn loop_crossfade_finds_low_delta_cut() {
        let samples = vec![0.0, 1.0, 2.0, 2.1, 2.2, 10.0];
        let cut = find_crossfade_cut_frame(&samples, 1, 6, 2);
        assert_eq!(cut, 3);
    }

    #[test]
    fn loop_crossfade_moves_cut_to_front() {
        let mut samples = vec![0.0, 1.0, 2.0, 2.1, 2.2, 10.0];
        apply_loop_crossfade(&mut samples, 1, 6, 2).unwrap();
        let expected = [2.2, 10.0, 0.0, 1.0, 2.2, 2.1];
        for (actual, expected) in samples.iter().zip(expected.iter()) {
            assert!((actual - expected).abs() < 1.0e-6);
        }
    }
}
