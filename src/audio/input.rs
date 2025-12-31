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
}

/// Actual input parameters in use after opening a stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedInput {
    pub host_id: String,
    pub device_name: String,
    pub sample_rate: u32,
    pub buffer_size_frames: Option<u32>,
    pub channel_count: u16,
    pub used_fallback: bool,
}

/// Resolved device + stream configuration for input.
pub struct ResolvedInputConfig {
    pub device: cpal::Device,
    pub stream_config: cpal::StreamConfig,
    pub sample_format: cpal::SampleFormat,
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

pub fn resolve_input_stream_config(
    config: &AudioInputConfig,
    desired_channels: u16,
) -> Result<ResolvedInputConfig, AudioInputError> {
    let (host, host_id, host_fallback) = resolve_host(config.host.as_deref())?;
    let (device, device_name, device_fallback) = resolve_device(&host, config.device.as_deref())?;
    let default_config = device
        .default_input_config()
        .map_err(|source| AudioInputError::DefaultInputConfig { source })?;
    let supported = device.supported_input_configs().map_err(|source| {
        AudioInputError::SupportedInputConfigs {
            host_id: host_id.clone(),
            source,
        }
    })?;
    let supported: Vec<_> = supported.collect();
    if supported.is_empty() {
        return Err(AudioInputError::NoInputDevices);
    }
    let default_rate = default_config.sample_rate().0;
    let requested_rate = config.sample_rate;
    let mut used_fallback = host_fallback || device_fallback;
    let (range, rate, channel_count) = pick_stream_config(
        &supported,
        default_rate,
        requested_rate,
        desired_channels,
        &mut used_fallback,
    );
    let mut stream_config = range
        .with_sample_rate(cpal::SampleRate(rate))
        .config();
    if let Some(size) = config.buffer_size.filter(|size| *size > 0) {
        stream_config.buffer_size = cpal::BufferSize::Fixed(size);
    }
    if requested_rate.is_some_and(|rate| rate != stream_config.sample_rate.0) {
        used_fallback = true;
    }
    if channel_count != desired_channels {
        used_fallback = true;
    }
    let applied_buffer = match stream_config.buffer_size {
        cpal::BufferSize::Default => None,
        cpal::BufferSize::Fixed(size) => Some(size),
    };
    let sample_rate = stream_config.sample_rate.0;
    Ok(ResolvedInputConfig {
        device,
        stream_config,
        sample_format: range.sample_format(),
        resolved: ResolvedInput {
            host_id,
            device_name,
            sample_rate,
            buffer_size_frames: applied_buffer,
            channel_count,
            used_fallback,
        },
    })
}

fn pick_stream_config<'a>(
    supported: &'a [cpal::SupportedStreamConfigRange],
    default_rate: u32,
    requested_rate: Option<u32>,
    desired_channels: u16,
    used_fallback: &mut bool,
) -> (&'a cpal::SupportedStreamConfigRange, u32, u16) {
    let desired: Vec<&cpal::SupportedStreamConfigRange> = supported
        .iter()
        .filter(|range| range.channels() == desired_channels)
        .collect();
    let using_desired = !desired.is_empty();
    let ranges: Vec<&cpal::SupportedStreamConfigRange> = if !using_desired {
        *used_fallback = true;
        supported.iter().collect()
    } else {
        desired
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
