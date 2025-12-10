use std::{
    io::Cursor,
    sync::Arc,
    time::{Duration, Instant},
};

pub mod output;

use rodio::{Decoder, OutputStream, Sink, Source};

/// Simple audio helper that plays a loaded wav buffer and reports progress.
pub struct AudioPlayer {
    stream: OutputStream,
    sink: Option<Sink>,
    current_audio: Option<Arc<[u8]>>,
    track_duration: Option<f32>,
    started_at: Option<Instant>,
    play_span: Option<(f32, f32)>,
    looping: bool,
    loop_offset: Option<f32>,
    volume: f32,
    output: ResolvedOutput,
}

const SEGMENT_FADE: Duration = Duration::from_millis(5);

impl AudioPlayer {
    /// Create a new audio player using the default output device.
    pub fn new() -> Result<Self, String> {
        Self::from_config(&AudioOutputConfig::default())
    }

    /// Create a new audio player honoring the requested output configuration.
    pub fn from_config(config: &AudioOutputConfig) -> Result<Self, String> {
        let outcome = open_output_stream(config)?;
        Ok(Self {
            stream: outcome.stream,
            sink: None,
            current_audio: None,
            track_duration: None,
            started_at: None,
            play_span: None,
            looping: false,
            loop_offset: None,
            volume: 1.0,
            output: outcome.resolved,
        })
    }

    /// Store audio bytes and duration for later playback.
    pub fn set_audio(&mut self, data: Vec<u8>, duration: f32) {
        let audio = Arc::from(data);
        let provided = duration.max(0.0);
        let fallback = Self::decoder_duration(&audio)
            .or_else(|| Self::wav_header_duration(&audio))
            .unwrap_or(0.0);
        let chosen = if provided > 0.0 { provided } else { fallback };
        self.track_duration = Some(chosen);
        self.current_audio = Some(audio);
        self.started_at = None;
        self.play_span = None;
        self.looping = false;
        self.loop_offset = None;
    }

    /// Adjust master output volume for current and future playback.
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        if let Some(sink) = self.sink.as_mut() {
            sink.set_volume(self.volume);
        }
    }

    /// Stop any active playback.
    pub fn stop(&mut self) {
        self.fade_out_current_sink();
        self.started_at = None;
        self.play_span = None;
        self.looping = false;
        self.loop_offset = None;
    }

    /// Begin playback from the stored buffer.
    #[allow(dead_code)]
    pub fn play(&mut self) -> Result<(), String> {
        self.play_range(0.0, 1.0, false)
    }

    /// Begin playback at the given normalized position (0.0 - 1.0).
    pub fn play_from_fraction(&mut self, fraction: f32) -> Result<(), String> {
        self.play_range(fraction, 1.0, false)
    }

    /// Play between two normalized points, optionally looping the segment.
    pub fn play_range(&mut self, start: f32, end: f32, looped: bool) -> Result<(), String> {
        let duration = self
            .track_duration
            .ok_or_else(|| "Load a .wav file first".to_string())?;
        let clamped_start = start.clamp(0.0, 1.0) * duration;
        let clamped_end = end.clamp(0.0, 1.0) * duration;
        let mut bounded_start = clamped_start.min(duration);
        let mut bounded_end = clamped_end.min(duration);
        let min_span = (duration * 0.01).max(0.01);
        if bounded_end <= bounded_start {
            bounded_end = (bounded_start + min_span).min(duration);
            if bounded_end <= bounded_start {
                bounded_start = (duration - min_span).max(0.0);
                bounded_end = duration.max(bounded_start + 0.001);
            }
        }
        self.loop_offset = None;
        self.start_with_span(clamped_start, bounded_end, duration, looped)
    }

    /// Loop the full track while starting playback at the given normalized position.
    pub fn play_full_wrapped_from(&mut self, start: f32) -> Result<(), String> {
        let duration = self
            .track_duration
            .ok_or_else(|| "Load a .wav file first".to_string())?;
        let offset = start.clamp(0.0, 1.0) * duration;
        let bytes = self.audio_bytes()?;
        if duration <= 0.0 {
            return Err("Load a .wav file first".into());
        }

        self.fade_out_current_sink();

        let fade = fade_duration(duration);
        let source = Self::decoder_from_bytes(bytes)?;
        let limited = source
            .fade_in(fade)
            .take_duration(Duration::from_secs_f32(duration))
            .buffered();
        let faded = EdgeFade::new(limited, fade);
        let repeated = faded
            .repeat_infinite()
            .skip_duration(Duration::from_secs_f32(offset));

        let sink = Sink::connect_new(self.stream.mixer());
        sink.set_volume(self.volume);
        sink.append(repeated);
        sink.play();

        self.started_at = Some(Instant::now());
        self.play_span = Some((0.0, duration));
        self.looping = true;
        self.loop_offset = Some(offset);
        self.sink = Some(sink);
        Ok(())
    }

    /// Current playback progress as a 0-1 fraction.
    pub fn progress(&self) -> Option<f32> {
        let duration = self.track_duration?;
        let started_at = self.started_at?;
        let elapsed = started_at.elapsed().as_secs_f32();
        let adjusted_elapsed = if self.looping {
            let span_length = self
                .play_span
                .map(|(start, end)| (end - start).max(f32::EPSILON))
                .unwrap_or(duration);
            let offset = self
                .loop_offset
                .filter(|_| (span_length - duration).abs() < f32::EPSILON)
                .unwrap_or(0.0);
            elapsed + offset
        } else {
            elapsed
        };
        normalized_progress(self.play_span, duration, adjusted_elapsed, self.looping)
    }

    /// True while the sink is still playing the queued audio.
    pub fn is_playing(&self) -> bool {
        self.sink
            .as_ref()
            .map(|sink| !sink.empty())
            .unwrap_or(false)
            && self.started_at.is_some()
    }

    /// True when the current sink is configured to loop.
    pub fn is_looping(&self) -> bool {
        self.looping
    }

    /// Remaining wall-clock time until the current loop iteration finishes.
    pub fn remaining_loop_duration(&self) -> Option<Duration> {
        if !self.looping {
            return None;
        }
        let started_at = self.started_at?;
        let (start, end) = self.play_span?;
        let span_length = (end - start).max(f32::EPSILON);
        let elapsed = started_at.elapsed().as_secs_f32();
        let elapsed_in_span = elapsed % span_length;
        let remaining = span_length - elapsed_in_span;
        Some(Duration::from_secs_f32(remaining.max(0.0)))
    }

    fn start_with_span(
        &mut self,
        start_seconds: f32,
        end_seconds: f32,
        duration: f32,
        looped: bool,
    ) -> Result<(), String> {
        let bytes = self.audio_bytes()?;
        if duration <= 0.0 {
            return Err("Load a .wav file first".into());
        }
        let bounded_start = start_seconds.clamp(0.0, duration);
        let bounded_end = end_seconds.clamp(bounded_start, duration);
        let span_length = (bounded_end - bounded_start).max(0.001);
        let fade = fade_duration(span_length);

        self.fade_out_current_sink();

        let mut source = Self::decoder_from_bytes(bytes)?;
        source
            .try_seek(Duration::from_secs_f32(bounded_start))
            .map_err(Self::map_seek_error)?;
        let limited = source
            .fade_in(fade)
            .take_duration(Duration::from_secs_f32(span_length))
            .buffered();
        let faded = EdgeFade::new(limited, fade);

        let final_source: Box<dyn Source<Item = f32> + Send> = if looped {
            Box::new(faded.repeat_infinite())
        } else {
            Box::new(faded)
        };

        let sink = Sink::connect_new(self.stream.mixer());
        sink.set_volume(self.volume);
        sink.append(final_source);
        sink.play();
        self.started_at = Some(Instant::now());
        self.play_span = Some((bounded_start, bounded_start + span_length));
        self.looping = looped;
        self.sink = Some(sink);
        Ok(())
    }

    fn map_seek_error(error: rodio::source::SeekError) -> String {
        match error {
            rodio::source::SeekError::NotSupported { .. } => {
                "Seeking not supported for this audio source".into()
            }
            _ => format!("Audio seek failed: {error}"),
        }
    }

    fn audio_bytes(&self) -> Result<Arc<[u8]>, String> {
        self.current_audio
            .as_ref()
            .cloned()
            .ok_or_else(|| "Load a .wav file first".to_string())
    }

    fn decoder_from_bytes(bytes: Arc<[u8]>) -> Result<Decoder<Cursor<Arc<[u8]>>>, String> {
        let byte_len = bytes.len() as u64;
        Decoder::builder()
            .with_data(Cursor::new(bytes))
            .with_byte_len(byte_len)
            .with_seekable(true)
            .with_hint("wav")
            .build()
            .map_err(|error| format!("Audio decode failed: {error}"))
    }

    fn decoder_duration(bytes: &Arc<[u8]>) -> Option<f32> {
        Self::decoder_from_bytes(bytes.clone())
            .ok()
            .and_then(|decoder| decoder.total_duration())
            .map(|duration| duration.as_secs_f32())
    }

    fn wav_header_duration(bytes: &Arc<[u8]>) -> Option<f32> {
        let reader = hound::WavReader::new(Cursor::new(bytes.clone())).ok()?;
        let spec = reader.spec();
        let sample_rate = spec.sample_rate as f32;
        let channels = spec.channels.max(1) as f32;
        if sample_rate <= 0.0 {
            return None;
        }
        Some(reader.duration() as f32 / (sample_rate * channels))
    }

    #[cfg(test)]
    fn span_sample_count(
        bytes: Arc<[u8]>,
        start_seconds: f32,
        end_seconds: f32,
    ) -> Result<(usize, u32, u16), String> {
        let mut source = Self::decoder_from_bytes(bytes)?;
        source
            .try_seek(Duration::from_secs_f32(start_seconds))
            .map_err(Self::map_seek_error)?;
        let span_length = (end_seconds - start_seconds).max(0.001);
        let fade = fade_duration(span_length);
        let limited = source
            .fade_in(fade)
            .take_duration(Duration::from_secs_f32(span_length))
            .buffered();
        let mut faded = EdgeFade::new(limited, fade);
        let sample_rate = faded.sample_rate();
        let channels = faded.channels();
        let mut count = 0usize;
        while faded.next().is_some() {
            count = count.saturating_add(1);
        }
        Ok((count, sample_rate, channels))
    }

    /// Mute and stop the current sink without blocking the UI thread.
    fn fade_out_current_sink(&mut self) {
        let Some(mut sink) = self.sink.take() else {
            return;
        };
        let start_volume = sink.volume();
        if start_volume <= 0.0 {
            sink.stop();
            return;
        }
        let fade = SEGMENT_FADE;
        if fade.is_zero() {
            sink.stop();
            return;
        }
        std::thread::spawn(move || {
            let steps = 5u32;
            let step_sleep = fade / steps;
            for step in 0..steps {
                let remaining = steps.saturating_sub(step + 1) as f32 / steps as f32;
                sink.set_volume(start_volume * remaining.max(0.0));
                std::thread::sleep(step_sleep);
            }
            sink.stop();
        });
    }

    /// Active output configuration after initialization.
    pub fn output_details(&self) -> &ResolvedOutput {
        &self.output
    }

    #[cfg(test)]
    /// Build a short-lived playing instance for tests that need an active sink.
    pub fn playing_for_tests() -> Option<Self> {
        use rodio::source::{SineWave, Source};

        let outcome = open_output_stream(&AudioOutputConfig::default()).ok()?;
        let sink = rodio::Sink::connect_new(outcome.stream.mixer());
        sink.append(SineWave::new(220.0).take_duration(Duration::from_millis(50)));
        sink.play();
        Some(Self {
            stream: outcome.stream,
            sink: Some(sink),
            current_audio: None,
            track_duration: Some(0.05),
            started_at: Some(Instant::now()),
            play_span: Some((0.0, 0.05)),
            looping: false,
            loop_offset: None,
            volume: 1.0,
            output: outcome.resolved,
        })
    }
}

fn fade_duration(span_seconds: f32) -> Duration {
    if span_seconds <= 0.0 {
        return Duration::from_secs(0);
    }
    let max_fade = SEGMENT_FADE.as_secs_f32();
    let clamped = max_fade.min(span_seconds * 0.5);
    Duration::from_secs_f32(clamped.max(0.0))
}

fn normalized_progress(
    span: Option<(f32, f32)>,
    duration: f32,
    elapsed: f32,
    looping: bool,
) -> Option<f32> {
    if duration <= 0.0 {
        return None;
    }
    let (start, end) = span.unwrap_or((0.0, duration));
    let span_length = (end - start).max(f32::EPSILON);
    let within_span = if looping {
        elapsed % span_length
    } else {
        elapsed.min(span_length)
    };
    let absolute = start + within_span;
    Some((absolute / duration).clamp(0.0, 1.0))
}

#[derive(Clone)]
struct EdgeFade<S> {
    inner: S,
    fade_secs: f32,
    total_secs: Option<f32>,
    fade_out_start: Option<f32>,
    sample_rate: u32,
    channels: u16,
    samples_emitted: u64,
}

impl<S> EdgeFade<S> {
    fn new(inner: S, fade: Duration) -> Self
    where
        S: Source<Item = f32> + Clone,
    {
        let fade_secs = fade.as_secs_f32();
        let total_secs = inner.total_duration().map(|d| d.as_secs_f32());
        let fade_out_start = total_secs.and_then(|total| {
            if fade_secs <= 0.0 || fade_secs >= total {
                None
            } else {
                Some(total - fade_secs)
            }
        });
        let sample_rate = inner.sample_rate();
        let channels = inner.channels();
        Self {
            inner,
            fade_secs,
            total_secs,
            fade_out_start,
            sample_rate,
            channels,
            samples_emitted: 0,
        }
    }
}

impl<S> Iterator for EdgeFade<S>
where
    S: Source<Item = f32>,
{
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        let sample = self.inner.next()?;
        let time = if self.sample_rate > 0 && self.channels > 0 {
            self.samples_emitted as f32 / (self.sample_rate as f32 * self.channels as f32)
        } else {
            0.0
        };
        self.samples_emitted = self.samples_emitted.saturating_add(1);
        if self.fade_secs <= 0.0 {
            return Some(sample);
        }
        let mut factor = 1.0;
        if time < self.fade_secs {
            factor *= (time / self.fade_secs).clamp(0.0, 1.0);
        }
        if let (Some(total), Some(start)) = (self.total_secs, self.fade_out_start) {
            if time > start {
                factor *= ((total - time) / self.fade_secs).clamp(0.0, 1.0);
            }
        }
        Some(sample * factor)
    }
}

impl<S> Source for EdgeFade<S>
where
    S: Source<Item = f32>,
{
    #[inline]
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }

    #[inline]
    fn channels(&self) -> u16 {
        self.inner.channels()
    }

    #[inline]
    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }

    #[inline]
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
}

#[cfg(test)]
mod tests;
pub use output::{
    AudioDeviceSummary, AudioHostSummary, AudioOutputConfig, ResolvedOutput, available_devices,
    available_hosts, open_output_stream, supported_sample_rates,
};
