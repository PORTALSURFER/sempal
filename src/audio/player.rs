use std::sync::Arc;
use std::time::{Duration, Instant};

use rodio::{OutputStream, Sink, Source};

use super::DEFAULT_ANTI_CLIP_FADE;
use super::fade::{EdgeFade, FadeOutHandle, FadeOutOnRequest, fade_duration, fade_frames_for_duration};
use super::mixer::{decoder_duration, decoder_from_bytes, map_seek_error, wav_header_duration};
use super::output::{AudioOutputConfig, ResolvedOutput, open_output_stream};
use super::routing::{duration_from_secs_f32, duration_mod};

/// Simple audio helper that plays a loaded wav buffer and reports progress.
pub struct AudioPlayer {
    stream: OutputStream,
    sink: Option<Sink>,
    fade_out: Option<FadeOutHandle>,
    sink_format: Option<(u32, u16)>,
    current_audio: Option<Arc<[u8]>>,
    track_duration: Option<f32>,
    started_at: Option<Instant>,
    play_span: Option<(f32, f32)>,
    looping: bool,
    loop_offset: Option<f32>,
    volume: f32,
    anti_clip_enabled: bool,
    anti_clip_fade: Duration,
    output: ResolvedOutput,
    #[cfg(test)]
    elapsed_override: Option<Duration>,
}

impl AudioPlayer {
    /// Create a new audio player using the default output device.
    pub fn new() -> Result<Self, String> {
        Self::from_config(&AudioOutputConfig::default())
    }

    /// Create a new audio player honoring the requested output configuration.
    pub fn from_config(config: &AudioOutputConfig) -> Result<Self, String> {
        let outcome = open_output_stream(config).map_err(|err| err.to_string())?;
        Ok(Self {
            stream: outcome.stream,
            sink: None,
            fade_out: None,
            sink_format: None,
            current_audio: None,
            track_duration: None,
            started_at: None,
            play_span: None,
            looping: false,
            loop_offset: None,
            volume: 1.0,
            anti_clip_enabled: true,
            anti_clip_fade: DEFAULT_ANTI_CLIP_FADE,
            output: outcome.resolved,
            #[cfg(test)]
            elapsed_override: None,
        })
    }

    /// Store audio bytes and duration for later playback.
    pub fn set_audio(&mut self, data: Vec<u8>, duration: f32) {
        let audio = Arc::from(data);
        let provided = duration.max(0.0);
        let fallback = decoder_duration(&audio)
            .or_else(|| wav_header_duration(&audio))
            .unwrap_or(0.0);
        let chosen = if provided > 0.0 { provided } else { fallback };
        self.track_duration = Some(chosen);
        self.current_audio = Some(audio);
        self.started_at = None;
        self.play_span = None;
        self.looping = false;
        self.loop_offset = None;
        #[cfg(test)]
        {
            self.elapsed_override = None;
        }
    }

    /// Adjust master output volume for current and future playback.
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        if let Some(sink) = self.sink.as_mut() {
            sink.set_volume(self.volume);
        }
    }

    /// Configure the anti-click fade used for playback edges.
    pub fn set_anti_clip_settings(&mut self, enabled: bool, fade_ms: f32) {
        self.anti_clip_enabled = enabled;
        self.anti_clip_fade = duration_from_secs_f32(fade_ms / 1000.0);
    }

    /// Stop any active playback.
    pub fn stop(&mut self) {
        self.fade_out_current_sink(self.anti_clip_fade());
        self.started_at = None;
        self.play_span = None;
        self.looping = false;
        self.loop_offset = None;
        #[cfg(test)]
        {
            self.elapsed_override = None;
        }
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
        self.start_with_span(bounded_start, bounded_end, duration, looped)
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

        self.fade_out_current_sink(self.anti_clip_fade());

        let fade = fade_duration(duration, self.anti_clip_fade());
        let source = decoder_from_bytes(bytes)?;
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
        let format = (repeated.sample_rate(), repeated.channels());
        let handle = FadeOutHandle::new();
        sink.append(FadeOutOnRequest::new(repeated, handle.clone()));
        sink.play();

        self.started_at = Some(Instant::now());
        self.play_span = Some((0.0, duration));
        self.looping = true;
        self.loop_offset = Some(offset);
        self.sink = Some(sink);
        self.fade_out = Some(handle);
        self.sink_format = Some(format);
        #[cfg(test)]
        {
            self.elapsed_override = None;
        }
        Ok(())
    }

    /// Current playback progress as a 0-1 fraction.
    pub fn progress(&self) -> Option<f32> {
        let duration = self.track_duration?;
        let started_at = self.started_at?;
        if duration <= 0.0 {
            return None;
        }

        let elapsed = self.elapsed_since(started_at);
        let (span_start, span_end) = self.play_span.unwrap_or((0.0, duration));
        let span_length_secs = (span_end - span_start).max(f32::EPSILON);
        let span_length = duration_from_secs_f32(span_length_secs);
        if span_length.is_zero() {
            return None;
        }

        let base_offset = if self.looping {
            duration_from_secs_f32(self.loop_offset.unwrap_or(0.0))
        } else {
            Duration::ZERO
        };
        let within_span = if self.looping {
            duration_mod(base_offset.saturating_add(elapsed), span_length)
        } else {
            elapsed.min(span_length)
        };
        let absolute_secs = span_start as f64 + within_span.as_secs_f64();
        Some(((absolute_secs / duration as f64) as f32).clamp(0.0, 1.0))
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

    #[cfg(test)]
    pub(crate) fn play_span(&self) -> Option<(f32, f32)> {
        self.play_span
    }

    #[cfg(test)]
    pub(crate) fn track_duration(&self) -> Option<f32> {
        self.track_duration
    }

    /// Remaining wall-clock time until the current loop iteration finishes.
    pub fn remaining_loop_duration(&self) -> Option<Duration> {
        if !self.looping {
            return None;
        }
        let started_at = self.started_at?;
        let (start, end) = self.play_span?;
        let span_length_secs = (end - start).max(f32::EPSILON);
        let span_length = duration_from_secs_f32(span_length_secs);
        if span_length.is_zero() {
            return None;
        }
        let elapsed = self.elapsed_since(started_at);
        let base_offset = duration_from_secs_f32(self.loop_offset.unwrap_or(0.0));
        let elapsed_in_span = duration_mod(base_offset.saturating_add(elapsed), span_length);
        Some(span_length.saturating_sub(elapsed_in_span))
    }

    fn elapsed_since(&self, started_at: Instant) -> Duration {
        #[cfg(test)]
        if let Some(override_elapsed) = self.elapsed_override {
            return override_elapsed;
        }
        started_at.elapsed()
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
        let fade = fade_duration(span_length, self.anti_clip_fade());

        self.fade_out_current_sink(self.anti_clip_fade());

        let mut source = decoder_from_bytes(bytes)?;
        source
            .try_seek(Duration::from_secs_f32(bounded_start))
            .map_err(map_seek_error)?;
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
        let format = (final_source.sample_rate(), final_source.channels());
        let handle = FadeOutHandle::new();

        let sink = Sink::connect_new(self.stream.mixer());
        sink.set_volume(self.volume);
        sink.append(FadeOutOnRequest::new(final_source, handle.clone()));
        sink.play();
        self.started_at = Some(Instant::now());
        self.play_span = Some((bounded_start, bounded_start + span_length));
        self.looping = looped;
        self.sink = Some(sink);
        self.fade_out = Some(handle);
        self.sink_format = Some(format);
        #[cfg(test)]
        {
            self.elapsed_override = None;
        }
        Ok(())
    }

    fn audio_bytes(&self) -> Result<Arc<[u8]>, String> {
        self.current_audio
            .as_ref()
            .cloned()
            .ok_or_else(|| "Load a .wav file first".to_string())
    }

    fn anti_clip_fade(&self) -> Duration {
        if self.anti_clip_enabled {
            self.anti_clip_fade
        } else {
            Duration::ZERO
        }
    }

    #[cfg(test)]
    pub(crate) fn span_sample_count(
        bytes: Arc<[u8]>,
        start_seconds: f32,
        end_seconds: f32,
    ) -> Result<(usize, u32, u16), String> {
        let mut source = decoder_from_bytes(bytes)?;
        source
            .try_seek(Duration::from_secs_f32(start_seconds))
            .map_err(map_seek_error)?;
        let span_length = (end_seconds - start_seconds).max(0.001);
        let fade = fade_duration(span_length, DEFAULT_ANTI_CLIP_FADE);
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
    fn fade_out_current_sink(&mut self, fade: Duration) {
        let Some(sink) = self.sink.take() else {
            return;
        };
        let handle = self.fade_out.take();
        let format = self.sink_format.take();

        let Some(handle) = handle else {
            sink.stop();
            return;
        };
        let Some((sample_rate, _channels)) = format else {
            sink.stop();
            return;
        };
        if fade.is_zero() {
            sink.stop();
            return;
        }
        let fade_frames = fade_frames_for_duration(sample_rate, fade);
        handle.request_fade_out_frames(fade_frames);
        sink.detach();
    }

    /// Active output configuration after initialization.
    pub fn output_details(&self) -> &ResolvedOutput {
        &self.output
    }

    #[cfg(test)]
    /// Build a looped playing instance for tests that need an active sink.
    pub fn playing_for_tests() -> Option<Self> {
        use rodio::source::SineWave;

        let outcome = open_output_stream(&AudioOutputConfig::default()).ok()?;
        let sink = rodio::Sink::connect_new(outcome.stream.mixer());
        // Loop the tone so playback stays active long enough for UI/controller tests to observe it.
        let source = SineWave::new(220.0).repeat_infinite();
        let format = (source.sample_rate(), source.channels());
        let handle = FadeOutHandle::new();
        sink.append(FadeOutOnRequest::new(source, handle.clone()));
        sink.play();
        Some(Self {
            stream: outcome.stream,
            sink: Some(sink),
            fade_out: Some(handle),
            sink_format: Some(format),
            current_audio: None,
            track_duration: Some(1.0),
            started_at: Some(Instant::now()),
            play_span: Some((0.0, 1.0)),
            looping: true,
            loop_offset: Some(0.0),
            volume: 1.0,
            anti_clip_enabled: true,
            anti_clip_fade: DEFAULT_ANTI_CLIP_FADE,
            output: outcome.resolved,
            elapsed_override: None,
        })
    }
}
