use cpal;
use cpal::traits::{DeviceTrait, HostTrait};
use rodio::{OutputStream, OutputStreamBuilder};
use serde::{Deserialize, Serialize};

/// Persisted audio output preferences chosen by the user.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AudioOutputConfig {
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub device: Option<String>,
    #[serde(default)]
    pub sample_rate: Option<u32>,
    #[serde(default)]
    pub buffer_size: Option<u32>,
}

/// Available audio host (backend) presented to the user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioHostSummary {
    pub id: String,
    pub label: String,
    pub is_default: bool,
}

/// Available device on a specific audio host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioDeviceSummary {
    pub host_id: String,
    pub name: String,
    pub is_default: bool,
}

/// Actual output parameters in use after opening an audio stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedOutput {
    pub host_id: String,
    pub device_name: String,
    pub sample_rate: u32,
    pub buffer_size_frames: Option<u32>,
    pub channel_count: u16,
    pub used_fallback: bool,
}

impl Default for ResolvedOutput {
    fn default() -> Self {
        Self {
            host_id: "default".into(),
            device_name: "default".into(),
            sample_rate: 44_100,
            buffer_size_frames: None,
            channel_count: 2,
            used_fallback: false,
        }
    }
}

/// Stream creation result that keeps both the Rodio handle and resolved settings.
pub struct OpenStreamOutcome {
    pub stream: OutputStream,
    pub resolved: ResolvedOutput,
}

/// Enumerate audio hosts available on this platform.
pub fn available_hosts() -> Vec<AudioHostSummary> {
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

/// Enumerate output devices for a specific host.
pub fn available_devices(host_id: &str) -> Result<Vec<AudioDeviceSummary>, String> {
    let (host, id, _) = resolve_host(Some(host_id))?;
    let default_name = host
        .default_output_device()
        .and_then(|device| device_label(&device))
        .unwrap_or_else(|| "System default".to_string());
    let devices = host
        .output_devices()
        .map_err(|err| format!("Could not list output devices: {err}"))?
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
pub fn supported_sample_rates(host_id: &str, device_name: &str) -> Result<Vec<u32>, String> {
    let (host, resolved_host, _) = resolve_host(Some(host_id))?;
    let (device, _, _) = resolve_device(&host, Some(device_name))?;
    let mut supported = Vec::new();
    for range in device
        .supported_output_configs()
        .map_err(|err| format!("Failed to read supported configs for {resolved_host}: {err}"))?
    {
        supported.extend(sample_rates_in_range(
            range.min_sample_rate().0,
            range.max_sample_rate().0,
        ));
    }
    if supported.is_empty() && let Ok(default) = device.default_output_config() {
        supported.push(default.sample_rate().0);
    }
    supported.sort_unstable();
    supported.dedup();
    Ok(supported)
}

/// Open an audio stream honoring user preferences with safe fallbacks.
pub fn open_output_stream(config: &AudioOutputConfig) -> Result<OpenStreamOutcome, String> {
    let (host, host_id, host_fallback) = resolve_host(config.host.as_deref())?;
    let (device, device_name, device_fallback) = resolve_device(&host, config.device.as_deref())?;

    let mut builder =
        OutputStreamBuilder::from_device(device).map_err(map_stream_error("open stream"))?;
    if let Some(rate) = config.sample_rate {
        builder = builder.with_sample_rate(rate);
    }
    if let Some(size) = config.buffer_size.filter(|size| *size > 0) {
        builder = builder.with_buffer_size(cpal::BufferSize::Fixed(size));
    }

    let mut used_fallback = host_fallback || device_fallback;
    let mut resolved_host_id = host_id;
    let mut resolved_device_name = device_name;
    let stream = match builder.open_stream_or_fallback() {
        Ok(stream) => stream,
        Err(err) => {
            used_fallback = true;
            let default_host = cpal::default_host();
            let fallback_device = default_host
                .default_output_device()
                .ok_or_else(|| format!("Audio init failed: {err}"))?;
            resolved_host_id = default_host.id().name().to_string();
            resolved_device_name =
                device_label(&fallback_device).unwrap_or_else(|| "Default device".to_string());
            let fallback_builder = OutputStreamBuilder::from_device(fallback_device)
                .map_err(map_stream_error("open default stream"))?;
            fallback_builder
                .open_stream_or_fallback()
                .map_err(map_stream_error("open default stream"))?
        }
    };

    let resolved_config = *stream.config();
    let applied_buffer = match resolved_config.buffer_size() {
        cpal::BufferSize::Default => None,
        cpal::BufferSize::Fixed(size) => Some(*size),
    };
    if let Some(rate) = config.sample_rate && rate != resolved_config.sample_rate() {
        used_fallback = true;
    }
    if let Some(requested) = config.buffer_size && applied_buffer != Some(requested) {
        used_fallback = true;
    }

    Ok(OpenStreamOutcome {
        stream,
        resolved: ResolvedOutput {
            host_id: resolved_host_id,
            device_name: resolved_device_name,
            sample_rate: resolved_config.sample_rate(),
            buffer_size_frames: applied_buffer,
            channel_count: resolved_config.channel_count() as u16,
            used_fallback,
        },
    })
}

fn resolve_host(id: Option<&str>) -> Result<(cpal::Host, String, bool), String> {
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
) -> Result<(cpal::Device, String, bool), String> {
    let default_device = host
        .default_output_device()
        .ok_or_else(|| "No audio output devices found".to_string())?;
    let default_name = device_label(&default_device).unwrap_or_else(|| "Default device".into());
    let requested_name = name.unwrap_or(&default_name);
    let devices = host
        .output_devices()
        .map_err(|err| format!("Could not list output devices: {err}"))?;
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

fn map_stream_error(action: &'static str) -> impl FnOnce(rodio::StreamError) -> String {
    move |err| format!("Failed to {action}: {err}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_no_preferences() {
        let cfg = AudioOutputConfig::default();
        assert!(cfg.host.is_none());
        assert!(cfg.device.is_none());
        assert!(cfg.sample_rate.is_none());
        assert!(cfg.buffer_size.is_none());
    }

    #[test]
    fn sample_rate_filter_returns_common_values() {
        let rates = sample_rates_in_range(40_000, 50_000);
        assert_eq!(rates, vec![44_100, 48_000]);
    }
}
