use std::sync::Arc;
#[cfg(test)]
use std::time::{Duration, Instant};

#[cfg(test)]
use rodio::Source;
 
use super::super::DEFAULT_ANTI_CLIP_FADE;
use super::super::output::{CpalAudioStream, AudioOutputConfig, ResolvedOutput, open_output_stream};
use super::super::routing::duration_from_secs_f32;
use super::AudioPlayer;

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
            active_sources: 0,
            fade_out: None,
            sink_format: None,
            current_audio: None,
            track_duration: None,
            sample_rate: None,
            started_at: None,
            play_span: None,
            looping: false,
            loop_offset: None,
            volume: 1.0,
            playback_gain: 1.0,
            anti_clip_enabled: true,
            anti_clip_fade: DEFAULT_ANTI_CLIP_FADE,
            min_span_seconds: None,
            output: outcome.resolved,
            #[cfg(test)]
            elapsed_override: None,
        })
    }

    /// Store audio bytes and duration for later playback.
    pub fn set_audio(&mut self, data: Vec<u8>, duration: f32) {
        use super::super::mixer::{decoder_duration, wav_header_duration, wav_spec_from_bytes};
        let audio = Arc::from(data);
        let provided = duration.max(0.0);
        let fallback = decoder_duration(&audio)
            .or_else(|| wav_header_duration(&audio))
            .unwrap_or(0.0);
        
        let sample_rate = wav_spec_from_bytes(&audio).map(|(_, rate)| rate);
        let chosen = if provided > 0.0 { provided } else { fallback };
        self.track_duration = Some(chosen);
        self.sample_rate = sample_rate;
        self.current_audio = Some(audio);
        self.reset_playback_state();
    }

    /// Adjust master output volume for current and future playback.
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        let effective = self.effective_volume();
        let mut state = self.stream.state.lock().unwrap();
        state.volume = effective;
    }
 
    /// Adjust normalized audition gain for current and future playback.
    pub fn set_playback_gain(&mut self, gain: f32) {
        self.playback_gain = if gain.is_finite() && gain > 0.0 {
            gain
        } else {
            1.0
        };
        let effective = self.effective_volume();
        let mut state = self.stream.state.lock().unwrap();
        state.volume = effective;
    }

    /// Set the minimum span length (in seconds) enforced for playback ranges.
    pub fn set_min_span_seconds(&mut self, min_span: Option<f32>) {
        self.min_span_seconds = min_span.filter(|value| value.is_finite() && *value > 0.0);
    }

    /// Configure the anti-click fade used for playback edges.
    pub fn set_anti_clip_settings(&mut self, enabled: bool, fade_ms: f32) {
        self.anti_clip_enabled = enabled;
        self.anti_clip_fade = duration_from_secs_f32(fade_ms / 1000.0);
    }

    /// Stop any active playback.
    pub fn stop(&mut self) {
        self.fade_out_current_sink(self.anti_clip_fade());
        self.reset_playback_state();
    }

    /// Active output configuration after initialization.
    pub fn output_details(&self) -> &ResolvedOutput {
        &self.output
    }

    #[cfg(test)]
    pub(crate) fn test_with_state(
        stream: CpalAudioStream,
        track_duration: Option<f32>,
        started_at: Option<Instant>,
        play_span: Option<(f32, f32)>,
        looping: bool,
        loop_offset: Option<f32>,
        elapsed_override: Option<Duration>,
    ) -> Self {
        Self {
            stream,
            active_sources: 0,
            fade_out: None,
            sink_format: None,
            current_audio: None,
            track_duration,
            sample_rate: None,
            started_at,
            play_span,
            looping,
            loop_offset,
            volume: 1.0,
            playback_gain: 1.0,
            anti_clip_enabled: true,
            anti_clip_fade: DEFAULT_ANTI_CLIP_FADE,
            min_span_seconds: None,
            output: ResolvedOutput::default(),
            elapsed_override,
        }
    }

    #[cfg(test)]
    /// Build a looped playing instance for tests that need an active sink.
    pub fn playing_for_tests() -> Option<Self> {
        use rodio::source::SineWave;

        let outcome = open_output_stream(&AudioOutputConfig::default()).ok()?;
        let source = SineWave::new(220.0).repeat_infinite();
        let (sink, handle, format) =
            Self::build_sink_with_fade_for_stream(&outcome.stream, 1.0, source);
        // Loop the tone so playback stays active long enough for UI/controller tests to observe it.
        Some(Self {
            stream: outcome.stream,
            active_sources: 1,
            fade_out: Some(handle),
            sink_format: Some(format),
            current_audio: None,
            track_duration: Some(1.0),
            sample_rate: Some(44100),
            started_at: Some(Instant::now()),
            play_span: Some((0.0, 1.0)),
            looping: true,
            loop_offset: Some(0.0),
            volume: 1.0,
            playback_gain: 1.0,
            anti_clip_enabled: true,
            anti_clip_fade: DEFAULT_ANTI_CLIP_FADE,
            min_span_seconds: None,
            output: outcome.resolved,
            elapsed_override: None,
        })
    }
}
