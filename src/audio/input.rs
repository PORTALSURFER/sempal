use cpal;
use cpal::traits::{DeviceTrait, HostTrait};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::output::{AudioDeviceSummary, AudioHostSummary};

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

/// Enumerate audio hosts available on this platform.
pub fn available_input_hosts() -> Vec<AudioHostSummary> {
    let default_host = cpal::default_host();
    let default_id = default_host.id().name().to_string();
    cpal::available_hosts()
        .into_iter()
        .filter_map(|id| cpal::host_from_id(id).ok())
        .map(|host| {
            let id = host.id().name().to_string();
            AudioHostSummary {
                label: host_label(&id),
                is_default: id == default_id,
                id,
            }
        })
        .collect()
}

/// Enumerate input devices for a specific host.
pub fn available_input_devices(host_id: &str) -> Result<Vec<AudioDeviceSummary>, AudioInputError> {
    let (host, id, _) = resolve_host(Some(host_id))?;
    let default_name = host
        .default_input_device()
        .and_then(|device| device_label(&device))
        .unwrap_or_else(|| "System default".to_string());
    let devices = host
        .input_devices()
        .map_err(|source| AudioInputError::ListInputDevices { source })?
        .filter_map(|device| {
            let name = device_label(&device)?;
            Some(AudioDeviceSummary {
                host_id: id.clone(),
                is_default: name == default_name,
                name,
            })
        })
        .collect();
    Ok(devices)
}

/// Sample rates supported by the given host/device pair.
pub fn supported_input_sample_rates(
    host_id: &str,
    device_name: &str,
) -> Result<Vec<u32>, AudioInputError> {
    let (host, resolved_host, _) = resolve_host(Some(host_id))?;
    let (device, _, _) = resolve_device(&host, Some(device_name))?;
    let mut supported = Vec::new();
    for range in device.supported_input_configs().map_err(|source| {
        AudioInputError::SupportedInputConfigs {
            host_id: resolved_host.clone(),
            source,
        }
    })? {
        supported.extend(sample_rates_in_range(
            range.min_sample_rate().0,
            range.max_sample_rate().0,
        ));
    }
    if supported.is_empty()
        && let Ok(default) = device.default_input_config()
    {
        supported.push(default.sample_rate().0);
    }
    supported.sort_unstable();
    supported.dedup();
    Ok(supported)
}

/// Maximum number of input channels available on the device.
pub fn available_input_channel_count(
    host_id: &str,
    device_name: &str,
) -> Result<u16, AudioInputError> {
    let (host, resolved_host, _) = resolve_host(Some(host_id))?;
    let (device, _, _) = resolve_device(&host, Some(device_name))?;
    let mut max_channels = None;
    for range in device.supported_input_configs().map_err(|source| {
        AudioInputError::SupportedInputConfigs {
            host_id: resolved_host.clone(),
            source,
        }
    })? {
        let channels = range.channels();
        max_channels = Some(max_channels.map_or(channels, |max: u16| max.max(channels)));
    }
    if let Some(channels) = max_channels {
        return Ok(channels);
    }
    let default = device
        .default_input_config()
        .map_err(|source| AudioInputError::DefaultInputConfig { source })?;
    Ok(default.channels())
}

pub fn resolve_input_stream_config(
    config: &AudioInputConfig,
) -> Result<ResolvedInputConfig, AudioInputError> {
    let (host, host_id, host_fallback) = resolve_host(config.host.as_deref())?;
    let (device, device_name, device_fallback) = resolve_device(&host, config.device.as_deref())?;
    let (default_config, supported) = load_input_configs(&device, &host_id)?;
    let max_channels = max_supported_channels(&supported, default_config.channels());
    let selection = resolve_selected_input_channels(&config.channels, max_channels);
    let default_rate = default_config.sample_rate().0;
    let requested_rate = config.sample_rate;
    let mut used_fallback = host_fallback || device_fallback || selection.used_fallback;
    let preferred_stream_channels = selection.output_channels.max(selection.min_stream_channels);
    let (range, rate, _channel_count) = pick_stream_config(
        &supported,
        default_rate,
        requested_rate,
        preferred_stream_channels,
        selection.min_stream_channels,
        &mut used_fallback,
    );
    let (stream_config, applied_buffer) =
        build_input_stream_config(range, rate, config.buffer_size);
    if requested_rate.is_some_and(|rate| rate != stream_config.sample_rate.0) {
        used_fallback = true;
    }
    let sample_rate = stream_config.sample_rate.0;
    Ok(ResolvedInputConfig {
        device,
        stream_config,
        sample_format: range.sample_format(),
        selected_channels: selection.selected_channels.clone(),
        resolved: ResolvedInput {
            host_id,
            device_name,
            sample_rate,
            buffer_size_frames: applied_buffer,
            channel_count: selection.output_channels,
            selected_channels: selection.selected_channels,
            used_fallback,
        },
    })
}

fn load_input_configs(
    device: &cpal::Device,
    host_id: &str,
) -> Result<(cpal::SupportedStreamConfig, Vec<cpal::SupportedStreamConfigRange>), AudioInputError> {
    let default_config = device
        .default_input_config()
        .map_err(|source| AudioInputError::DefaultInputConfig { source })?;
    let supported = device.supported_input_configs().map_err(|source| {
        AudioInputError::SupportedInputConfigs {
            host_id: host_id.to_string(),
            source,
        }
    })?;
    let supported: Vec<_> = supported.collect();
    if supported.is_empty() {
        return Err(AudioInputError::NoInputDevices);
    }
    Ok((default_config, supported))
}

fn build_input_stream_config(
    range: &cpal::SupportedStreamConfigRange,
    rate: u32,
    buffer_size: Option<u32>,
) -> (cpal::StreamConfig, Option<u32>) {
    let mut stream_config = range
        .with_sample_rate(cpal::SampleRate(rate))
        .config();
    if let Some(size) = buffer_size.filter(|size| *size > 0) {
        stream_config.buffer_size = cpal::BufferSize::Fixed(size);
    }
    let applied_buffer = match stream_config.buffer_size {
        cpal::BufferSize::Default => None,
        cpal::BufferSize::Fixed(size) => Some(size),
    };
    (stream_config, applied_buffer)
}

fn pick_stream_config<'a>(
    supported: &'a [cpal::SupportedStreamConfigRange],
    default_rate: u32,
    requested_rate: Option<u32>,
    preferred_channels: u16,
    min_channels: u16,
    used_fallback: &mut bool,
) -> (&'a cpal::SupportedStreamConfigRange, u32, u16) {
    let desired: Vec<&cpal::SupportedStreamConfigRange> = supported
        .iter()
        .filter(|range| range.channels() >= min_channels)
        .collect();
    let using_desired = !desired.is_empty();
    let ranges: Vec<&cpal::SupportedStreamConfigRange> = if !using_desired {
        *used_fallback = true;
        supported.iter().collect()
    } else {
        desired
    };
    let ranges: Vec<&cpal::SupportedStreamConfigRange> = {
        let exact: Vec<_> = ranges
            .iter()
            .copied()
            .filter(|range| range.channels() == preferred_channels)
            .collect();
        if exact.is_empty() {
            if using_desired {
                *used_fallback = true;
            }
            ranges
        } else {
            exact
        }
    };
    let mut picked = None;
    let mut rate = default_rate;
    if let Some(requested) = requested_rate {
        if let Some(range) = ranges
            .iter()
            .find(|range| rate_in_range(requested, *range))
        {
            picked = Some(*range);
            rate = requested;
        } else if using_desired {
            *used_fallback = true;
            if let Some(range) = supported
                .iter()
                .find(|range| rate_in_range(requested, *range))
            {
                picked = Some(range);
                rate = requested;
            }
        }
        if picked.is_none() {
            *used_fallback = true;
        }
    }
    if picked.is_none() {
        if let Some(range) = ranges
            .iter()
            .find(|range| rate_in_range(default_rate, *range))
        {
            picked = Some(*range);
            rate = default_rate;
        } else {
            let range = ranges[0];
            picked = Some(range);
            rate = range.max_sample_rate().0;
            *used_fallback = true;
        }
    }
    let range = picked.expect("stream config should be chosen");
    (range, rate, range.channels())
}

fn rate_in_range(rate: u32, range: &cpal::SupportedStreamConfigRange) -> bool {
    let min = range.min_sample_rate().0;
    let max = range.max_sample_rate().0;
    rate >= min && rate <= max
}

fn max_supported_channels(
    supported: &[cpal::SupportedStreamConfigRange],
    default_channels: u16,
) -> u16 {
    supported
        .iter()
        .map(|range| range.channels())
        .max()
        .unwrap_or(default_channels)
}

struct InputChannelSelection {
    selected_channels: Vec<u16>,
    output_channels: u16,
    min_stream_channels: u16,
    used_fallback: bool,
}

fn resolve_selected_input_channels(
    requested: &[u16],
    max_channels: u16,
) -> InputChannelSelection {
    let mut used_fallback = false;
    let mut selected = normalize_selected_channels(requested, max_channels);
    if !requested.is_empty() && selected.len() != requested.len() {
        used_fallback = true;
    }
    if selected.is_empty() {
        selected = default_input_channels(max_channels);
    }
    if !requested.is_empty() && selected.is_empty() {
        used_fallback = true;
    }
    let output_channels = selected.len().clamp(1, 2) as u16;
    let min_stream_channels = selected
        .iter()
        .copied()
        .max()
        .unwrap_or(output_channels);
    InputChannelSelection {
        selected_channels: selected,
        output_channels,
        min_stream_channels,
        used_fallback,
    }
}

fn normalize_selected_channels(requested: &[u16], max_channels: u16) -> Vec<u16> {
    let mut selected: Vec<u16> = requested
        .iter()
        .copied()
        .filter(|channel| *channel >= 1 && *channel <= max_channels)
        .collect();
    selected.sort_unstable();
    selected.dedup();
    if selected.len() > 2 {
        selected.truncate(2);
    }
    selected
}

fn default_input_channels(max_channels: u16) -> Vec<u16> {
    if max_channels >= 2 {
        vec![1, 2]
    } else if max_channels == 1 {
        vec![1]
    } else {
        Vec::new()
    }
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

fn resolve_host(id: Option<&str>) -> Result<(cpal::Host, String, bool), AudioInputError> {
    let default_host = cpal::default_host();
    let default_id = default_host.id().name().to_string();
    let Some(requested) = id else {
        return Ok((default_host, default_id, false));
    };

    let host = cpal::available_hosts()
        .into_iter()
        .find(|candidate| candidate.name() == requested)
        .and_then(|id| cpal::host_from_id(id).ok())
        .unwrap_or(default_host);
    let resolved_id = host.id().name().to_string();
    let used_fallback = resolved_id != requested;
    Ok((host, resolved_id, used_fallback))
}

fn resolve_device(
    host: &cpal::Host,
    name: Option<&str>,
) -> Result<(cpal::Device, String, bool), AudioInputError> {
    let default_device = host
        .default_input_device()
        .ok_or(AudioInputError::NoInputDevices)?;
    let default_name = device_label(&default_device).unwrap_or_else(|| "Default device".into());
    let requested_name = name.unwrap_or(&default_name);
    let devices = host
        .input_devices()
        .map_err(|source| AudioInputError::ListInputDevices { source })?;
    let mut chosen = None;
    for device in devices {
        if device_label(&device)
            .as_ref()
            .is_some_and(|name| name == requested_name)
        {
            chosen = Some(device);
            break;
        }
    }
    let resolved = chosen.unwrap_or(default_device);
    let resolved_name = device_label(&resolved).unwrap_or_else(|| default_name.clone());
    let used_fallback = resolved_name != requested_name;
    Ok((resolved, resolved_name, used_fallback))
}

fn device_label(device: &cpal::Device) -> Option<String> {
    device.name().ok()
}

fn host_label(id: &str) -> String {
    match id.to_ascii_lowercase().as_str() {
        "asio" => "ASIO".into(),
        "wasapi" => "WASAPI".into(),
        "coreaudio" => "Core Audio".into(),
        "alsa" => "ALSA".into(),
        "jack" => "JACK".into(),
        _ => id.to_uppercase(),
    }
}

const COMMON_SAMPLE_RATES: &[u32] = &[32_000, 44_100, 48_000, 88_200, 96_000, 176_400, 192_000];

fn sample_rates_in_range(min: u32, max: u32) -> Vec<u32> {
    COMMON_SAMPLE_RATES
        .iter()
        .copied()
        .filter(|rate| *rate >= min && *rate <= max)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_rate_filter_returns_common_values() {
        let rates = sample_rates_in_range(40_000, 50_000);
        assert_eq!(rates, vec![44_100, 48_000]);
    }
}
