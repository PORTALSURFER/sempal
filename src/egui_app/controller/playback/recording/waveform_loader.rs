//! Background worker for recording waveform refresh tasks.

use super::{RECORDING_MAX_FULL_FRAMES, RECORDING_MAX_PEAK_BUCKETS};
use crate::sample_sources::SourceId;
use crate::waveform::{DecodedWaveform, WaveformPeaks};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::{mpsc::Receiver, Arc, Condvar, Mutex, OnceLock};
use std::{fs, thread};
use tracing::warn;

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
    Missing,
    /// The recording file failed to load.
    Failed,
    /// The recording file could not be decoded.
    DecodeFailed,
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

#[derive(Clone, Hash, PartialEq, Eq)]
struct RecordingWaveformKey {
    source_id: SourceId,
    relative_path: PathBuf,
}

enum RecordingWaveformMode {
    Full { samples: Vec<f32> },
    Peaks {
        bucket_size_frames: usize,
        mono: Vec<(f32, f32)>,
        left: Option<Vec<(f32, f32)>>,
        right: Option<Vec<(f32, f32)>>,
    },
}

struct RecordingWaveformState {
    data_offset: usize,
    bytes_read: u64,
    tail: Vec<u8>,
    total_frames: usize,
    sample_rate: u32,
    channels: u16,
    analysis_stride: usize,
    analysis_sum: f32,
    analysis_count: usize,
    analysis_samples: Vec<f32>,
    mode: RecordingWaveformMode,
}

impl RecordingWaveformState {
    fn new(sample_rate: u32, channels: u16, data_offset: usize) -> Self {
        Self {
            data_offset,
            bytes_read: 0,
            tail: Vec::new(),
            total_frames: 0,
            sample_rate,
            channels: channels.max(1),
            analysis_stride: 1,
            analysis_sum: 0.0,
            analysis_count: 0,
            analysis_samples: Vec::new(),
            mode: RecordingWaveformMode::Full { samples: Vec::new() },
        }
    }

    fn prepare_for_total_frames(&mut self, total_frames: usize) {
        if total_frames <= RECORDING_MAX_FULL_FRAMES {
            let total_samples = total_frames.saturating_mul(self.channels as usize);
            self.mode = RecordingWaveformMode::Full {
                samples: Vec::with_capacity(total_samples),
            };
            return;
        }
        self.analysis_stride = analysis_stride(self.sample_rate, total_frames);
        self.analysis_samples =
            Vec::with_capacity(total_frames.div_ceil(self.analysis_stride).max(1));
        let bucket_size_frames = peak_bucket_size(total_frames);
        let bucket_count = total_frames.div_ceil(bucket_size_frames).max(1);
        let mono = vec![(1.0_f32, -1.0_f32); bucket_count];
        let left = if self.channels >= 2 {
            Some(vec![(1.0_f32, -1.0_f32); bucket_count])
        } else {
            None
        };
        let right = if self.channels >= 2 {
            Some(vec![(1.0_f32, -1.0_f32); bucket_count])
        } else {
            None
        };
        self.mode = RecordingWaveformMode::Peaks {
            bucket_size_frames,
            mono,
            left,
            right,
        };
    }

    fn requires_rebuild(&self, total_frames: usize) -> bool {
        if let RecordingWaveformMode::Peaks {
            bucket_size_frames,
            ..
        } = self.mode
        {
            let next_bucket = peak_bucket_size(total_frames);
            let next_stride = analysis_stride(self.sample_rate, total_frames);
            return next_bucket != bucket_size_frames || next_stride != self.analysis_stride;
        }
        false
    }

    fn convert_full_to_peaks(&mut self) {
        let RecordingWaveformMode::Full { samples } = &self.mode else {
            return;
        };
        let total_frames = self.total_frames;
        self.analysis_stride = analysis_stride(self.sample_rate, total_frames);
        self.analysis_samples =
            Vec::with_capacity(total_frames.div_ceil(self.analysis_stride).max(1));
        let bucket_size_frames = peak_bucket_size(total_frames);
        let bucket_count = total_frames.div_ceil(bucket_size_frames).max(1);
        let mut mono = vec![(1.0_f32, -1.0_f32); bucket_count];
        let mut left = if self.channels >= 2 {
            Some(vec![(1.0_f32, -1.0_f32); bucket_count])
        } else {
            None
        };
        let mut right = if self.channels >= 2 {
            Some(vec![(1.0_f32, -1.0_f32); bucket_count])
        } else {
            None
        };
        let channels = self.channels as usize;
        let mut analysis_sum = 0.0f32;
        let mut analysis_count = 0usize;
        for frame in 0..total_frames {
            let frame_start = frame.saturating_mul(channels);
            let mut frame_sum = 0.0_f32;
            for ch in 0..channels {
                if let Some(sample) = samples.get(frame_start + ch) {
                    let sample = clamp_sample(*sample);
                    frame_sum += sample;
                    if ch == 0 {
                        if let Some(left_peaks) = left.as_mut() {
                            let bucket = frame / bucket_size_frames;
                            let (min, max) = &mut left_peaks[bucket];
                            *min = (*min).min(sample);
                            *max = (*max).max(sample);
                        }
                    } else if ch == 1 {
                        if let Some(right_peaks) = right.as_mut() {
                            let bucket = frame / bucket_size_frames;
                            let (min, max) = &mut right_peaks[bucket];
                            *min = (*min).min(sample);
                            *max = (*max).max(sample);
                        }
                    }
                }
            }
            let frame_avg = if channels > 0 {
                frame_sum / channels as f32
            } else {
                0.0
            };
            let bucket = frame / bucket_size_frames;
            let (min, max) = &mut mono[bucket];
            *min = (*min).min(frame_avg);
            *max = (*max).max(frame_avg);
            analysis_sum += frame_avg;
            analysis_count += 1;
            if analysis_count >= self.analysis_stride {
                self.analysis_samples
                    .push(analysis_sum / analysis_count as f32);
                analysis_sum = 0.0;
                analysis_count = 0;
            }
        }
        if analysis_count > 0 {
            self.analysis_samples
                .push(analysis_sum / analysis_count as f32);
        }
        self.mode = RecordingWaveformMode::Peaks {
            bucket_size_frames,
            mono,
            left,
            right,
        };
    }

    fn consume_data_bytes(&mut self, bytes: &[u8]) -> usize {
        let frame_bytes = 4usize * self.channels as usize;
        if frame_bytes == 0 {
            return 0;
        }
        let frames_before = self.total_frames;
        self.tail.extend_from_slice(bytes);
        let usable = (self.tail.len() / frame_bytes) * frame_bytes;
        let mut offset = 0usize;
        while offset < usable {
            let frame_index = self.total_frames;
            self.total_frames = self.total_frames.saturating_add(1);
            let mut frame_sum = 0.0f32;
            for ch in 0..self.channels as usize {
                let sample =
                    f32::from_le_bytes(self.tail[offset..offset + 4].try_into().unwrap_or_default());
                let sample = clamp_sample(sample);
                if let RecordingWaveformMode::Full { samples } = &mut self.mode {
                    samples.push(sample);
                } else if let RecordingWaveformMode::Peaks {
                    bucket_size_frames,
                    left,
                    right,
                    ..
                } = &mut self.mode
                {
                    let bucket_size = *bucket_size_frames;
                    let bucket = frame_index / bucket_size;
                    if ch == 0 {
                        if let Some(left_peaks) = left.as_mut() {
                            if bucket >= left_peaks.len() {
                                left_peaks.resize(bucket + 1, (1.0, -1.0));
                            }
                            let (min, max) = &mut left_peaks[bucket];
                            *min = (*min).min(sample);
                            *max = (*max).max(sample);
                        }
                    } else if ch == 1 {
                        if let Some(right_peaks) = right.as_mut() {
                            if bucket >= right_peaks.len() {
                                right_peaks.resize(bucket + 1, (1.0, -1.0));
                            }
                            let (min, max) = &mut right_peaks[bucket];
                            *min = (*min).min(sample);
                            *max = (*max).max(sample);
                        }
                    }
                }
                frame_sum += sample;
                offset += 4;
            }
            let frame_avg = frame_sum / self.channels.max(1) as f32;
            if let RecordingWaveformMode::Peaks {
                bucket_size_frames,
                mono,
                ..
            } = &mut self.mode
            {
                let bucket_size = *bucket_size_frames;
                let bucket = frame_index / bucket_size;
                if bucket >= mono.len() {
                    mono.resize(bucket + 1, (1.0, -1.0));
                }
                let (min, max) = &mut mono[bucket];
                *min = (*min).min(frame_avg);
                *max = (*max).max(frame_avg);
                self.analysis_sum += frame_avg;
                self.analysis_count += 1;
                if self.analysis_count >= self.analysis_stride {
                    self.analysis_samples
                        .push(self.analysis_sum / self.analysis_count as f32);
                    self.analysis_sum = 0.0;
                    self.analysis_count = 0;
                }
            }
        }
        self.bytes_read = self.bytes_read.saturating_add(bytes.len() as u64);
        self.tail.drain(..usable);
        self.total_frames.saturating_sub(frames_before)
    }

    fn to_decoded(&self) -> DecodedWaveform {
        let duration_seconds = self.total_frames as f32 / self.sample_rate.max(1) as f32;
        match &self.mode {
            RecordingWaveformMode::Full { samples } => DecodedWaveform {
                cache_token: next_recording_cache_token(),
                samples: Arc::from(samples.clone()),
                analysis_samples: Arc::from(Vec::new()),
                analysis_sample_rate: 0,
                analysis_stride: 1,
                peaks: None,
                duration_seconds,
                sample_rate: self.sample_rate,
                channels: self.channels,
            },
            RecordingWaveformMode::Peaks {
                bucket_size_frames,
                mono,
                left,
                right,
            } => {
                let analysis_sample_rate =
                    ((self.sample_rate as f32) / self.analysis_stride as f32)
                        .round()
                        .max(1.0) as u32;
                let mut analysis_samples = self.analysis_samples.clone();
                if self.analysis_count > 0 {
                    analysis_samples.push(self.analysis_sum / self.analysis_count as f32);
                }
                DecodedWaveform {
                    cache_token: next_recording_cache_token(),
                    samples: Arc::from(Vec::new()),
                    analysis_samples: Arc::from(analysis_samples),
                    analysis_sample_rate,
                    analysis_stride: self.analysis_stride.max(1),
                    peaks: Some(Arc::new(WaveformPeaks {
                        total_frames: self.total_frames,
                        channels: self.channels,
                        bucket_size_frames: *bucket_size_frames,
                        mono: mono.clone(),
                        left: left.clone(),
                        right: right.clone(),
                    })),
                    duration_seconds,
                    sample_rate: self.sample_rate,
                    channels: self.channels,
                }
            }
        }
    }
}

fn recording_state_map() -> &'static Mutex<HashMap<RecordingWaveformKey, RecordingWaveformState>> {
    static RECORDING_STATE: OnceLock<Mutex<HashMap<RecordingWaveformKey, RecordingWaveformState>>> =
        OnceLock::new();
    RECORDING_STATE.get_or_init(|| Mutex::new(HashMap::new()))
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
        let mut state = self.lock_state();
        state.pending = Some(job);
        self.ready.notify_one();
    }

    fn take_blocking(&self) -> RecordingWaveformJob {
        let mut state = self.lock_state();
        loop {
            if let Some(job) = state.pending.take() {
                return job;
            }
            state = self.wait_ready(state);
        }
    }

    #[cfg(test)]
    fn try_take(&self) -> Option<RecordingWaveformJob> {
        let mut state = self.lock_state();
        state.pending.take()
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, RecordingWaveformJobQueueState> {
        match self.state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                warn!("Recording waveform queue lock poisoned; recovering.");
                poisoned.into_inner()
            }
        }
    }

    fn wait_ready<'a>(
        &self,
        guard: std::sync::MutexGuard<'a, RecordingWaveformJobQueueState>,
    ) -> std::sync::MutexGuard<'a, RecordingWaveformJobQueueState> {
        self.ready.wait(guard).unwrap_or_else(|poisoned| {
            warn!("Recording waveform queue condvar poisoned; recovering.");
            poisoned.into_inner()
        })
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
                RecordingWaveformError::Missing
            } else {
                RecordingWaveformError::Failed
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
    if file_len == job.last_file_len {
        return RecordingWaveformLoadResult {
            request_id: job.request_id,
            source_id: job.source_id,
            relative_path: job.relative_path,
            result: Ok(RecordingWaveformUpdate::NoChange { file_len }),
        };
    }
    if file_len == 0 {
        return RecordingWaveformLoadResult {
            request_id: job.request_id,
            source_id: job.source_id,
            relative_path: job.relative_path,
            result: Ok(RecordingWaveformUpdate::Updated {
                decoded: empty_recording_waveform(job.sample_rate, job.channels),
                bytes: None,
                file_len,
            }),
        };
    }

    let key = RecordingWaveformKey {
        source_id: job.source_id.clone(),
        relative_path: job.relative_path.clone(),
    };
    let mut state = {
        let mut guard = recording_state_map()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.remove(&key)
    };
    if file_len < job.last_file_len {
        state = None;
    }

    if !job.loaded_once {
        let bytes = match fs::read(&job.absolute_path) {
            Ok(bytes) => bytes,
            Err(err) => {
                let missing = err.kind() == std::io::ErrorKind::NotFound;
                let message = if missing {
                    RecordingWaveformError::Missing
                } else {
                    RecordingWaveformError::Failed
                };
                return RecordingWaveformLoadResult {
                    request_id: job.request_id,
                    source_id: job.source_id,
                    relative_path: job.relative_path,
                    result: Err(message),
                };
            }
        };
        let data_offset = match find_wav_data_chunk(&bytes) {
            Some(offset) => offset,
            None => {
                return RecordingWaveformLoadResult {
                    request_id: job.request_id,
                    source_id: job.source_id,
                    relative_path: job.relative_path,
                    result: Err(RecordingWaveformError::DecodeFailed),
                };
            }
        };
        let data_len = bytes.len().saturating_sub(data_offset) as u64;
        let total_frames = total_frames_for_data(data_len, job.channels);
        let mut next_state = RecordingWaveformState::new(job.sample_rate, job.channels, data_offset);
        next_state.prepare_for_total_frames(total_frames);
        next_state.consume_data_bytes(&bytes[data_offset..]);
        if matches!(next_state.mode, RecordingWaveformMode::Full { .. })
            && next_state.total_frames > RECORDING_MAX_FULL_FRAMES
        {
            next_state.convert_full_to_peaks();
        }
        let decoded = next_state.to_decoded();
        let mut guard = recording_state_map()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.insert(key, next_state);
        return RecordingWaveformLoadResult {
            request_id: job.request_id,
            source_id: job.source_id,
            relative_path: job.relative_path,
            result: Ok(RecordingWaveformUpdate::Updated {
                decoded,
                bytes: Some(bytes),
                file_len,
            }),
        };
    }

    let mut file = match File::open(&job.absolute_path) {
        Ok(file) => file,
        Err(err) => {
            let missing = err.kind() == std::io::ErrorKind::NotFound;
            let message = if missing {
                RecordingWaveformError::Missing
            } else {
                RecordingWaveformError::Failed
            };
            return RecordingWaveformLoadResult {
                request_id: job.request_id,
                source_id: job.source_id,
                relative_path: job.relative_path,
                result: Err(message),
            };
        }
    };

    let data_offset = match state.as_ref().map(|s| s.data_offset) {
        Some(offset) => offset,
        None => match read_wav_data_offset_from_file(&mut file, file_len) {
            Some(offset) => offset,
            None => {
                return RecordingWaveformLoadResult {
                    request_id: job.request_id,
                    source_id: job.source_id,
                    relative_path: job.relative_path,
                    result: Err(RecordingWaveformError::DecodeFailed),
                };
            }
        },
    };
    let data_len = file_len.saturating_sub(data_offset as u64);
    let total_frames = total_frames_for_data(data_len, job.channels);
    if total_frames == 0 {
        if let Some(state) = state {
            let mut guard = recording_state_map()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard.insert(key, state);
        }
        return RecordingWaveformLoadResult {
            request_id: job.request_id,
            source_id: job.source_id,
            relative_path: job.relative_path,
            result: Ok(RecordingWaveformUpdate::NoChange { file_len }),
        };
    }

    if let Some(existing) = &state {
        if existing.sample_rate != job.sample_rate || existing.channels != job.channels {
            state = None;
        } else if existing.bytes_read > data_len || existing.total_frames > total_frames {
            state = None;
        }
    }

    let mut next_state = match state {
        Some(state) if !state.requires_rebuild(total_frames) => state,
        _ => match rebuild_state_from_file(
            &mut file,
            data_offset,
            data_len,
            job.sample_rate,
            job.channels,
            total_frames,
        ) {
            Ok(state) => state,
            Err(err) => {
                return RecordingWaveformLoadResult {
                    request_id: job.request_id,
                    source_id: job.source_id,
                    relative_path: job.relative_path,
                    result: Err(err),
                };
            }
        },
    };

    if next_state.bytes_read < data_len {
        let start = data_offset as u64 + next_state.bytes_read;
        if file.seek(SeekFrom::Start(start)).is_err() {
            return RecordingWaveformLoadResult {
                request_id: job.request_id,
                source_id: job.source_id,
                relative_path: job.relative_path,
                result: Err(RecordingWaveformError::Failed),
            };
        }
        let mut remaining = data_len.saturating_sub(next_state.bytes_read);
        let mut buf = vec![0u8; 64 * 1024];
        while remaining > 0 {
            let to_read = remaining.min(buf.len() as u64) as usize;
            let read = file.read(&mut buf[..to_read]).unwrap_or(0);
            if read == 0 {
                break;
            }
            next_state.consume_data_bytes(&buf[..read]);
            remaining = remaining.saturating_sub(read as u64);
        }
    }
    if matches!(next_state.mode, RecordingWaveformMode::Full { .. })
        && next_state.total_frames > RECORDING_MAX_FULL_FRAMES
    {
        next_state.convert_full_to_peaks();
    }
    let decoded = next_state.to_decoded();
    let mut guard = recording_state_map()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.insert(key, next_state);
    RecordingWaveformLoadResult {
        request_id: job.request_id,
        source_id: job.source_id,
        relative_path: job.relative_path,
        result: Ok(RecordingWaveformUpdate::Updated {
            decoded,
            bytes: None,
            file_len,
        }),
    }
}

#[cfg(test)]
fn decode_recording_waveform(
    bytes: &[u8],
    sample_rate: u32,
    channels: u16,
) -> Option<DecodedWaveform> {
    let data_offset = find_wav_data_chunk(bytes)?;
    let data_len = bytes.len().saturating_sub(data_offset) as u64;
    let total_frames = total_frames_for_data(data_len, channels);
    if total_frames == 0 {
        return None;
    }
    let mut state = RecordingWaveformState::new(sample_rate, channels, data_offset);
    state.prepare_for_total_frames(total_frames);
    state.consume_data_bytes(&bytes[data_offset..]);
    if matches!(state.mode, RecordingWaveformMode::Full { .. })
        && state.total_frames > RECORDING_MAX_FULL_FRAMES
    {
        state.convert_full_to_peaks();
    }
    Some(state.to_decoded())
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

fn read_wav_data_offset_from_file(file: &mut File, file_len: u64) -> Option<usize> {
    if file.seek(SeekFrom::Start(0)).is_err() {
        return None;
    }
    let max_read = file_len.min(64 * 1024) as usize;
    let mut header = vec![0u8; max_read];
    let read = file.read(&mut header).ok()?;
    header.truncate(read);
    find_wav_data_chunk(&header)
}

fn total_frames_for_data(data_len: u64, channels: u16) -> usize {
    let channels = channels.max(1) as u64;
    let frame_bytes = 4u64 * channels;
    if frame_bytes == 0 {
        return 0;
    }
    (data_len / frame_bytes) as usize
}

fn rebuild_state_from_file(
    file: &mut File,
    data_offset: usize,
    data_len: u64,
    sample_rate: u32,
    channels: u16,
    total_frames: usize,
) -> Result<RecordingWaveformState, RecordingWaveformError> {
    let mut state = RecordingWaveformState::new(sample_rate, channels, data_offset);
    state.prepare_for_total_frames(total_frames);
    if data_len == 0 {
        return Ok(state);
    }
    if file.seek(SeekFrom::Start(data_offset as u64)).is_err() {
        return Err(RecordingWaveformError::Failed);
    }
    let mut buf = vec![0u8; 64 * 1024];
    let mut remaining = data_len;
    while remaining > 0 {
        let to_read = remaining.min(buf.len() as u64) as usize;
        let read = file.read(&mut buf[..to_read]).unwrap_or(0);
        if read == 0 {
            break;
        }
        state.consume_data_bytes(&buf[..read]);
        remaining = remaining.saturating_sub(read as u64);
    }
    if matches!(state.mode, RecordingWaveformMode::Full { .. })
        && state.total_frames > RECORDING_MAX_FULL_FRAMES
    {
        state.convert_full_to_peaks();
    }
    Ok(state)
}

fn empty_recording_waveform(sample_rate: u32, channels: u16) -> DecodedWaveform {
    DecodedWaveform {
        cache_token: next_recording_cache_token(),
        samples: Arc::from(Vec::new()),
        analysis_samples: Arc::from(Vec::new()),
        analysis_sample_rate: 0,
        analysis_stride: 1,
        peaks: None,
        duration_seconds: 0.0,
        sample_rate,
        channels,
    }
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
    use std::io::Write;
    use tempfile::NamedTempFile;
    use std::sync::Mutex;

    static RECORDING_STATE_LOCK: Mutex<()> = Mutex::new(());

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

    fn build_wav_bytes(data_bytes: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        let chunk_size = 4u32 + 8u32 + data_bytes.len() as u32;
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&chunk_size.to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(data_bytes.len() as u32).to_le_bytes());
        bytes.extend_from_slice(data_bytes);
        bytes
    }

    fn build_wav_samples(samples: &[f32]) -> Vec<u8> {
        let mut data = Vec::with_capacity(samples.len() * 4);
        for sample in samples {
            data.extend_from_slice(&sample.to_le_bytes());
        }
        build_wav_bytes(&data)
    }

    fn clear_recording_state() {
        let mut guard = recording_state_map()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.clear();
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
    fn decode_recording_waveform_ignores_partial_frames() {
        let mut data = Vec::new();
        for sample in [0.1_f32, -0.2_f32, 0.3_f32] {
            data.extend_from_slice(&sample.to_le_bytes());
        }
        let bytes = build_wav_bytes(&data);
        let decoded =
            decode_recording_waveform(&bytes, 48_000, 2).expect("expected waveform");
        assert_eq!(decoded.samples.len(), 2);
        assert!((decoded.samples[0] - 0.1).abs() < 1e-6);
        assert!((decoded.samples[1] + 0.2).abs() < 1e-6);
    }

    #[test]
    fn load_recording_waveform_decodes_updated_file() {
        let _guard = RECORDING_STATE_LOCK.lock().unwrap();
        clear_recording_state();
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

    #[test]
    fn load_recording_waveform_handles_truncation() {
        let _guard = RECORDING_STATE_LOCK.lock().unwrap();
        clear_recording_state();
        let bytes = build_minimal_wav(0.25);
        let mut temp = NamedTempFile::new().expect("tempfile");
        temp.write_all(&bytes).expect("write wav");
        let file_len = bytes.len() as u64;
        let path = temp.path().to_path_buf();
        let job = RecordingWaveformJob {
            request_id: 20,
            source_id: SourceId::from_string("source"),
            relative_path: PathBuf::from("recording.wav"),
            absolute_path: path.clone(),
            last_file_len: 0,
            loaded_once: true,
            sample_rate: 48_000,
            channels: 1,
        };
        let result = load_recording_waveform(job);
        let update = result.result.expect("expected update");
        match update {
            RecordingWaveformUpdate::Updated { decoded, .. } => {
                assert_eq!(decoded.frame_count(), 1);
            }
            _ => panic!("expected updated waveform"),
        }

        temp.as_file_mut().set_len(0).expect("truncate wav");
        let truncated_job = RecordingWaveformJob {
            request_id: 21,
            source_id: SourceId::from_string("source"),
            relative_path: PathBuf::from("recording.wav"),
            absolute_path: path,
            last_file_len: file_len,
            loaded_once: true,
            sample_rate: 48_000,
            channels: 1,
        };
        let truncated_result = load_recording_waveform(truncated_job);
        let update = truncated_result.result.expect("expected update");
        match update {
            RecordingWaveformUpdate::Updated { decoded, file_len, .. } => {
                assert_eq!(file_len, 0);
                assert_eq!(decoded.frame_count(), 0);
            }
            _ => panic!("expected updated waveform"),
        }
    }

    #[test]
    fn load_recording_waveform_appends_incrementally() {
        let _guard = RECORDING_STATE_LOCK.lock().unwrap();
        clear_recording_state();
        let mut temp = NamedTempFile::new().expect("tempfile");
        let first = build_wav_samples(&[0.25]);
        temp.write_all(&first).expect("write wav");
        let path = temp.path().to_path_buf();
        let first_len = first.len() as u64;

        let first_job = RecordingWaveformJob {
            request_id: 30,
            source_id: SourceId::from_string("source"),
            relative_path: PathBuf::from("recording.wav"),
            absolute_path: path.clone(),
            last_file_len: 0,
            loaded_once: false,
            sample_rate: 48_000,
            channels: 1,
        };
        let first_result = load_recording_waveform(first_job);
        let first_update = first_result.result.expect("expected update");
        let RecordingWaveformUpdate::Updated { decoded, .. } = first_update else {
            panic!("expected updated waveform");
        };
        assert_eq!(decoded.frame_count(), 1);

        let second = build_wav_samples(&[0.25, -0.5]);
        temp.as_file_mut().set_len(0).expect("truncate wav");
        temp.as_file_mut()
            .seek(SeekFrom::Start(0))
            .expect("seek wav");
        temp.write_all(&second).expect("write wav");

        let second_job = RecordingWaveformJob {
            request_id: 31,
            source_id: SourceId::from_string("source"),
            relative_path: PathBuf::from("recording.wav"),
            absolute_path: path,
            last_file_len: first_len,
            loaded_once: true,
            sample_rate: 48_000,
            channels: 1,
        };
        let second_result = load_recording_waveform(second_job);
        let second_update = second_result.result.expect("expected update");
        let RecordingWaveformUpdate::Updated { decoded, .. } = second_update else {
            panic!("expected updated waveform");
        };
        assert_eq!(decoded.frame_count(), 2);
    }

    #[test]
    fn recording_waveform_queue_recovers_after_poisoned_lock() {
        let queue = RecordingWaveformJobQueue::new();
        let job = RecordingWaveformJob {
            request_id: 42,
            source_id: SourceId::from_string("source"),
            relative_path: PathBuf::from("recover.wav"),
            absolute_path: PathBuf::from("/tmp/recover.wav"),
            last_file_len: 0,
            loaded_once: false,
            sample_rate: 48_000,
            channels: 1,
        };
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = queue.state.lock().expect("poison queue lock");
            panic!("poison queue lock for test");
        }));
        queue.send(job.clone());
        let pending = queue.try_take().expect("expected pending job");
        assert_eq!(pending.request_id, job.request_id);
        assert_eq!(pending.relative_path, job.relative_path);
    }
}
