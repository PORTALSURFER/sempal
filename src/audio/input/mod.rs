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
pub(crate) use stream::{StreamChannelSelection, build_input_stream};

/// Errors that can occur while enumerating or opening audio inputs.
#[derive(Debug, Error)]
pub enum AudioInputError {
    /// No audio input devices are available on the host.
    #[error("No audio input devices found")]
    NoInputDevices,
    /// Failed to enumerate input devices on the host.
    #[error("Could not list input devices: {source}")]
    ListInputDevices {
        /// Underlying cpal error.
        source: cpal::DevicesError,
    },
    /// Failed to query supported input configs for a host.
    #[error("Failed to read supported configs for {host_id}: {source}")]
    SupportedInputConfigs {
        /// Host identifier used for the query.
        host_id: String,
        /// Underlying cpal error.
        source: cpal::SupportedStreamConfigsError,
    },
    /// Failed to create an input stream.
    #[error("Failed to open input stream: {source}")]
    OpenStream {
        /// Underlying cpal error.
        source: cpal::BuildStreamError,
    },
    /// Failed to read the default input config.
    #[error("Failed to read default input config: {source}")]
    DefaultInputConfig {
        /// Underlying cpal error.
        source: cpal::DefaultStreamConfigError,
    },
    /// Failed to start playback on an input stream.
    #[error("Failed to start input stream: {source}")]
    StartStream {
        /// Underlying cpal error.
        source: cpal::PlayStreamError,
    },
    /// Recording failed due to a runtime error.
    #[error("Recording failed: {detail}")]
    RecordingFailed {
        /// Human-readable failure detail.
        detail: String,
    },
}

/// Persisted audio input preferences chosen by the user.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AudioInputConfig {
    /// Preferred host identifier (e.g., "wasapi").
    #[serde(default)]
    pub host: Option<String>,
    /// Preferred device name.
    #[serde(default)]
    pub device: Option<String>,
    /// Preferred sample rate in Hz.
    #[serde(default)]
    pub sample_rate: Option<u32>,
    /// Preferred buffer size in frames.
    #[serde(default)]
    pub buffer_size: Option<u32>,
    /// Preferred channel indices (1-based).
    #[serde(default, deserialize_with = "deserialize_input_channels")]
    pub channels: Vec<u16>,
}

/// Actual input parameters in use after opening a stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedInput {
    /// Host identifier used to open the stream.
    pub host_id: String,
    /// Human-readable device name.
    pub device_name: String,
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Buffer size in frames, if configurable.
    pub buffer_size_frames: Option<u32>,
    /// Total channel count provided by the device.
    pub channel_count: u16,
    /// Selected channel indices (1-based).
    pub selected_channels: Vec<u16>,
    /// Whether a fallback device/config was chosen.
    pub used_fallback: bool,
}

/// Resolved device + stream configuration for input.
pub struct ResolvedInputConfig {
    /// Resolved cpal device.
    pub device: cpal::Device,
    /// Stream configuration to use for capture.
    pub stream_config: cpal::StreamConfig,
    /// Sample format emitted by the stream.
    pub sample_format: cpal::SampleFormat,
    /// Selected channel indices (1-based).
    pub selected_channels: Vec<u16>,
    /// Human-readable resolution summary.
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
        #[serde(deserialize_with = "deserialize_input_channels")]
        channels: Vec<u16>,
    }

    #[test]
    fn deserialize_input_channels_accepts_single_value() {
        let config: ChannelsConfig = serde_json::from_str(r#"{ "channels": 1 }"#).unwrap();
        assert_eq!(config.channels, vec![1]);
    }
}
