use std::time::Duration;

mod device;
/// Audio input device enumeration and configuration helpers.
pub mod input;
/// Audio output device enumeration and stream helpers.
pub mod output;
/// Recording and input monitoring helpers.
pub mod recording;

mod fade;
mod loop_diagnostic;
mod mixer;
mod player;
mod source;
mod async_decode;
/// Low-level decoder wrapper for Symphonia.
pub mod decoder;
mod time_stretch;
mod routing;

pub use input::{
    AudioInputConfig, AudioInputError, ResolvedInput, ResolvedInputConfig,
    available_input_channel_count, available_input_devices, available_input_hosts,
    resolve_input_stream_config, supported_input_sample_rates,
};
pub use output::{
    AudioDeviceSummary, AudioHostSummary, AudioOutputConfig, AudioOutputError, ResolvedOutput,
    available_devices, available_hosts, open_output_stream, supported_sample_rates,
};
pub use player::AudioPlayer;
pub(crate) use time_stretch::Wsola;
pub use recording::{AudioRecorder, InputMonitor, RecordingOutcome};

pub(crate) use async_decode::AsyncSource;
pub use source::{Source, SamplesBuffer};
#[cfg(test)]
pub(crate) use fade::{EdgeFade, FadeOutHandle, FadeOutOnRequest, fade_duration};
#[cfg(test)]
pub(crate) use routing::normalized_progress;

pub(crate) const DEFAULT_ANTI_CLIP_FADE: Duration = Duration::from_millis(2);

#[cfg(test)]
mod tests;
