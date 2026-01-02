use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::output::{AudioDeviceSummary, AudioHostSummary};

mod enumerate;
mod resolve;
pub(crate) mod stream;

pub use enumerate::{
    available_input_channel_count, available_input_devices, available_input_hosts,
    supported_input_sample_rates,
};
pub use resolve::resolve_input_stream_config;
pub(crate) use stream::{build_input_stream, StreamChannelSelection};

#[derive(Debug, Error)]
pub enum AudioInputError {
    #[error("No audio input devices found")]
    NoInputDevices,
    #[error("Could not list input devices: {source}")]
    ListInputDevices { source: cpal::DevicesError },
    #[error("Failed to read supported configs for {host_id}: {source}")]
    SupportedInputConfigs {
        host_id: String,
        source: cpal::SupportedStreamConfigsError,
    },
    #[error("Failed to open input stream: {source}")]
    OpenStream { source: cpal::BuildStreamError },
    #[error("Failed to read default input config: {source}")]
    DefaultInputConfig { source: cpal::DefaultStreamConfigError },
    #[error("Failed to start input stream: {source}")]
    StartStream { source: cpal::PlayStreamError },
    #[error("Recording failed: {detail}")]
    RecordingFailed { detail: String },
}

/// Persisted audio input preferences chosen by the user.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AudioInputConfig {
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub device: Option<String>,
    #[serde(default)]
    pub sample_rate: Option<u32>,
    #[serde(default)]
    pub buffer_size: Option<u32>,
    #[serde(default, deserialize_with = "deserialize_input_channels")]
    pub channels: Vec<u16>,
}

/// Actual input parameters in use after opening a stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedInput {
    pub host_id: String,
    pub device_name: String,
    pub sample_rate: u32,
    pub buffer_size_frames: Option<u32>,
    pub channel_count: u16,
    pub selected_channels: Vec<u16>,
    pub used_fallback: bool,
}

/// Resolved device + stream configuration for input.
pub struct ResolvedInputConfig {
    pub device: cpal::Device,
    pub stream_config: cpal::StreamConfig,
    pub sample_format: cpal::SampleFormat,
    pub selected_channels: Vec<u16>,
    pub resolved: ResolvedInput,
}

fn deserialize_input_channels<'de, D>(deserializer: D) -> Result<Vec<u16>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum InputChannels {
        Single(u16),
        Multiple(Vec<u16>),
    }

    match InputChannels::deserialize(deserializer)? {
        InputChannels::Single(channel) => Ok(vec![channel]),
        InputChannels::Multiple(channels) => Ok(channels),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct ChannelsConfig {
        channels: Vec<u16>,
    }

    #[test]
    fn deserialize_input_channels_accepts_single_value() {
        let config: ChannelsConfig = serde_json::from_str(r#"{ "channels": 1 }"#).unwrap();
        assert_eq!(config.channels, vec![1]);
    }
}
