use std::{
    io::Cursor,
    time::{Duration, Instant},
};

use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};

/// Simple audio helper that plays a loaded wav buffer and reports progress.
pub struct AudioPlayer {
    _stream: OutputStream,
    handle: OutputStreamHandle,
    sink: Option<Sink>,
    current_audio: Option<Vec<u8>>,
    track_duration: Option<f32>,
    started_at: Option<Instant>,
}

impl AudioPlayer {
    /// Create a new audio player using the default output device.
    pub fn new() -> Result<Self, String> {
        let (stream, handle) =
            OutputStream::try_default().map_err(|error| format!("Audio init failed: {error}"))?;
        Ok(Self {
            _stream: stream,
            handle,
            sink: None,
            current_audio: None,
            track_duration: None,
            started_at: None,
        })
    }

    /// Store audio bytes and duration for later playback.
    pub fn set_audio(&mut self, data: Vec<u8>, duration: f32) {
        self.current_audio = Some(data);
        self.track_duration = Some(duration);
        self.started_at = None;
    }

    /// Stop any active playback.
    pub fn stop(&mut self) {
        if let Some(sink) = self.sink.take() {
            sink.stop();
        }
        self.started_at = None;
    }

    /// Begin playback from the stored buffer.
    pub fn play(&mut self) -> Result<(), String> {
        self.play_from_fraction(0.0)
    }

    /// Begin playback at the given normalized position (0.0 - 1.0).
    pub fn play_from_fraction(&mut self, fraction: f32) -> Result<(), String> {
        let duration = self
            .track_duration
            .ok_or_else(|| "Load a .wav file first".to_string())?;
        let start_seconds = fraction.clamp(0.0, 1.0) * duration;
        self.start_from_offset(start_seconds, duration)
    }

    /// Current playback progress as a 0-1 fraction.
    pub fn progress(&self) -> Option<f32> {
        let duration = self.track_duration?;
        let started_at = self.started_at?;
        if duration <= 0.0 {
            return None;
        }
        let elapsed = started_at.elapsed().as_secs_f32();
        Some((elapsed / duration).clamp(0.0, 1.0))
    }

    /// True while the sink is still playing the queued audio.
    pub fn is_playing(&self) -> bool {
        self.sink
            .as_ref()
            .map(|sink| !sink.empty())
            .unwrap_or(false)
            && self.started_at.is_some()
    }

    fn start_from_offset(&mut self, start_seconds: f32, duration: f32) -> Result<(), String> {
        let bytes = self
            .current_audio
            .as_ref()
            .cloned()
            .ok_or_else(|| "Load a .wav file first".to_string())?;
        if duration <= 0.0 {
            return Err("Load a .wav file first".into());
        }

        if let Some(sink) = self.sink.take() {
            sink.stop();
        }

        let offset = start_seconds.clamp(0.0, duration);
        let source = Decoder::new(Cursor::new(bytes))
            .map_err(|error| format!("Audio decode failed: {error}"))?
            .skip_duration(Duration::from_secs_f32(offset));

        let sink =
            Sink::try_new(&self.handle).map_err(|error| format!("Audio output failed: {error}"))?;
        sink.append(source);
        sink.play();
        self.started_at = Some(Instant::now() - Duration::from_secs_f32(offset));
        self.sink = Some(sink);
        Ok(())
    }
}
