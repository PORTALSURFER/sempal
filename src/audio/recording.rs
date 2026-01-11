use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use cpal::Stream;
use cpal::traits::StreamTrait;
use crate::audio::SamplesBuffer;
use super::output::MonitorSink;

use super::input::{
    AudioInputConfig, AudioInputError, ResolvedInput, StreamChannelSelection, build_input_stream,
    resolve_input_stream_config,
};

pub struct RecordingOutcome {
    pub path: PathBuf,
    pub resolved: ResolvedInput,
    pub frames: u64,
    pub duration_seconds: f32,
}

pub struct AudioRecorder {
    stream: Stream,
    writer: RecorderWriter,
    resolved: ResolvedInput,
    path: PathBuf,
    started_at: Instant,
    monitor_sender: Arc<std::sync::Mutex<Option<Sender<MonitorCommand>>>>,
}

impl AudioRecorder {
    pub fn start(config: &AudioInputConfig, path: PathBuf) -> Result<Self, AudioInputError> {
        let resolved = resolve_input_stream_config(config)?;
        let selection = StreamChannelSelection::new(
            resolved.stream_config.channels,
            &resolved.selected_channels,
        );
        let (sender, receiver) = std::sync::mpsc::channel();
        let monitor_sender = Arc::new(std::sync::Mutex::new(None::<Sender<MonitorCommand>>));
        let writer = RecorderWriter::spawn(
            path.clone(),
            resolved.resolved.sample_rate,
            resolved.resolved.channel_count,
            receiver,
            sender.clone(),
        )?;
        let sender_clone = sender.clone();
        let monitor_sender_clone = monitor_sender.clone();
        let stream = build_input_stream(
            &resolved.device,
            &resolved.stream_config,
            resolved.sample_format,
            selection,
            move |samples| {
                if let Ok(slot) = monitor_sender_clone.lock()
                    && let Some(monitor) = slot.as_ref()
                {
                    let _ = monitor.send(MonitorCommand::Samples(samples.clone()));
                }
                let _ = sender_clone.send(RecorderCommand::Samples(samples));
            },
        )?;
        stream
            .play()
            .map_err(|source| AudioInputError::StartStream { source })?;
        Ok(Self {
            stream,
            writer,
            resolved: resolved.resolved,
            path,
            started_at: Instant::now(),
            monitor_sender,
        })
    }

    pub fn stop(mut self) -> Result<RecordingOutcome, AudioInputError> {
        drop(self.stream);
        let _ = self.writer.stop();
        let stats = self.writer.join()?;
        let duration_seconds = if stats.frames == 0 {
            0.0
        } else {
            stats.frames as f32 / self.resolved.sample_rate.max(1) as f32
        };
        Ok(RecordingOutcome {
            path: self.path,
            resolved: self.resolved,
            frames: stats.frames,
            duration_seconds,
        })
    }

    pub fn is_active(&self) -> bool {
        self.started_at.elapsed().as_secs_f32() >= 0.0
    }

    pub fn resolved(&self) -> &ResolvedInput {
        &self.resolved
    }

    pub fn output_path(&self) -> &Path {
        &self.path
    }

    pub fn attach_monitor(&self, monitor: &InputMonitor) {
        self.set_monitor_sender(Some(monitor.sender()));
    }

    pub fn detach_monitor(&self) {
        self.set_monitor_sender(None);
    }

    fn set_monitor_sender(&self, sender: Option<Sender<MonitorCommand>>) {
        if let Ok(mut slot) = self.monitor_sender.lock() {
            *slot = sender;
        }
    }
}

struct RecorderWriter {
    sender: Sender<RecorderCommand>,
    join: Option<JoinHandle<Result<RecordingStats, AudioInputError>>>,
}

impl RecorderWriter {
    fn spawn(
        path: PathBuf,
        sample_rate: u32,
        channels: u16,
        receiver: Receiver<RecorderCommand>,
        sender: Sender<RecorderCommand>,
    ) -> Result<Self, AudioInputError> {
        let writer = WavSampleWriter::new(&path, sample_rate, channels)?;
        let join = thread::spawn(move || writer_loop(writer, receiver));
        Ok(Self {
            sender,
            join: Some(join),
        })
    }

    fn stop(&self) -> Result<(), AudioInputError> {
        self.sender
            .send(RecorderCommand::Stop)
            .map_err(|err| AudioInputError::RecordingFailed {
                detail: format!("Failed to stop recorder: {err}"),
            })
    }

    fn join(&mut self) -> Result<RecordingStats, AudioInputError> {
        let handle = self
            .join
            .take()
            .ok_or_else(|| AudioInputError::RecordingFailed {
                detail: "Recorder writer already joined".into(),
            })?;
        handle
            .join()
            .map_err(|_| AudioInputError::RecordingFailed {
                detail: "Recorder writer thread panicked".into(),
            })?
    }
}

#[derive(Clone, Copy)]
struct RecordingStats {
    frames: u64,
}

enum RecorderCommand {
    Samples(Vec<f32>),
    Stop,
}

pub enum MonitorCommand {
    Samples(Vec<f32>),
    Stop,
}

pub struct InputMonitor {
    sender: Sender<MonitorCommand>,
    join: Option<JoinHandle<()>>,
}

impl InputMonitor {
    pub fn start(sink: MonitorSink, channels: u16, sample_rate: u32) -> Self {
        let (sender, receiver) = std::sync::mpsc::channel();
        let join = thread::spawn(move || monitor_loop(sink, channels, sample_rate, receiver));
        Self {
            sender,
            join: Some(join),
        }
    }

    pub fn sender(&self) -> Sender<MonitorCommand> {
        self.sender.clone()
    }

    pub fn stop(mut self) {
        let _ = self.sender.send(MonitorCommand::Stop);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn writer_loop(
    mut writer: WavSampleWriter,
    receiver: Receiver<RecorderCommand>,
) -> Result<RecordingStats, AudioInputError> {
    while let Ok(command) = receiver.recv() {
        match command {
            RecorderCommand::Samples(samples) => {
                writer.write_samples(&samples)?;
            }
            RecorderCommand::Stop => break,
        }
    }
    writer.finalize()
}

fn monitor_loop(sink: MonitorSink, channels: u16, sample_rate: u32, receiver: Receiver<MonitorCommand>) {
    let channels = channels.max(1);
    let sample_rate = sample_rate.max(1);
    sink.play();
    while let Ok(command) = receiver.recv() {
        match command {
            MonitorCommand::Samples(samples) => {
                if samples.is_empty() {
                    continue;
                }
                let source = SamplesBuffer::new(channels, sample_rate, samples);
                sink.append(source);
            }
            MonitorCommand::Stop => break,
        }
    }
    sink.stop();
}

struct WavSampleWriter {
    writer: hound::WavWriter<BufWriter<File>>,
    channels: u16,
    written_samples: u64,
}

impl WavSampleWriter {
    fn new(path: &Path, sample_rate: u32, channels: u16) -> Result<Self, AudioInputError> {
        let spec = hound::WavSpec {
            channels,
            sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let file = File::create(path).map_err(|err| AudioInputError::RecordingFailed {
            detail: format!("Failed to create wav file: {err}"),
        })?;
        let writer = hound::WavWriter::new(BufWriter::new(file), spec).map_err(|err| {
            AudioInputError::RecordingFailed {
                detail: format!("Failed to create wav writer: {err}"),
            }
        })?;
        Ok(Self {
            writer,
            channels,
            written_samples: 0,
        })
    }

    fn write_samples(&mut self, samples: &[f32]) -> Result<(), AudioInputError> {
        for &sample in samples {
            self.writer
                .write_sample(sample)
                .map_err(|err| AudioInputError::RecordingFailed {
                    detail: format!("Failed to write wav sample: {err}"),
                })?;
            self.written_samples += 1;
        }
        Ok(())
    }

    fn finalize(self) -> Result<RecordingStats, AudioInputError> {
        self.writer
            .finalize()
            .map_err(|err| AudioInputError::RecordingFailed {
                detail: format!("Failed to finalize wav writer: {err}"),
            })?;
        let channels = self.channels.max(1) as u64;
        let frames = self.written_samples / channels;
        Ok(RecordingStats { frames })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn wav_writer_outputs_float_wav() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("recording.wav");
        let mut writer = WavSampleWriter::new(&path, 48_000, 2).unwrap();
        writer.write_samples(&[0.0, 0.5, -0.5, 1.0]).unwrap();
        let stats = writer.finalize().unwrap();
        assert_eq!(stats.frames, 2);

        let mut reader = hound::WavReader::open(&path).unwrap();
        let spec = reader.spec();
        assert_eq!(spec.channels, 2);
        assert_eq!(spec.sample_rate, 48_000);
        assert_eq!(spec.sample_format, hound::SampleFormat::Float);
        assert_eq!(reader.samples::<f32>().count(), 4);
    }
}
