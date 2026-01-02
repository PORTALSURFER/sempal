use cpal;
use cpal::traits::{DeviceTrait, HostTrait};

use super::{AudioDeviceSummary, AudioHostSummary, AudioInputError};
use crate::audio::device::{device_label, host_label};
use super::resolve::{resolve_device, resolve_host};

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
