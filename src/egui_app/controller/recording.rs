use super::*;
use crate::audio::{AudioRecorder, RecordingOutcome};
use crate::waveform::{DecodedWaveform, WaveformPeaks};
use super::state::audio::RecordingTarget;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use time::format_description::FormatItem;
use time::macros::format_description;

const RECORDING_FILE_PREFIX: &str = "recording_";
const RECORDING_FILE_EXT: &str = "wav";
const RECORDING_REFRESH_INTERVAL: Duration = Duration::from_millis(60);
const RECORDING_MAX_FULL_FRAMES: usize = 2_500_000;
const RECORDING_MAX_PEAK_BUCKETS: usize = 1_000_000;

impl EguiController {
    pub fn is_recording(&self) -> bool {
        self.audio.recorder.is_some()
    }

    pub fn start_recording(&mut self) -> Result<(), String> {
        self.start_recording_in_current_source()
    }

    pub(super) fn start_recording_in_current_source(&mut self) -> Result<(), String> {
        if self.is_recording() {
            return Ok(());
        }
        if self.is_playing() {
            self.stop_playback_if_active();
        }
        let (source, relative_path, output_path) = self.next_recording_path_in_source()?;
        self.sample_view.wav.selected_wav = Some(relative_path.clone());
        self.audio.recording_target = Some(RecordingTarget {
            source_id: source.id.clone(),
            relative_path,
            absolute_path: output_path.clone(),
            last_refresh_at: None,
            last_file_len: 0,
            loaded_once: false,
        });
        let recorder = AudioRecorder::start(&self.settings.audio_input, output_path.clone())
            .map_err(|err| err.to_string())?;
        self.update_audio_input_status(recorder.resolved());
        self.audio.recorder = Some(recorder);
        self.set_status(
            format!("Recording to {}", output_path.display()),
            StatusTone::Busy,
        );
        Ok(())
    }

    pub fn stop_recording(&mut self) -> Result<Option<RecordingOutcome>, String> {
        let Some(recorder) = self.audio.recorder.take() else {
            return Ok(None);
        };
        let outcome = recorder.stop().map_err(|err| err.to_string())?;
        self.audio.recording_target = None;
        self.set_status(
            format!(
                "Recorded {:.2}s to {}",
                outcome.duration_seconds,
                outcome.path.display()
            ),
            StatusTone::Info,
        );
        Ok(Some(outcome))
    }

    pub fn stop_recording_and_load(&mut self) -> Result<(), String> {
        let target = self.audio.recording_target.clone();
        let Some(outcome) = self.stop_recording()? else {
            return Ok(());
        };
        let (source, relative_path) =
            self.resolve_recording_target(target.as_ref(), &outcome.path)?;
        self.load_waveform_for_selection(&source, &relative_path)?;
        Ok(())
    }

    fn ensure_recordings_source(&mut self, recording_path: &PathBuf) -> Result<SampleSource, String> {
        let root = recording_path
            .parent()
            .ok_or_else(|| "Recording path missing parent".to_string())?
            .to_path_buf();
        if let Some(existing) = self
            .library
            .sources
            .iter()
            .find(|s| s.root == root)
            .cloned()
        {
            self.select_source(Some(existing.id.clone()));
            return Ok(existing);
        }
        let source = match crate::sample_sources::library::lookup_source_id_for_root(&root) {
            Ok(Some(id)) => SampleSource::new_with_id(id, root.clone()),
            Ok(None) => SampleSource::new(root.clone()),
            Err(err) => {
                self.set_status(
                    format!("Could not check library history (continuing): {err}"),
                    StatusTone::Warning,
                );
                SampleSource::new(root.clone())
            }
        };
        SourceDatabase::open(&root)
            .map_err(|err| format!("Failed to create recordings database: {err}"))?;
        let _ = self.cache_db(&source);
        self.library.sources.push(source.clone());
        self.select_source(Some(source.id.clone()));
        self.persist_config("Failed to save config after adding recordings source")?;
        self.prepare_similarity_for_selected_source();
        Ok(source)
    }

    fn next_recording_path_in_source(
        &mut self,
    ) -> Result<(SampleSource, PathBuf, PathBuf), String> {
        let source = self
            .current_source()
            .ok_or_else(|| "Select a source to record into".to_string())?;
        let mut target_folder = self
            .selected_folder_paths()
            .into_iter()
            .next()
            .unwrap_or_default();
        if target_folder.is_absolute() {
            target_folder = target_folder
                .strip_prefix(&source.root)
                .map_err(|_| "Selected folder is outside the current source".to_string())?
                .to_path_buf();
        }
        let base_name = format!("{RECORDING_FILE_PREFIX}{}", formatted_timestamp());
        let mut counter = 0_u32;
        let (relative_path, absolute_path) = loop {
            let suffix = if counter == 0 {
                String::new()
            } else {
                format!("_{counter}")
            };
            let filename = format!("{base_name}{suffix}.{RECORDING_FILE_EXT}");
            let relative_path = target_folder.join(filename);
            let absolute_path = source.root.join(&relative_path);
            if !absolute_path.exists() {
                break (relative_path, absolute_path);
            }
            counter += 1;
        };
        let absolute_path = ensure_recording_path(absolute_path)?;
        Ok((source, relative_path, absolute_path))
    }

    fn resolve_recording_target(
        &mut self,
        target: Option<&RecordingTarget>,
        recording_path: &PathBuf,
    ) -> Result<(SampleSource, PathBuf), String> {
        if let Some(target) = target
            && &target.absolute_path == recording_path
        {
            let source = self
                .source_by_id(&target.source_id)
                .ok_or_else(|| "Recording source unavailable".to_string())?;
            return Ok((source, target.relative_path.clone()));
        }
        let source = self.ensure_recordings_source(recording_path)?;
        let relative_path = recording_path
            .strip_prefix(&source.root)
            .map_err(|_| "Failed to resolve recording path".to_string())?
            .to_path_buf();
        Ok((source, relative_path))
    }

    pub(crate) fn refresh_recording_waveform(&mut self) {
        if !self.is_recording() {
            self.audio.recording_target = None;
            return;
        }
        let (source_id, relative_path, absolute_path, last_refresh_at, last_file_len, loaded_once) =
            match self.audio.recording_target.as_ref() {
                Some(target) => (
                    target.source_id.clone(),
                    target.relative_path.clone(),
                    target.absolute_path.clone(),
                    target.last_refresh_at,
                    target.last_file_len,
                    target.loaded_once,
                ),
                None => return,
            };
        let now = Instant::now();
        if last_refresh_at.is_some_and(|last| now.duration_since(last) < RECORDING_REFRESH_INTERVAL) {
            return;
        }
        let metadata = match std::fs::metadata(&absolute_path) {
            Ok(metadata) => metadata,
            Err(_) => return,
        };
        let len = metadata.len();
        if len == 0 || len == last_file_len {
            if let Some(target) = self.audio.recording_target.as_mut() {
                target.last_refresh_at = Some(now);
            }
            return;
        }
        let bytes = match std::fs::read(&absolute_path) {
            Ok(bytes) => bytes,
            Err(_) => return,
        };
        let recorder = self.audio.recorder.as_ref();
        let decoded = recorder.and_then(|recorder| {
            decode_recording_waveform(
                &bytes,
                recorder.resolved().sample_rate,
                recorder.resolved().channel_count,
            )
        });
        let Some(decoded) = decoded else {
            if let Some(target) = self.audio.recording_target.as_mut() {
                target.last_refresh_at = Some(now);
            }
            return;
        };
        if let Some(source) = self.source_by_id(&source_id) {
            if loaded_once {
                self.apply_waveform_image(decoded);
            } else {
                let _ = self.finish_waveform_load(
                    &source,
                    &relative_path,
                    decoded,
                    bytes,
                    AudioLoadIntent::Selection,
                );
                if let Some(target) = self.audio.recording_target.as_mut() {
                    target.loaded_once = true;
                }
            }
        }
        if let Some(target) = self.audio.recording_target.as_mut() {
            target.last_file_len = len;
            target.last_refresh_at = Some(now);
        }
    }

    fn source_by_id(&self, source_id: &SourceId) -> Option<SampleSource> {
        self.library
            .sources
            .iter()
            .find(|source| &source.id == source_id)
            .cloned()
    }
}

fn ensure_recording_path(mut path: PathBuf) -> Result<PathBuf, String> {
    if path.extension().is_none() {
        path.set_extension(RECORDING_FILE_EXT);
    }
    let parent = path
        .parent()
        .ok_or_else(|| "Recording path missing parent".to_string())?;
    std::fs::create_dir_all(parent).map_err(|err| {
        format!(
            "Failed to create recordings folder {}: {err}",
            parent.display()
        )
    })?;
    Ok(path)
}

fn formatted_timestamp() -> String {
    const FORMAT: &[FormatItem<'_>] =
        format_description!("[year][month][day]_[hour][minute][second]");
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    now.format(&FORMAT).unwrap_or_else(|_| "unknown".into())
}

fn decode_recording_waveform(
    bytes: &[u8],
    sample_rate: u32,
    channels: u16,
) -> Option<DecodedWaveform> {
    let data_offset = find_wav_data_chunk(bytes)?;
    if data_offset >= bytes.len() {
        return None;
    }
    let data = &bytes[data_offset..];
    let total_samples = data.len() / 4;
    if total_samples == 0 {
        return None;
    }
    let channels = channels.max(1) as usize;
    let frames = total_samples / channels.max(1);
    if frames == 0 {
        return None;
    }
    let duration_seconds = frames as f32 / sample_rate.max(1) as f32;
    if frames <= RECORDING_MAX_FULL_FRAMES {
        let mut samples = Vec::with_capacity(total_samples);
        for chunk in data.chunks_exact(4) {
            samples.push(f32::from_le_bytes(chunk.try_into().ok()?));
        }
        return Some(DecodedWaveform {
            cache_token: next_recording_cache_token(),
            samples: Arc::from(samples),
            peaks: None,
            duration_seconds,
            sample_rate,
            channels: channels as u16,
        });
    }

    let bucket_size_frames = peak_bucket_size(frames);
    let bucket_count = frames.div_ceil(bucket_size_frames).max(1);
    let mut mono = vec![(1.0_f32, -1.0_f32); bucket_count];
    let mut left = if channels >= 2 {
        Some(vec![(1.0_f32, -1.0_f32); bucket_count])
    } else {
        None
    };
    let mut right = if channels >= 2 {
        Some(vec![(1.0_f32, -1.0_f32); bucket_count])
    } else {
        None
    };

    let mut sample_index = 0usize;
    for frame in 0..frames {
        let bucket = frame / bucket_size_frames;
        let mut frame_sum = 0.0_f32;
        for ch in 0..channels {
            let offset = sample_index.saturating_mul(4);
            let sample = if offset + 4 <= data.len() {
                let raw = f32::from_le_bytes(data[offset..offset + 4].try_into().ok()?);
                clamp_sample(raw)
            } else {
                0.0
            };
            sample_index = sample_index.saturating_add(1);
            frame_sum += sample;
            if ch == 0 {
                if let Some(left_peaks) = left.as_mut() {
                    let (min, max) = &mut left_peaks[bucket];
                    *min = (*min).min(sample);
                    *max = (*max).max(sample);
                }
            } else if ch == 1 {
                if let Some(right_peaks) = right.as_mut() {
                    let (min, max) = &mut right_peaks[bucket];
                    *min = (*min).min(sample);
                    *max = (*max).max(sample);
                }
            }
        }
        let frame_avg = frame_sum / channels as f32;
        let (min, max) = &mut mono[bucket];
        *min = (*min).min(frame_avg);
        *max = (*max).max(frame_avg);
    }

    Some(DecodedWaveform {
        cache_token: next_recording_cache_token(),
        samples: Arc::from(Vec::new()),
        peaks: Some(Arc::new(WaveformPeaks {
            total_frames: frames,
            channels: channels as u16,
            bucket_size_frames,
            mono,
            left,
            right,
        })),
        duration_seconds,
        sample_rate,
        channels: channels as u16,
    })
}

fn find_wav_data_chunk(bytes: &[u8]) -> Option<usize> {
    if bytes.len() < 12 {
        return None;
    }
    if &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut offset = 12usize;
    while offset + 8 <= bytes.len() {
        let id = &bytes[offset..offset + 4];
        let chunk_size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().ok()?);
        let data_start = offset + 8;
        if id == b"data" {
            return Some(data_start);
        }
        let mut next = data_start.saturating_add(chunk_size as usize);
        if chunk_size % 2 == 1 {
            next = next.saturating_add(1);
        }
        if next <= offset {
            break;
        }
        offset = next;
    }
    None
}

fn peak_bucket_size(frames: usize) -> usize {
    frames.div_ceil(RECORDING_MAX_PEAK_BUCKETS).max(1)
}

fn clamp_sample(sample: f32) -> f32 {
    sample.clamp(-1.0, 1.0)
}

fn next_recording_cache_token() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_CACHE_TOKEN: AtomicU64 = AtomicU64::new(1);
    NEXT_CACHE_TOKEN.fetch_add(1, Ordering::Relaxed)
}
