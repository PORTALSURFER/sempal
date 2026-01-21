use std::time::Duration;

use crate::audio::{AsyncSource, Source};
use crate::audio::SamplesBuffer;

use super::super::fade::{EdgeFade, fade_duration};
use super::super::mixer::{decoder_from_bytes, map_seek_error};
use super::{AudioPlayer, EditFadeSource};

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
        let sample_rate = source.sample_rate();
        let channels = source.channels();
        let aligned_duration = Self::aligned_span_duration(duration, sample_rate);
        
        let mut samples = Vec::new();
        let mut limited = source.take_duration(aligned_duration);
        while let Some(s) = limited.next() {
            samples.push(s);
        }
        // Ensure even count for stereo
        if channels == 2 && samples.len() % 2 != 0 {
            samples.push(0.0);
        }
        
        let buffer = SamplesBuffer::new(channels, sample_rate, samples);
        let offset = (start.clamp(0.0, 1.0) * duration).min(aligned_duration.as_secs_f32());
        let offset_dur = Self::aligned_offset_duration(offset, sample_rate);
        let repeated = buffer
            .repeat_infinite()
            .skip_duration(offset_dur);

        let (handle, format) = self.build_sink_with_fade(repeated);
        self.started_at = Some(std::time::Instant::now());
        self.play_span = Some((0.0, aligned_duration.as_secs_f32()));
        self.looping = true;
        self.loop_offset = Some(offset);
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
        let aligned_span_dur = if looped {
            Self::aligned_span_duration(span_length, source.sample_rate())
        } else {
             // For non-looped, we can just use the span length directly, 
             // or use aligned_span_duration if we want consistent cutting? 
             // Rodio take_duration takes Duration. 
             // Let's use duration from secs f32 if not looping, or just use aligned for consistency?
             // The original used `aligned_span` (f32) for both.
             Duration::from_secs_f32(span_length)
        };
        
        // We still need f32 for fade setup
        let aligned_span_sec = if looped {
             aligned_span_dur.as_secs_f32()
        } else {
             span_length
        };

        let seek_dur = Self::aligned_seek_duration(bounded_start, source.sample_rate());
        let sample_rate = source.sample_rate();
        let channels = source.channels();
        
        tracing::debug!(
            "Loop setup: rate={}, channels={}, span_sec={:.6}, looped={}",
            sample_rate, channels, aligned_span_sec, looped
        );
        
        source
            .try_seek(seek_dur)
            .map_err(map_seek_error)?;
        
        // For looped playback, calculate duration that guarantees even sample count
        let frames = (aligned_span_sec * sample_rate as f32).round() as u64;
        let frames_adjusted = if looped && channels == 2 && frames % 2 != 0 { 
            frames + 1 
        } else { 
            frames 
        };
        
        let loop_duration = if looped {
            // Convert to duration using floor division
            let nanos = (frames_adjusted * 1_000_000_000) / sample_rate as u64;
            let _needed_nanos = ((1.0f64 / 44100.0f64) * 1_000_000_000.0f64).ceil() as u64; // 22676?
            let duration = Duration::from_nanos(nanos);
            
            tracing::debug!(
                "Loop duration calc: frames={}, adjusted={}, nanos={}, samples_expected={}",
                frames, frames_adjusted, nanos, frames_adjusted * channels as u64
            );
            
            duration
        } else {
            aligned_span_dur
        };
        
        let fade = fade_duration(aligned_span_sec, self.anti_clip_fade());
        let expected_samples = frames_adjusted * channels as u64;
        
        // For looped playback, pre-decode the segment into a memory buffer
        // to ensure perfect sample alignment and avoid stereo channel swap.
        let final_source: Box<dyn Source<Item = f32> + Send> = if looped {
            let mut limited = source.take_duration(loop_duration);
            let mut samples = Vec::with_capacity(expected_samples as usize);
            for _ in 0..expected_samples {
                if let Some(s) = limited.next() {
                    samples.push(s);
                } else {
                    break;
                }
            }
            // Ensure exactly expected_samples (even for stereo)
            while samples.len() < expected_samples as usize {
                samples.push(0.0);
            }
            samples.truncate(expected_samples as usize);
            
            let buffer = SamplesBuffer::new(channels, sample_rate, samples);
            let diagnostic = crate::audio::loop_diagnostic::LoopDiagnostic::new(
                buffer.repeat_infinite(),
                expected_samples,
            );
            let editable = EditFadeSource::new_looped(
                diagnostic,
                self.edit_fade_handle.clone(),
                bounded_start,
                frames_adjusted,
                0,
            );
            Box::new(editable)
        } else {
            let mut async_source = AsyncSource::new(source);
            async_source.prefill();
            let limited = async_source.take_duration(loop_duration).buffered();
            let editable = EditFadeSource::new(limited, self.edit_fade_handle.clone(), bounded_start);
            let faded = EdgeFade::new(editable, fade);
            Box::new(faded)
        };
        let (handle, format) = self.build_sink_with_fade(final_source);
        self.started_at = Some(std::time::Instant::now());
        self.play_span = Some((bounded_start, bounded_start + aligned_span_sec));
        self.looping = looped;
        self.loop_offset = None;
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
        let seek_dur = Self::aligned_seek_duration(start_seconds, source.sample_rate());
        let sample_rate = source.sample_rate();
        let channels = source.channels();
        source
            .try_seek(seek_dur)
            .map_err(map_seek_error)?;
        
        let aligned_span = Self::aligned_span_duration(span_length, sample_rate);
        let aligned_span_sec = aligned_span.as_secs_f32();
        
        // Calculate duration that guarantees even sample count for stereo
        let frames = (aligned_span_sec * sample_rate as f32).round() as u64;
        // For stereo, ensure we have an even number of frames
        let frames = if channels == 2 && frames % 2 != 0 { frames + 1 } else { frames };
        let loop_duration = Duration::from_nanos((frames * 1_000_000_000) / sample_rate as u64);
        
        let _fade = fade_duration(aligned_span_sec, self.anti_clip_fade());
        let expected_samples = frames * channels as u64;
        
        let mut limited = source.take_duration(loop_duration);
        let mut samples = Vec::with_capacity(expected_samples as usize);
        for _ in 0..expected_samples {
            if let Some(s) = limited.next() {
                samples.push(s);
            } else {
                break;
            }
        }
        while samples.len() < expected_samples as usize {
            samples.push(0.0);
        }
        samples.truncate(expected_samples as usize);
        
        let buffer = SamplesBuffer::new(channels, sample_rate, samples);
        let final_source: Box<dyn Source<Item = f32> + Send> = {
            let offset_dur = Self::aligned_offset_duration(offset_seconds, sample_rate);
            let offset_frames = (offset_seconds * sample_rate as f32).floor().max(0.0) as u64;
            let editable = EditFadeSource::new_looped(
                buffer,
                self.edit_fade_handle.clone(),
                start_seconds,
                frames,
                offset_frames,
            );
            let repeated = editable.repeat_infinite().skip_duration(offset_dur);
            let diagnostic = crate::audio::loop_diagnostic::LoopDiagnostic::new(
                repeated,
                expected_samples,
            );
            Box::new(diagnostic)
        };
        
        let (handle, format) = self.build_sink_with_fade(final_source);
        self.started_at = Some(std::time::Instant::now());
        self.play_span = Some((start_seconds, end_seconds));
        self.looping = true;
        self.loop_offset = Some(offset_seconds);
        self.fade_out = Some(handle);
        self.sink_format = Some(format);
        #[cfg(test)]
        {
            self.elapsed_override = None;
        }
        Ok(())
    }



    /// Calculate a Duration that covers at least `frames` full frames.
    /// This uses u64 arithmetic to avoid f32 precision loss which can cause audio decoders to drop one sample (half a frame)
    /// in stereo sources, leading to channel swapping.
    pub(crate) fn aligned_span_duration(span_seconds: f32, sample_rate: u32) -> Duration {
        if sample_rate == 0 {
            return Duration::from_secs_f32(span_seconds);
        }
        let frames = (span_seconds * sample_rate as f32).round().max(1.0) as u64;
        let nanos = (frames * 1_000_000_000) / sample_rate as u64;
        Duration::from_nanos(nanos)
    }

    /// Calculate a Duration that corresponds to the exact number of frames for `seconds`.
    /// This allows 0 duration.
    fn aligned_offset_duration(seconds: f32, sample_rate: u32) -> Duration {
        if sample_rate == 0 {
            return Duration::from_secs_f32(seconds);
        }
        let frames = (seconds * sample_rate as f32).round().max(0.0) as u64;
         // Use the same ceiling logic? No, for offset, we want to be exact or slightly padded?
         // If we skip 1 frame, we want to skip exactly 1 frame.
         // And since duration handling might be frame-based, we need to be careful.
         // Based on analysis, CEIL causes us to skip into the next frame (Sample 2), causing stereo swap.
         // We must use FLOOR (integer truncation) to ensure we stop BEFORE the next frame starts.
         // nanos = frames * 1e9 / rate.
         let nanos = (frames * 1_000_000_000) / sample_rate as u64;
         Duration::from_nanos(nanos)
    }

    /// Calculate a Duration for seeking that aligns exactly with frame boundaries.
    /// Uses floor division to prevent seeking past the intended frame.
    fn aligned_seek_duration(seconds: f32, sample_rate: u32) -> Duration {
        if sample_rate == 0 {
            return Duration::from_secs_f32(seconds);
        }
        let frames = (seconds * sample_rate as f32).round().max(0.0) as u64;
        let nanos = (frames * 1_000_000_000) / sample_rate as u64;
        Duration::from_nanos(nanos)
    }

    fn frame_align(&self, seconds: f32, sample_rate: u32) -> f32 {
        if sample_rate == 0 {
            return seconds;
        }
        let frames = (seconds * sample_rate as f32).round();
        frames / sample_rate as f32
    }
}
