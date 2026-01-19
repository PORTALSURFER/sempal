use std::time::Duration;

use super::super::routing::{duration_from_secs_f32, duration_mod};
use super::AudioPlayer;

impl AudioPlayer {
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
        self.stream.active_source_count() > 0 && self.started_at.is_some()
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

    /// Returns and clears the last error from the audio stream.
    pub fn take_error(&mut self) -> Option<String> {
        self.stream.take_error()
    }
}
