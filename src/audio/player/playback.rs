use std::time::Duration;

use rodio::Source;

use super::super::fade::{EdgeFade, fade_duration};
use super::super::mixer::{decoder_from_bytes, map_seek_error};
use super::AudioPlayer;

impl AudioPlayer {
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
        let (bounded_start, bounded_end, duration) = self.normalized_span(start, end)?;
        self.loop_offset = None;
        self.start_with_span(bounded_start, bounded_end, duration, looped)
    }

    /// Loop a selection while starting playback at an offset within the selection.
    pub fn play_looped_range_from(
        &mut self,
        start: f32,
        end: f32,
        offset: f32,
    ) -> Result<(), String> {
        let (bounded_start, bounded_end, duration) = self.normalized_span(start, end)?;
        let clamped_offset = offset.clamp(start.min(end), start.max(end));
        let offset_seconds = (clamped_offset * duration - bounded_start).max(0.0);
        self.start_with_looped_span_offset(bounded_start, bounded_end, duration, offset_seconds)
    }

    /// Loop the full track while starting playback at the given normalized position.
    pub fn play_full_wrapped_from(&mut self, start: f32) -> Result<(), String> {
        let duration = self
            .track_duration
            .ok_or_else(|| "Load a .wav file first".to_string())?;
        let bytes = self.audio_bytes()?;
        if duration <= 0.0 {
            return Err("Load a .wav file first".into());
        }

        self.fade_out_current_sink(self.anti_clip_fade());

        let source = decoder_from_bytes(bytes)?;
        let aligned_duration = Self::aligned_span_seconds(duration, source.sample_rate());
        let offset = (start.clamp(0.0, 1.0) * duration).min(aligned_duration);
        let fade = fade_duration(aligned_duration, self.anti_clip_fade());
        let limited = source
            .take_duration(Duration::from_secs_f32(aligned_duration))
            .buffered();
        let faded = EdgeFade::new(limited, fade);
        let repeated = faded
            .repeat_infinite()
            .skip_duration(Duration::from_secs_f32(offset));

        let (sink, handle, format) = self.build_sink_with_fade(repeated);
        self.started_at = Some(std::time::Instant::now());
        self.play_span = Some((0.0, aligned_duration));
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
        let mut bounded_start = start_seconds.clamp(0.0, duration);
        let mut bounded_end = end_seconds.clamp(bounded_start, duration);
        if let Some(rate) = self.sample_rate {
            bounded_start = self.frame_align(bounded_start, rate);
            bounded_end = self.frame_align(bounded_end, rate);
        }
        let span_length = (bounded_end - bounded_start).max(0.001);

        self.fade_out_current_sink(self.anti_clip_fade());

        let mut source = decoder_from_bytes(bytes)?;
        let aligned_span = if looped {
            Self::aligned_span_seconds(span_length, source.sample_rate())
        } else {
            span_length
        };
        source
            .try_seek(Duration::from_secs_f32(bounded_start))
            .map_err(map_seek_error)?;
        let fade = fade_duration(aligned_span, self.anti_clip_fade());
        let limited = source
            .take_duration(Duration::from_secs_f32(aligned_span))
            .buffered();
        let faded = EdgeFade::new(limited, fade);

        let final_source: Box<dyn Source<Item = f32> + Send> = if looped {
            Box::new(faded.repeat_infinite())
        } else {
            Box::new(faded)
        };
        let (sink, handle, format) = self.build_sink_with_fade(final_source);
        self.started_at = Some(std::time::Instant::now());
        self.play_span = Some((bounded_start, bounded_start + aligned_span));
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

    fn start_with_looped_span_offset(
        &mut self,
        start_seconds: f32,
        end_seconds: f32,
        duration: f32,
        offset_seconds: f32,
    ) -> Result<(), String> {
        let bytes = self.audio_bytes()?;
        if duration <= 0.0 {
            return Err("Load a .wav file first".into());
        }
        let rate = self.sample_rate;
        let mut start_seconds = start_seconds;
        let mut end_seconds = end_seconds;
        let mut offset_seconds = offset_seconds;
        if let Some(rate) = rate {
            start_seconds = self.frame_align(start_seconds, rate);
            end_seconds = self.frame_align(end_seconds, rate);
            offset_seconds = self.frame_align(offset_seconds, rate);
        }
        let span_length = (end_seconds - start_seconds).max(0.001);
        self.fade_out_current_sink(self.anti_clip_fade());
        let mut source = decoder_from_bytes(bytes)?;
        source
            .try_seek(Duration::from_secs_f32(start_seconds))
            .map_err(map_seek_error)?;
        let aligned_span = Self::aligned_span_seconds(span_length, source.sample_rate());
        let fade = fade_duration(aligned_span, self.anti_clip_fade());
        let limited = source
            .take_duration(Duration::from_secs_f32(aligned_span))
            .buffered();
        let faded = EdgeFade::new(limited, fade);
        let offset = offset_seconds.clamp(0.0, aligned_span);
        let repeated = faded
            .repeat_infinite()
            .skip_duration(Duration::from_secs_f32(offset));
        self.start_looped_sink(repeated, start_seconds, aligned_span, offset)
    }

    fn aligned_span_seconds(span_length: f32, sample_rate: u32) -> f32 {
        if sample_rate == 0 {
            return span_length;
        }
        let frames = (span_length * sample_rate as f32).floor();
        let frames = if frames < 1.0 { 1.0 } else { frames };
        frames / sample_rate as f32
    }

    fn start_looped_sink<S: Source<Item = f32> + Send + 'static>(
        &mut self,
        source: S,
        span_start: f32,
        span_length: f32,
        offset: f32,
    ) -> Result<(), String> {
        let (sink, handle, format) = self.build_sink_with_fade(source);
        self.started_at = Some(std::time::Instant::now());
        self.play_span = Some((span_start, span_start + span_length));
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

    #[cfg(test)]
    pub(crate) fn aligned_span_seconds_for_tests(span_length: f32, sample_rate: u32) -> f32 {
        Self::aligned_span_seconds(span_length, sample_rate)
    }

    fn frame_align(&self, seconds: f32, sample_rate: u32) -> f32 {
        if sample_rate == 0 {
            return seconds;
        }
        let frames = (seconds * sample_rate as f32).round();
        frames / sample_rate as f32
    }
}
