use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::audio::Source;

#[cfg(test)]
use super::super::DEFAULT_ANTI_CLIP_FADE;
#[cfg(test)]
use super::super::fade::{EdgeFade, fade_duration};
use super::super::fade::{FadeOutHandle, FadeOutOnRequest, fade_frames_for_duration};
#[cfg(test)]
use crate::audio::mixer::{decoder_from_bytes, map_seek_error};
use super::AudioPlayer;

impl AudioPlayer {
    pub(super) fn effective_volume(&self) -> f32 {
        let volume = self.volume * self.playback_gain;
        if volume.is_finite() {
            volume.max(0.0)
        } else {
            0.0
        }
    }

    pub(super) fn reset_playback_state(&mut self) {
        self.started_at = None;
        self.play_span = None;
        self.looping = false;
        self.loop_offset = None;
        #[cfg(test)]
        {
            self.elapsed_override = None;
        }
    }

    pub(super) fn build_sink_with_fade<S: Source + Send + 'static>(
        &mut self,
        source: S,
    ) -> (FadeOutHandle, (u32, u16)) {
        let _volume = self.effective_volume();
        let format = (source.sample_rate(), source.channels());
        let handle = FadeOutHandle::new();
        
        {
            let mut state = self.stream.state.lock().unwrap();
            state.sources.push((Box::new(FadeOutOnRequest::new(source, handle.clone())), 1.0));
            self.active_sources = state.sources.len();
        }
        
        (handle, format)
    }

    pub fn create_monitor_sink(&self, volume: f32) -> crate::audio::output::MonitorSink {
        crate::audio::output::MonitorSink {
            state: self.stream.state.clone(),
            volume,
        }
    }

    pub(super) fn elapsed_since(&self, started_at: Instant) -> Duration {
        #[cfg(test)]
        if let Some(override_elapsed) = self.elapsed_override {
            return override_elapsed;
        }
        started_at.elapsed()
    }

    pub(super) fn audio_bytes(&self) -> Result<Arc<[u8]>, String> {
        self.current_audio
            .as_ref()
            .cloned()
            .ok_or_else(|| "Load a .wav file first".to_string())
    }

    pub(super) fn anti_clip_fade(&self) -> Duration {
        if self.anti_clip_enabled {
            self.anti_clip_fade
        } else {
            Duration::ZERO
        }
    }

    pub(super) fn normalized_span(&self, start: f32, end: f32) -> Result<(f32, f32, f32), String> {
        let duration = self
            .track_duration
            .ok_or_else(|| "Load a .wav file first".to_string())?;
        if duration <= 0.0 {
            return Err("Load a .wav file first".into());
        }
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        let clamped_start = start.clamp(0.0, 1.0) * duration;
        let clamped_end = end.clamp(0.0, 1.0) * duration;
        let mut bounded_start = clamped_start.min(duration);
        let mut bounded_end = clamped_end.min(duration);
        let min_span = self.min_span_seconds.unwrap_or(0.01);
        if bounded_end <= bounded_start {
            bounded_end = (bounded_start + min_span).min(duration);
            if bounded_end <= bounded_start {
                bounded_start = (duration - min_span).max(0.0);
                bounded_end = duration.max(bounded_start + 0.001);
            }
        }
        Ok((bounded_start, bounded_end, duration))
    }

    pub(super) fn fade_out_current_sink(&mut self, fade: Duration) {
        if self.active_sources == 0 {
            return;
        }
        let handle = self.fade_out.take();
        let format = self.sink_format.take();
        self.active_sources = 0;

        let Some(handle) = handle else {
            let mut state = self.stream.state.lock().unwrap();
            state.sources.clear();
            return;
        };
        let Some((sample_rate, _channels)) = format else {
            let mut state = self.stream.state.lock().unwrap();
            state.sources.clear();
            return;
        };
        if fade.is_zero() {
            let mut state = self.stream.state.lock().unwrap();
            state.sources.clear();
            return;
        }
        let fade_frames = fade_frames_for_duration(sample_rate, fade);
        handle.request_fade_out_frames(fade_frames);
    }

    #[cfg(test)]
    pub(crate) fn span_sample_count(
        bytes: Arc<[u8]>,
        start_seconds: f32,
        end_seconds: f32,
    ) -> Result<(usize, u32, u16), String> {
        let source = decoder_from_bytes(bytes)?;
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
}
