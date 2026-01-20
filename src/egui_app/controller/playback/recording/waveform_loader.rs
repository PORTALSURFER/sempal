//! Background worker for recording waveform refresh tasks.

use super::{RECORDING_MAX_FULL_FRAMES, RECORDING_MAX_PEAK_BUCKETS};
use crate::sample_sources::SourceId;
use crate::waveform::{DecodedWaveform, WaveformPeaks};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, mpsc::Receiver};
use std::{fs, thread};

/// Request data needed to refresh a recording waveform off the UI thread.
#[derive(Clone, Debug)]
pub(crate) struct RecordingWaveformJob {
    pub(crate) request_id: u64,
    pub(crate) source_id: SourceId,
    pub(crate) relative_path: PathBuf,
    pub(crate) absolute_path: PathBuf,
    pub(crate) last_file_len: u64,
    pub(crate) loaded_once: bool,
    pub(crate) sample_rate: u32,
    pub(crate) channels: u16,
}

/// Result of a recording waveform refresh operation.
#[derive(Clone)]
pub(crate) enum RecordingWaveformUpdate {
    /// The file length did not change since the last refresh.
    NoChange { file_len: u64 },
    /// A new waveform was decoded from the recording file.
    Updated {
        decoded: DecodedWaveform,
        bytes: Option<Vec<u8>>,
        file_len: u64,
    },
}

/// Errors encountered while refreshing a recording waveform.
#[derive(Debug)]
pub(crate) enum RecordingWaveformError {
    /// The recording file is missing.
    Missing(String),
    /// The recording file failed to load.
    Failed(String),
    /// The recording file could not be decoded.
    DecodeFailed(String),
}

/// Completed recording waveform refresh response.
pub(crate) struct RecordingWaveformLoadResult {
    pub(crate) request_id: u64,
    pub(crate) source_id: SourceId,
    pub(crate) relative_path: PathBuf,
    pub(crate) result: Result<RecordingWaveformUpdate, RecordingWaveformError>,
}

#[derive(Default)]
struct RecordingWaveformJobQueueState {
    pending: Option<RecordingWaveformJob>,
}

/// Latest-only queue for recording waveform refresh jobs.
struct RecordingWaveformJobQueue {
    state: Mutex<RecordingWaveformJobQueueState>,
    ready: Condvar,
}

impl RecordingWaveformJobQueue {
    fn new() -> Self {
        Self {
            state: Mutex::new(RecordingWaveformJobQueueState::default()),
            ready: Condvar::new(),
        }
    }

    fn send(&self, job: RecordingWaveformJob) {
        let mut state = self.state.lock().expect("recording waveform queue poisoned");
        state.pending = Some(job);
        self.ready.notify_one();
    }

    fn take_blocking(&self) -> RecordingWaveformJob {
        let mut state = self.state.lock().expect("recording waveform queue poisoned");
        loop {
            if let Some(job) = state.pending.take() {
                return job;
            }
            state = self.ready.wait(state).expect("recording waveform queue poisoned");
        }
    }

    #[cfg(test)]
    fn try_take(&self) -> Option<RecordingWaveformJob> {
        let mut state = self.state.lock().expect("recording waveform queue poisoned");
        state.pending.take()
    }
}

/// Sender handle for coalesced recording waveform refresh requests.
#[derive(Clone)]
pub(crate) struct RecordingWaveformJobSender {
    queue: Arc<RecordingWaveformJobQueue>,
}

impl RecordingWaveformJobSender {
    /// Replace any pending recording waveform job with the latest request.
    pub(crate) fn send(&self, job: RecordingWaveformJob) {
        self.queue.send(job);
    }
}

/// Spawn a background worker that processes the latest pending recording waveform job.
pub(crate) fn spawn_recording_waveform_loader(
) -> (RecordingWaveformJobSender, Receiver<RecordingWaveformLoadResult>) {
    let queue = Arc::new(RecordingWaveformJobQueue::new());
    let sender = RecordingWaveformJobSender {
        queue: Arc::clone(&queue),
    };
    let (result_tx, result_rx) = std::sync::mpsc::channel::<RecordingWaveformLoadResult>();
    thread::spawn(move || loop {
        let job = queue.take_blocking();
        let result = load_recording_waveform(job);
        let _ = result_tx.send(result);
    });
    (sender, result_rx)
}

fn load_recording_waveform(job: RecordingWaveformJob) -> RecordingWaveformLoadResult {
    let metadata = match fs::metadata(&job.absolute_path) {
        Ok(metadata) => metadata,
        Err(err) => {
            let missing = err.kind() == std::io::ErrorKind::NotFound;
            let message = if missing {
                RecordingWaveformError::Missing(format!(
                    "Recording file missing: {} ({err})",
                    job.absolute_path.display()
                ))
            } else {
                RecordingWaveformError::Failed(format!(
                    "Failed to read recording metadata for {}: {err}",
                    job.absolute_path.display()
                ))
            };
            return RecordingWaveformLoadResult {
                request_id: job.request_id,
                source_id: job.source_id,
                relative_path: job.relative_path,
                result: Err(message),
            };
        }
    };
    let file_len = metadata.len();
    if file_len == 0 || file_len == job.last_file_len {
        return RecordingWaveformLoadResult {
            request_id: job.request_id,
            source_id: job.source_id,
            relative_path: job.relative_path,
            result: Ok(RecordingWaveformUpdate::NoChange { file_len }),
        };
    }

    let bytes = match fs::read(&job.absolute_path) {
        Ok(bytes) => bytes,
        Err(err) => {
            let missing = err.kind() == std::io::ErrorKind::NotFound;
            let message = if missing {
                RecordingWaveformError::Missing(format!(
                    "Recording file missing: {} ({err})",
                    job.absolute_path.display()
                ))
            } else {
                RecordingWaveformError::Failed(format!(
                    "Failed to read recording file {}: {err}",
                    job.absolute_path.display()
                ))
            };
            return RecordingWaveformLoadResult {
                request_id: job.request_id,
                source_id: job.source_id,
                relative_path: job.relative_path,
                result: Err(message),
            };
        }
    };

    let decoded = match decode_recording_waveform(&bytes, job.sample_rate, job.channels) {
        Some(decoded) => decoded,
        None => {
            return RecordingWaveformLoadResult {
                request_id: job.request_id,
                source_id: job.source_id,
                relative_path: job.relative_path,
                result: Err(RecordingWaveformError::DecodeFailed(format!(
                    "Failed to decode recording file {}",
                    job.absolute_path.display()
                ))),
            };
        }
    };

    let bytes = if job.loaded_once { None } else { Some(bytes) };

    RecordingWaveformLoadResult {
        request_id: job.request_id,
        source_id: job.source_id,
        relative_path: job.relative_path,
        result: Ok(RecordingWaveformUpdate::Updated {
            decoded,
            bytes,
            file_len,
        }),
    }
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
            analysis_samples: Arc::from(Vec::new()),
            analysis_sample_rate: 0,
            analysis_stride: 1,
            peaks: None,
            duration_seconds,
            sample_rate,
            channels: channels as u16,
        });
    }

    let bucket_size_frames = peak_bucket_size(frames);
    let bucket_count = frames.div_ceil(bucket_size_frames).max(1);
    let analysis_stride = analysis_stride(sample_rate, frames);
    let mut analysis_samples = Vec::with_capacity(frames.div_ceil(analysis_stride).max(1));
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
    let mut analysis_sum = 0.0f32;
    let mut analysis_count = 0usize;
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
        analysis_sum += frame_avg;
        analysis_count += 1;
        if analysis_count >= analysis_stride {
            analysis_samples.push(analysis_sum / analysis_count as f32);
            analysis_sum = 0.0;
            analysis_count = 0;
        }
    }
    if analysis_count > 0 {
        analysis_samples.push(analysis_sum / analysis_count as f32);
    }
    let analysis_sample_rate =
        ((sample_rate as f32) / analysis_stride as f32).round().max(1.0) as u32;

    Some(DecodedWaveform {
        cache_token: next_recording_cache_token(),
        samples: Arc::from(Vec::new()),
        analysis_samples: Arc::from(analysis_samples),
        analysis_sample_rate,
        analysis_stride,
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

fn analysis_stride(sample_rate: u32, total_frames: usize) -> usize {
    const MIN_ANALYSIS_SAMPLE_RATE: u32 = 8_000;
    const MAX_ANALYSIS_SAMPLES: usize = 5_000_000;

    let sample_rate = sample_rate.max(1);
    let min_stride = (sample_rate / MIN_ANALYSIS_SAMPLE_RATE).max(1) as usize;
    let max_samples_stride = total_frames.div_ceil(MAX_ANALYSIS_SAMPLES).max(1);
    min_stride.max(max_samples_stride).max(1)
}

fn clamp_sample(sample: f32) -> f32 {
    sample.clamp(-1.0, 1.0)
}

fn next_recording_cache_token() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_CACHE_TOKEN: AtomicU64 = AtomicU64::new(1);
    NEXT_CACHE_TOKEN.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;
    use std::io::Write;

    fn build_minimal_wav(sample: f32) -> Vec<u8> {
        let mut bytes = Vec::new();
        let data_bytes = sample.to_le_bytes();
        let chunk_size = 4u32 + 8u32 + data_bytes.len() as u32;
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&chunk_size.to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(data_bytes.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&data_bytes);
        bytes
    }

    #[test]
    fn recording_waveform_queue_replaces_pending() {
        let queue = RecordingWaveformJobQueue::new();
        let job = RecordingWaveformJob {
            request_id: 1,
            source_id: SourceId::from_string("source"),
            relative_path: PathBuf::from("one.wav"),
            absolute_path: PathBuf::from("/tmp/one.wav"),
            last_file_len: 0,
            loaded_once: false,
            sample_rate: 48_000,
            channels: 1,
        };
        let newer = RecordingWaveformJob {
            request_id: 2,
            source_id: SourceId::from_string("source"),
            relative_path: PathBuf::from("two.wav"),
            absolute_path: PathBuf::from("/tmp/two.wav"),
            last_file_len: 0,
            loaded_once: false,
            sample_rate: 48_000,
            channels: 1,
        };
        queue.send(job);
        queue.send(newer.clone());
        let pending = queue.try_take().expect("expected pending job");
        assert_eq!(pending.request_id, newer.request_id);
        assert_eq!(pending.relative_path, newer.relative_path);
    }

    #[test]
    fn load_recording_waveform_decodes_updated_file() {
        let bytes = build_minimal_wav(0.5);
        let mut temp = NamedTempFile::new().expect("tempfile");
        temp.write_all(&bytes).expect("write wav");
        let path = temp.path().to_path_buf();
        let job = RecordingWaveformJob {
            request_id: 10,
            source_id: SourceId::from_string("source"),
            relative_path: PathBuf::from("recording.wav"),
            absolute_path: path,
            last_file_len: 0,
            loaded_once: false,
            sample_rate: 48_000,
            channels: 1,
        };
        let result = load_recording_waveform(job);
        let update = result.result.expect("expected update");
        match update {
            RecordingWaveformUpdate::Updated { decoded, bytes, file_len } => {
                assert!(decoded.duration_seconds > 0.0);
                assert!(bytes.is_some());
                assert!(file_len > 0);
            }
            RecordingWaveformUpdate::NoChange { .. } => {
                panic!("expected updated waveform");
            }
        }
    }
}
