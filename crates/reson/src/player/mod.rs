use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::fade::FadeOutHandle;
use super::output::{CpalAudioStream, ResolvedOutput};

mod edit_fade_impl;
mod helpers;
mod metronome;
mod playback;
mod playback_span_handle;
mod playback_span_plan;
mod progress;
mod runtime;
mod state;
pub(crate) use edit_fade_impl::{EditFadeHandle, EditFadeSource};
pub use edit_fade_impl::{EditFadeRange, FadeParams};
pub use metronome::PlaybackMetronomeConfig;
pub(crate) use playback_span_handle::PlaybackSpanHandle;
pub use playback_span_plan::{
    PlaybackChannelLayout, PlaybackSeekBehavior, PlaybackSourceIdentity, PlaybackSourceKind,
    PlaybackSpanPlan, PlaybackSpanPlanError, PlaybackSpanRequest,
};
pub use runtime::{
    PlaybackRequestId, PlaybackRuntime, PlaybackRuntimeCancellation, PlaybackRuntimeConfig,
    PlaybackRuntimeEvent, PlaybackRuntimeEventIter, PlaybackRuntimeEventReceiver,
    PlaybackRuntimeEventRecvError, PlaybackRuntimeEventRecvTimeoutError,
    PlaybackRuntimeGainNormalization, PlaybackRuntimeHandle, PlaybackRuntimeMode,
    PlaybackRuntimeProgress, PlaybackRuntimeReplacePolicy, PlaybackRuntimeRequest,
    PlaybackRuntimeSource, PlaybackRuntimeSpanUpdate, PlaybackRuntimeStarted,
    PlaybackRuntimeStreamPolicy, PlaybackRuntimeSubmitError,
};

#[derive(Clone)]
pub(crate) enum AudioPlaybackSource {
    Bytes(Arc<[u8]>),
    File(PathBuf),
    InterleavedF32File { path: PathBuf, sample_count: u64 },
}

impl AudioPlaybackSource {
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::Bytes(_) => "bytes",
            Self::File(_) => "file",
            Self::InterleavedF32File { .. } => "interleaved_f32_file",
        }
    }

    pub(crate) fn identity(&self) -> PlaybackSourceIdentity {
        match self {
            Self::Bytes(_) => PlaybackSourceIdentity::new(PlaybackSourceKind::Bytes, None),
            Self::File(_) => PlaybackSourceIdentity::new(PlaybackSourceKind::File, None),
            Self::InterleavedF32File { sample_count, .. } => PlaybackSourceIdentity::new(
                PlaybackSourceKind::InterleavedF32File,
                Some(*sample_count),
            ),
        }
    }
}

/// Simple audio helper that plays a loaded wav buffer and reports progress.
pub struct AudioPlayer {
    pub(crate) edit_fade_handle: EditFadeHandle,
    stream: CpalAudioStream,
    active_sources: usize,
    fade_out: Option<FadeOutHandle>,
    sink_format: Option<(u32, u16)>,
    current_audio: Option<AudioPlaybackSource>,
    playback_samples: Option<Arc<[f32]>>,
    track_duration: Option<f32>,
    track_total_frames: Option<u64>,
    track_channels: Option<u16>,
    sample_rate: Option<u32>,
    started_at: Option<Instant>,
    play_span: Option<(f32, f32)>,
    play_span_frames: Option<(u64, u64)>,
    looping: bool,
    active_playback_span: Option<PlaybackSpanHandle>,
    loop_offset: Option<f32>,
    loop_offset_frames: Option<u64>,
    volume: f32,
    playback_gain: f32,
    anti_clip_enabled: bool,
    anti_clip_fade: Duration,
    min_span_seconds: Option<f32>,
    stream_policy: PlaybackRuntimeStreamPolicy,
    output: ResolvedOutput,
    #[cfg(test)]
    elapsed_override: Option<Duration>,
}
