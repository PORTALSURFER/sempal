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

const DEFAULT_ANTI_CLIP_FADE: Duration = Duration::from_millis(2);

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

    fn anti_clip_fade(&self) -> Duration {
        if self.anti_clip_enabled {
            self.anti_clip_fade
        } else {
            Duration::ZERO
        }
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
        let fade = fade_duration(span_length, self.anti_clip_fade());
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
        use rodio::source::{SineWave, Source};

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

fn fade_duration(span_seconds: f32, max_fade: Duration) -> Duration {
    if span_seconds <= 0.0 || max_fade.is_zero() {
        return Duration::ZERO;
    }
    let max_fade_secs = max_fade.as_secs_f32();
    let clamped = max_fade_secs.min(span_seconds * 0.5);
    duration_from_secs_f32(clamped)
}

fn fade_frames_for_duration(sample_rate: u32, fade: Duration) -> u32 {
    if fade.is_zero() || sample_rate == 0 {
        return 1;
    }
    let frames = (fade.as_secs_f64() * sample_rate as f64).ceil();
    frames.clamp(1.0, u32::MAX as f64) as u32
}

#[cfg(test)]
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

fn duration_from_secs_f32(seconds: f32) -> Duration {
    if !seconds.is_finite() || seconds <= 0.0 {
        return Duration::ZERO;
    }
    Duration::from_secs_f64(seconds as f64)
}

fn duration_mod(value: Duration, modulus: Duration) -> Duration {
    let modulus_nanos = modulus.as_nanos();
    if modulus_nanos == 0 {
        return Duration::ZERO;
    }
    let remainder = value.as_nanos() % modulus_nanos;
    let secs = (remainder / 1_000_000_000) as u64;
    let nanos = (remainder % 1_000_000_000) as u32;
    Duration::new(secs, nanos)
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
        if let (Some(total), Some(start)) = (self.total_secs, self.fade_out_start)
            && time > start
        {
            factor *= ((total - time) / self.fade_secs).clamp(0.0, 1.0);
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

#[derive(Clone)]
struct FadeOutHandle {
    requested_frames: Arc<std::sync::atomic::AtomicU32>,
}

impl FadeOutHandle {
    fn new() -> Self {
        Self {
            requested_frames: Arc::new(std::sync::atomic::AtomicU32::new(0)),
        }
    }

    fn request_fade_out_frames(&self, frames: u32) {
        self.requested_frames
            .store(frames.max(1), std::sync::atomic::Ordering::Release);
    }

    fn take_requested_frames(&self) -> Option<u32> {
        let frames = self
            .requested_frames
            .swap(0, std::sync::atomic::Ordering::AcqRel);
        if frames == 0 { None } else { Some(frames) }
    }
}

struct FadeOutOnRequest<S> {
    inner: S,
    handle: FadeOutHandle,
    sample_rate: u32,
    channels: u16,
    samples_emitted: u64,
    pending_frames: Option<u32>,
    fade_start_frame: u64,
    fade_total_frames: u32,
    fading: bool,
}

impl<S> FadeOutOnRequest<S>
where
    S: Source<Item = f32>,
{
    fn new(inner: S, handle: FadeOutHandle) -> Self {
        let sample_rate = inner.sample_rate();
        let channels = inner.channels();
        Self {
            inner,
            handle,
            sample_rate,
            channels,
            samples_emitted: 0,
            pending_frames: None,
            fade_start_frame: 0,
            fade_total_frames: 0,
            fading: false,
        }
    }

    fn current_frame(&self) -> u64 {
        let channels = self.channels.max(1) as u64;
        self.samples_emitted / channels
    }

    fn is_frame_boundary(&self) -> bool {
        let channels = self.channels.max(1) as u64;
        self.samples_emitted % channels == 0
    }

    fn start_fade_if_ready(&mut self) {
        if self.fading {
            return;
        }
        if self.pending_frames.is_none() {
            self.pending_frames = self.handle.take_requested_frames();
        }
        if self.pending_frames.is_none() || !self.is_frame_boundary() {
            return;
        }
        let Some(frames) = self.pending_frames.take() else {
            return;
        };
        self.fading = true;
        self.fade_start_frame = self.current_frame();
        self.fade_total_frames = frames.max(1);
    }

    fn fade_factor(&self) -> f32 {
        if !self.fading {
            return 1.0;
        }
        let current_frame = self.current_frame();
        let offset = current_frame.saturating_sub(self.fade_start_frame);
        let total = self.fade_total_frames.max(1) as u64;
        if offset >= total {
            return 0.0;
        }
        if total <= 1 {
            return 0.0;
        }
        let denom = (total - 1) as f32;
        let progress = (offset as f32 / denom).clamp(0.0, 1.0);
        (1.0 - progress).clamp(0.0, 1.0)
    }
}

impl<S> Iterator for FadeOutOnRequest<S>
where
    S: Source<Item = f32>,
{
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        self.start_fade_if_ready();
        if self.fading && self.is_frame_boundary() {
            let current_frame = self.current_frame();
            let offset = current_frame.saturating_sub(self.fade_start_frame);
            if offset >= self.fade_total_frames as u64 {
                return None;
            }
        }

        let sample = self.inner.next()?;
        let factor = self.fade_factor();
        self.samples_emitted = self.samples_emitted.saturating_add(1);
        Some(sample * factor)
    }
}

impl<S> Source for FadeOutOnRequest<S>
where
    S: Source<Item = f32>,
{
    #[inline]
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }

    #[inline]
    fn channels(&self) -> u16 {
        self.channels
    }

    #[inline]
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    #[inline]
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

#[cfg(test)]
mod tests;
pub use output::{
    AudioDeviceSummary, AudioHostSummary, AudioOutputConfig, AudioOutputError, ResolvedOutput,
    available_devices, available_hosts, open_output_stream, supported_sample_rates,
};
