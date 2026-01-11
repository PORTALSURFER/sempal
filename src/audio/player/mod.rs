use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::info;

use super::fade::FadeOutHandle;
use super::output::{CpalAudioStream, ResolvedOutput};

mod helpers;
mod playback;
mod progress;
mod state;

/// Simple audio helper that plays a loaded wav buffer and reports progress.
pub struct AudioPlayer {
    stream: CpalAudioStream,
    active_sources: usize,
    fade_out: Option<FadeOutHandle>,
    sink_format: Option<(u32, u16)>,
    current_audio: Option<Arc<[u8]>>,
    track_duration: Option<f32>,
    sample_rate: Option<u32>,
    started_at: Option<Instant>,
    play_span: Option<(f32, f32)>,
    looping: bool,
    loop_offset: Option<f32>,
    volume: f32,
    playback_gain: f32,
    anti_clip_enabled: bool,
    anti_clip_fade: Duration,
    min_span_seconds: Option<f32>,
    output: ResolvedOutput,
    #[cfg(test)]
    elapsed_override: Option<Duration>,
}
