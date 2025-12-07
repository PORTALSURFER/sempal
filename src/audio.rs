use std::{
    io::Cursor,
    time::{Duration, Instant},
};

use rodio::{Decoder, OutputStream, OutputStreamBuilder, Sink, Source};

/// Simple audio helper that plays a loaded wav buffer and reports progress.
pub struct AudioPlayer {
    stream: OutputStream,
    sink: Option<Sink>,
    current_audio: Option<Vec<u8>>,
    track_duration: Option<f32>,
    started_at: Option<Instant>,
    play_span: Option<(f32, f32)>,
    looping: bool,
    volume: f32,
}

const SEGMENT_FADE: Duration = Duration::from_millis(5);

impl AudioPlayer {
    /// Create a new audio player using the default output device.
    pub fn new() -> Result<Self, String> {
        let stream = OutputStreamBuilder::open_default_stream()
            .map_err(|error| format!("Audio init failed: {error}"))?;
        Ok(Self {
            stream,
            sink: None,
            current_audio: None,
            track_duration: None,
            started_at: None,
            play_span: None,
            looping: false,
            volume: 1.0,
        })
    }

    /// Store audio bytes and duration for later playback.
    pub fn set_audio(&mut self, data: Vec<u8>, duration: f32) {
        self.current_audio = Some(data);
        self.track_duration = Some(duration);
        self.started_at = None;
        self.play_span = None;
        self.looping = false;
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
        if let Some(sink) = self.sink.take() {
            sink.stop();
        }
        self.started_at = None;
        self.play_span = None;
        self.looping = false;
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
        self.start_with_span(clamped_start, bounded_end, duration, looped)
    }

    /// Current playback progress as a 0-1 fraction.
    pub fn progress(&self) -> Option<f32> {
        let duration = self.track_duration?;
        let started_at = self.started_at?;
        let elapsed = started_at.elapsed().as_secs_f32();
        normalized_progress(self.play_span, duration, elapsed, self.looping)
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
        let bytes = self
            .current_audio
            .as_ref()
            .cloned()
            .ok_or_else(|| "Load a .wav file first".to_string())?;
        if duration <= 0.0 {
            return Err("Load a .wav file first".into());
        }
        let bounded_start = start_seconds.clamp(0.0, duration);
        let bounded_end = end_seconds.clamp(bounded_start, duration);
        let span_length = (bounded_end - bounded_start).max(0.001);

        self.fade_out_current_sink();

        let mut source = Decoder::new(Cursor::new(bytes))
            .map_err(|error| format!("Audio decode failed: {error}"))?;
        source
            .try_seek(Duration::from_secs_f32(bounded_start))
            .map_err(Self::map_seek_error)?;
        let limited = source
            .fade_in(SEGMENT_FADE)
            .take_duration(Duration::from_secs_f32(span_length))
            .buffered();
        let faded = EdgeFade::new(limited, SEGMENT_FADE);

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

    /// Mute and stop the current sink without blocking the UI thread.
    fn fade_out_current_sink(&mut self) {
        if let Some(sink) = self.sink.take() {
            sink.set_volume(0.0);
            sink.stop();
        }
    }
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
        Self {
            inner,
            fade_secs,
            total_secs,
            fade_out_start,
            sample_rate,
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
        let time = if self.sample_rate > 0 {
            self.samples_emitted as f32 / self.sample_rate as f32
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
mod tests {
    use super::*;

    #[test]
    fn normalized_progress_respects_span() {
        let progress = normalized_progress(Some((2.0, 4.0)), 10.0, 1.0, false);
        assert_eq!(progress, Some(0.3));
    }

    #[test]
    fn normalized_progress_handles_elapsed_beyond_span() {
        let progress = normalized_progress(Some((2.0, 4.0)), 10.0, 3.5, false);
        assert_eq!(progress, Some(0.4));
    }

    #[test]
    fn normalized_progress_loops_within_range() {
        let progress = normalized_progress(Some((2.0, 4.0)), 10.0, 5.5, true);
        assert!((progress.unwrap() - 0.35).abs() < 0.0001);
    }

    #[test]
    fn normalized_progress_handles_full_track() {
        let progress = normalized_progress(None, 8.0, 3.0, false);
        assert_eq!(progress, Some(0.375));
    }

    #[test]
    fn remaining_loop_duration_reports_time_left_in_cycle() {
        let Ok(stream) = rodio::OutputStreamBuilder::open_default_stream() else {
            return;
        };
        let started_at = Instant::now() - Duration::from_secs_f32(0.75);
        let player = AudioPlayer {
            stream,
            sink: None,
            current_audio: None,
            track_duration: Some(8.0),
            started_at: Some(started_at),
            play_span: Some((1.0, 3.0)),
            looping: true,
            volume: 1.0,
        };

        let remaining = player.remaining_loop_duration().unwrap();
        assert!((remaining.as_secs_f32() - 1.25).abs() < 0.1);
    }

    #[test]
    fn remaining_loop_duration_none_when_not_looping() {
        let Ok(stream) = rodio::OutputStreamBuilder::open_default_stream() else {
            return;
        };
        let player = AudioPlayer {
            stream,
            sink: None,
            current_audio: None,
            track_duration: Some(8.0),
            started_at: Some(Instant::now()),
            play_span: Some((1.0, 3.0)),
            looping: false,
            volume: 1.0,
        };

        assert!(player.remaining_loop_duration().is_none());
    }

    #[test]
    fn normalized_progress_returns_none_when_invalid_duration() {
        assert_eq!(normalized_progress(None, 0.0, 1.0, false), None);
        assert_eq!(normalized_progress(None, -1.0, 1.0, false), None);
    }

    #[test]
    fn play_range_accepts_zero_width_request() {
        let Ok(stream) = rodio::OutputStreamBuilder::open_default_stream() else {
            // Skip when no audio device is available in the test environment.
            return;
        };
        let mut player = AudioPlayer {
            stream,
            sink: None,
            current_audio: None,
            track_duration: None,
            started_at: None,
            play_span: None,
            looping: false,
            volume: 1.0,
        };
        // A minimal valid 1s mono wav (header only, no samples needed for the span logic).
        let bytes = vec![
            0x52, 0x49, 0x46, 0x46, 0x24, 0x80, 0x00, 0x00, 0x57, 0x41, 0x56, 0x45, 0x66, 0x6D,
            0x74, 0x20, 0x10, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x44, 0xAC, 0x00, 0x00,
            0x88, 0x58, 0x01, 0x00, 0x02, 0x00, 0x10, 0x00, 0x64, 0x61, 0x74, 0x61, 0x00, 0x80,
            0x00, 0x00, 0x00, 0x00,
        ];
        player.set_audio(bytes, 1.0);
        assert!(player.play_range(0.5, 0.5, false).is_ok());
    }
}
