use std::time::Duration;

pub mod output;

mod fade;
mod mixer;
mod player;
mod routing;

pub use output::{
    AudioDeviceSummary, AudioHostSummary, AudioOutputConfig, AudioOutputError, ResolvedOutput,
    available_devices, available_hosts, open_output_stream, supported_sample_rates,
};
pub use player::AudioPlayer;

pub(crate) use fade::{EdgeFade, FadeOutHandle, FadeOutOnRequest, fade_duration, fade_frames_for_duration};
pub(crate) use routing::normalized_progress;

pub(crate) const DEFAULT_ANTI_CLIP_FADE: Duration = Duration::from_millis(2);

#[cfg(test)]
mod tests;
