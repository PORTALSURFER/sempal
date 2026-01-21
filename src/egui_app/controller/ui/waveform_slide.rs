use crate::egui_app::controller::library::wav_io::{
    file_metadata, read_samples_for_normalization,
};
use super::*;
use crate::waveform::DecodedWaveform;
use hound::SampleFormat;
use std::sync::Arc;

impl EguiController {
    pub(crate) fn align_waveform_start_to_last_marker(&mut self) -> Result<(), String> {
        if self.is_waveform_circular_slide_active() {
            return Err("Finish the current waveform slide first".to_string());
        }
        let marker = match (
            self.ui.waveform.cursor,
            self.ui.waveform.cursor_last_hover_at,
            self.ui.waveform.cursor_last_navigation_at,
        ) {
            (Some(cursor), Some(hover), Some(nav)) if hover >= nav => Some(cursor),
            (Some(cursor), Some(_), None) => Some(cursor),
            _ => None,
        }
        .ok_or_else(|| "Hover over the waveform to set a start position first".to_string())?
        .clamp(0.0, 1.0);
        if !marker.is_finite() {
            return Err("Start marker is invalid".to_string());
        }
        if marker <= 0.0 {
            self.set_status("Start already aligned", StatusTone::Info);
            return Ok(());
        }
        self.start_waveform_circular_slide(marker)?;
        self.update_waveform_circular_slide(0.0);
        self.finish_waveform_circular_slide()?;
        self.ui.waveform.last_start_marker = Some(0.0);
        Ok(())
    }

    pub(crate) fn start_waveform_circular_slide(&mut self, position: f32) -> Result<(), String> {
        if self.sample_view.waveform_slide.is_some() {
            return Ok(());
        }
        let target = self.waveform_slide_target()?;
        let preview = self
            .ui
            .waveform
            .bpm_stretch_enabled
            .then(|| self.sample_view.waveform.decoded.as_ref())
            .flatten()
            .filter(|decoded| !decoded.samples.is_empty())
            .map(|decoded| WaveformSlidePreview {
                samples: decoded.samples.as_ref().to_vec(),
                channels: decoded.channels,
                sample_rate: decoded.sample_rate,
            });
        let (samples, spec): (Vec<f32>, _) = read_samples_for_normalization(&target.absolute_path)?;
        if samples.is_empty() {
            return Err("No audio data available".into());
        }
        let channels = spec.channels.max(1) as usize;
        let total_frames = samples.len() / channels;
        if total_frames == 0 {
            return Err("No audio frames available".into());
        }
        self.stop_playback_if_active();
        self.sample_view.waveform_slide = Some(WaveformSlideState {
            source: target.source,
            relative_path: target.relative_path,
            absolute_path: target.absolute_path,
            original_samples: samples,
            preview,
            channels,
            spec_channels: spec.channels.max(1),
            sample_rate: spec.sample_rate.max(1),
            start_normalized: position.clamp(0.0, 1.0),
            last_offset_frames: 0,
            last_preview_offset_frames: 0,
        });
        Ok(())
    }

    pub(crate) fn update_waveform_circular_slide(&mut self, position: f32) {
        let Some((rotated, spec_channels, sample_rate)) =
            self.sample_view.waveform_slide.as_mut().and_then(|state| {
                let (preview_samples, preview_channels, spec_channels, sample_rate) =
                    match state.preview.as_ref() {
                    Some(preview) => (
                        preview.samples.as_slice(),
                        preview.channels.max(1) as usize,
                        preview.channels.max(1),
                        preview.sample_rate.max(1),
                    ),
                    None => (
                        state.original_samples.as_slice(),
                        state.channels.max(1),
                        state.spec_channels,
                        state.sample_rate,
                    ),
                };
                let preview_total_frames = preview_samples.len() / preview_channels.max(1);
                let original_total_frames = state.original_samples.len() / state.channels.max(1);
                if preview_total_frames == 0 || original_total_frames == 0 {
                    return None;
                }
                let delta = position - state.start_normalized;
                let preview_offset_frames =
                    (delta * preview_total_frames as f32).round() as isize;
                let original_offset_frames =
                    (delta * original_total_frames as f32).round() as isize;
                if preview_offset_frames == state.last_preview_offset_frames
                    && original_offset_frames == state.last_offset_frames
                {
                    return None;
                }
                state.last_preview_offset_frames = preview_offset_frames;
                state.last_offset_frames = original_offset_frames;
                Some((
                    rotate_interleaved_samples(
                        preview_samples,
                        preview_channels,
                        preview_offset_frames,
                    ),
                    spec_channels,
                    sample_rate,
                ))
            })
        else {
            return;
        };
        self.apply_waveform_slide_preview(rotated, spec_channels, sample_rate);
    }

    pub(crate) fn finish_waveform_circular_slide(&mut self) -> Result<(), String> {
        let Some(state) = self.sample_view.waveform_slide.take() else {
            return Ok(());
        };
        let offset_frames = state.last_offset_frames;
        if offset_frames == 0 {
            self.apply_waveform_slide_preview(
                state.original_samples.clone(),
                state.spec_channels,
                state.sample_rate,
            );
            return Ok(());
        }
        let rotated =
            rotate_interleaved_samples(&state.original_samples, state.channels, offset_frames);
        let result = self.apply_waveform_slide_to_disk(&state, &rotated);
        if result.is_err() {
            self.apply_waveform_slide_preview(
                state.original_samples.clone(),
                state.spec_channels,
                state.sample_rate,
            );
        }
        result
    }

    pub(crate) fn is_waveform_circular_slide_active(&self) -> bool {
        self.sample_view.waveform_slide.is_some()
    }

    fn waveform_slide_target(&self) -> Result<WaveformSlideTarget, String> {
        let audio = self
            .sample_view
            .wav
            .loaded_audio
            .as_ref()
            .ok_or_else(|| "Load a sample to edit it".to_string())?;
        let source = self
            .library
            .sources
            .iter()
            .find(|s| s.id == audio.source_id)
            .cloned()
            .ok_or_else(|| "Source not available for loaded sample".to_string())?;
        let relative_path = audio.relative_path.clone();
        let absolute_path = source.root.join(&relative_path);
        Ok(WaveformSlideTarget {
            source,
            relative_path,
            absolute_path,
        })
    }

    fn apply_waveform_slide_preview(&mut self, samples: Vec<f32>, channels: u16, sample_rate: u32) {
        let channels = channels.max(1);
        let total_frames = samples.len() / channels as usize;
        if total_frames == 0 {
            return;
        }
        let duration_seconds = total_frames as f32 / sample_rate.max(1) as f32;
        let cache_token = crate::waveform::next_cache_token();
        self.sample_view.waveform.decoded = Some(DecodedWaveform {
            cache_token,
            samples: Arc::from(samples),
            analysis_samples: Arc::from(Vec::new()),
            analysis_sample_rate: 0,
            analysis_stride: 1,
            peaks: None,
            duration_seconds,
            sample_rate: sample_rate.max(1),
            channels,
        });
        self.sample_view.waveform.render_meta = None;
        self.ui.waveform.transient_cache_token = None;
        self.refresh_waveform_image();
    }

    fn apply_waveform_slide_to_disk(
        &mut self,
        state: &WaveformSlideState,
        rotated: &[f32],
    ) -> Result<(), String> {
        let backup = undo::OverwriteBackup::capture_before(&state.absolute_path)?;
        let spec = hound::WavSpec {
            channels: state.spec_channels,
            sample_rate: state.sample_rate.max(1),
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };
        write_waveform_wav(&state.absolute_path, rotated, spec)?;
        backup.capture_after(&state.absolute_path)?;
        let (file_size, modified_ns) = file_metadata(&state.absolute_path)?;
        let tag = self.sample_tag_for(&state.source, &state.relative_path)?;
        let db = self
            .database_for(&state.source)
            .map_err(|err| format!("Database unavailable: {err}"))?;
        db.upsert_file(&state.relative_path, file_size, modified_ns)
            .map_err(|err| format!("Failed to sync database entry: {err}"))?;
        db.set_tag(&state.relative_path, tag)
            .map_err(|err| format!("Failed to sync tag: {err}"))?;
        let (last_played_at, looped) = self
            .wav_index_for_path(&state.relative_path)
            .and_then(|idx| self.wav_entry(idx))
            .map(|entry| (entry.last_played_at, entry.looped))
            .unwrap_or((None, false));
        let entry = WavEntry {
            relative_path: state.relative_path.clone(),
            file_size,
            modified_ns,
            content_hash: None,
            tag,
            looped,
            missing: false,
            last_played_at,
        };
        self.update_cached_entry(&state.source, &state.relative_path, entry);
        self.refresh_waveform_for_sample(&state.source, &state.relative_path);
        self.push_undo_entry(self.selection_edit_undo_entry(
            format!("Circular slide {}", state.relative_path.display()),
            state.source.id.clone(),
            state.relative_path.clone(),
            state.absolute_path.clone(),
            backup,
        ));
        self.set_status(
            format!("Slid sample {}", state.relative_path.display()),
            StatusTone::Info,
        );
        Ok(())
    }
}

struct WaveformSlideTarget {
    source: SampleSource,
    relative_path: PathBuf,
    absolute_path: PathBuf,
}

fn rotate_interleaved_samples(samples: &[f32], channels: usize, offset_frames: isize) -> Vec<f32> {
    if samples.is_empty() || channels == 0 {
        return Vec::new();
    }
    let total_frames = samples.len() / channels;
    if total_frames == 0 {
        return Vec::new();
    }
    let offset = offset_frames.rem_euclid(total_frames as isize) as usize;
    if offset == 0 {
        return samples.to_vec();
    }
    let mut rotated = vec![0.0; samples.len()];
    for frame in 0..total_frames {
        let dest_frame = (frame + offset) % total_frames;
        let src = frame * channels;
        let dest = dest_frame * channels;
        rotated[dest..dest + channels].copy_from_slice(&samples[src..src + channels]);
    }
    rotated
}

fn write_waveform_wav(
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
mod tests {
    use super::rotate_interleaved_samples;

    #[test]
    fn rotate_interleaved_samples_wraps_frames() {
        let samples = vec![1.0, -1.0, 2.0, -2.0, 3.0, -3.0];
        let rotated = rotate_interleaved_samples(&samples, 2, 1);
        assert_eq!(rotated, vec![3.0, -3.0, 1.0, -1.0, 2.0, -2.0]);
        let rotated_back = rotate_interleaved_samples(&samples, 2, -1);
        assert_eq!(rotated_back, vec![2.0, -2.0, 3.0, -3.0, 1.0, -1.0]);
    }
}
