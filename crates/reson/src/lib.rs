use std::time::Duration;

mod device;
/// Audio input device enumeration and configuration helpers.
pub mod input;
/// Audio output device enumeration and stream helpers.
pub mod output;
/// Recording and input monitoring helpers.
pub mod recording;

mod async_decode;
/// Low-level decoder wrapper for Symphonia.
pub mod decoder;
mod fade;
mod loop_diagnostic;
mod mixer;
mod player;
mod routing;
mod source;
mod telemetry;
mod time_stretch;
mod timebase;
/// WAV header repair helpers for streaming and in-memory decode paths.
pub mod wav_sanitize;

pub use input::{
    AudioInputConfig, AudioInputError, ResolvedInput, ResolvedInputConfig,
    available_input_channel_count, available_input_devices, available_input_hosts,
    resolve_input_stream_config, supported_input_sample_rates,
};
pub use output::{
    AudioDeviceSummary, AudioHostSummary, AudioOutputConfig, AudioOutputError, ResolvedOutput,
    available_devices, available_hosts, open_output_stream, supported_sample_rates,
};
pub use player::{
    AudioPlayer, EditFadeRange, FadeParams, PlaybackChannelLayout, PlaybackMetronomeConfig,
    PlaybackRequestId, PlaybackRuntime, PlaybackRuntimeCancellation, PlaybackRuntimeConfig,
    PlaybackRuntimeEvent, PlaybackRuntimeEventIter, PlaybackRuntimeEventReceiver,
    PlaybackRuntimeEventRecvError, PlaybackRuntimeEventRecvTimeoutError,
    PlaybackRuntimeGainNormalization, PlaybackRuntimeHandle, PlaybackRuntimeMode,
    PlaybackRuntimeProgress, PlaybackRuntimeReplacePolicy, PlaybackRuntimeRequest,
    PlaybackRuntimeSource, PlaybackRuntimeSpanUpdate, PlaybackRuntimeStarted,
    PlaybackRuntimeStreamPolicy, PlaybackRuntimeSubmitError, PlaybackSeekBehavior,
    PlaybackSourceIdentity, PlaybackSourceKind, PlaybackSpanPlan, PlaybackSpanPlanError,
    PlaybackSpanRequest,
};
pub use recording::{AudioRecorder, InputMonitor, RecordingHealth, RecordingOutcome};
pub use time_stretch::Wsola;

pub(crate) use async_decode::AsyncSource;
#[cfg(test)]
pub(crate) use fade::{EdgeFade, FadeOutHandle, FadeOutOnRequest, fade_duration};
#[cfg(test)]
pub(crate) use routing::normalized_progress;
pub(crate) use source::OutputAdapter;
pub use source::{SamplesBuffer, Source};

// Keep the anti-click envelope short enough to preserve sharp audition
// transients while still smoothing abrupt source starts and stops.
pub(crate) const DEFAULT_ANTI_CLIP_FADE: Duration = Duration::from_millis(3);

#[cfg(test)]
mod tests;
