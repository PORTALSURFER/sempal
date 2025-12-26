use std::sync::Arc;
use std::time::{Duration, Instant};

use rodio::{OutputStream, Sink};

use super::fade::FadeOutHandle;
use super::output::ResolvedOutput;

mod helpers;
mod playback;
mod progress;
mod state;

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
