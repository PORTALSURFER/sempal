use cpal;
use cpal::traits::{DeviceTrait, HostTrait};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::info;

use super::device::{device_label, host_label};
#[derive(Debug, Error)]
pub enum AudioOutputError {
    #[error("No audio output devices found")]
    NoOutputDevices,
    #[error("Could not list output devices: {source}")]
    ListOutputDevices { source: cpal::DevicesError },
    #[error("Failed to read supported configs for {host_id}: {source}")]
    SupportedOutputConfigs {
        host_id: String,
        source: cpal::SupportedStreamConfigsError,
    },
    #[error("Failed to build stream: {source}")]
    BuildStream { source: cpal::BuildStreamError },
    #[error("Failed to build default stream: {source}")]
    BuildDefaultStream { source: cpal::BuildStreamError },
    #[error("Playback failed to start: {source}")]
    PlayStream { source: cpal::PlayStreamError },
    #[error("Default config error for {host_id}: {source}")]
    DefaultConfig { host_id: String, source: cpal::DefaultStreamConfigError },
}

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

use std::sync::{Arc, Mutex};
use cpal::traits::StreamTrait;

/// Shared state between the player and the audio thread.
pub struct StreamState {
    pub sources: Vec<(Box<dyn rodio::Source<Item = f32> + Send>, f32)>,
    pub volume: f32, // master volume
}

/// Custom container for cpal output stream.
pub struct CpalAudioStream {
    _stream: cpal::Stream,
    pub state: Arc<Mutex<StreamState>>,
}

impl CpalAudioStream {
    pub fn new(stream: cpal::Stream, state: Arc<Mutex<StreamState>>) -> Self {
        Self { _stream: stream, state }
    }
}

/// A bridge for input monitoring that mimics rodio::Sink interface.
pub struct MonitorSink {
    pub state: Arc<Mutex<StreamState>>,
    pub volume: f32,
}

impl MonitorSink {
    pub fn append<S: rodio::Source<Item = f32> + Send + 'static>(&self, source: S) {
        let mut state = self.state.lock().unwrap();
        state.sources.push((Box::new(source), self.volume));
    }

    pub fn play(&self) {}
    pub fn stop(&self) {
        let mut state = self.state.lock().unwrap();
        state.sources.clear(); // Simple implementation: stop all
    }
}

/// Stream creation result that keeps both the stream handle and resolved settings.
pub struct OpenStreamOutcome {
    pub stream: CpalAudioStream,
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
pub fn available_devices(host_id: &str) -> Result<Vec<AudioDeviceSummary>, AudioOutputError> {
    let (host, id, _) = resolve_host(Some(host_id))?;
    let default_name = host
        .default_output_device()
        .and_then(|device| device_label(&device))
        .unwrap_or_else(|| "System default".to_string());
    let devices = host
        .output_devices()
        .map_err(|source| AudioOutputError::ListOutputDevices { source })?
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
pub fn supported_sample_rates(
    host_id: &str,
    device_name: &str,
) -> Result<Vec<u32>, AudioOutputError> {
    let (host, resolved_host, _) = resolve_host(Some(host_id))?;
    let (device, _, _) = resolve_device(&host, Some(device_name))?;
    let mut supported = Vec::new();
    for range in device.supported_output_configs().map_err(|source| {
        AudioOutputError::SupportedOutputConfigs {
            host_id: resolved_host.clone(),
            source,
        }
    })? {
        supported.extend(sample_rates_in_range(
            range.min_sample_rate(),
            range.max_sample_rate(),
        ));
    }
    if supported.is_empty()
        && let Ok(default) = device.default_output_config()
    {
        supported.push(default.sample_rate());
    }
    supported.sort_unstable();
    supported.dedup();
    Ok(supported)
}

/// Open an audio stream honoring user preferences with safe fallbacks.
pub fn open_output_stream(
    config: &AudioOutputConfig,
) -> Result<OpenStreamOutcome, AudioOutputError> {
    let (host, host_id, host_fallback) = resolve_host(config.host.as_deref())?;
    let (device, device_name, device_fallback) = resolve_device(&host, config.device.as_deref())?;

    let stream_config = match device.default_output_config() {
        Ok(c) => c,
        Err(err) => return Err(AudioOutputError::DefaultConfig { host_id, source: err }),
    };

    let mut stream_config: cpal::StreamConfig = stream_config.into();
    if let Some(rate) = config.sample_rate {
        stream_config.sample_rate = rate;
    }
    if let Some(size) = config.buffer_size.filter(|size| *size > 0) {
        stream_config.buffer_size = cpal::BufferSize::Fixed(size);
    }

    let mut used_fallback = host_fallback || device_fallback;
    let mut resolved_host_id = host_id;
    let mut resolved_device_name = device_name;

    let state = Arc::new(Mutex::new(StreamState {
        sources: Vec::new(),
        volume: 1.0,
    }));

    let state_for_callback = state.clone();
    let callback = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
        let mut state = state_for_callback.lock().unwrap();
        let volume = state.volume;
        
        // Fill with silence first
        for sample in data.iter_mut() {
            *sample = 0.0;
        }

        // Mix in all active sources
        state.sources.retain_mut(|(source, source_volume)| {
            let mut finished = false;
            let combined_volume = volume * *source_volume;
            for sample_out in data.iter_mut() {
                if let Some(sample_in) = source.next() {
                    *sample_out += sample_in * combined_volume;
                } else {
                    finished = true;
                    break;
                }
            }
            !finished
        });
    };

    let stream = match device.build_output_stream(
        &stream_config,
        callback,
        |err| tracing::error!("Stream error: {}", err),
        None,
    ) {
        Ok(s) => s,
        Err(err) => {
            used_fallback = true;
            let default_host = cpal::default_host();
            let fallback_device = default_host
                .default_output_device()
                .ok_or_else(|| AudioOutputError::BuildStream { source: err.clone() })?;
            resolved_host_id = default_host.id().name().to_string();
            resolved_device_name =
                device_label(&fallback_device).unwrap_or_else(|| "Default device".to_string());
            
            let fallback_config = fallback_device.default_output_config()
                .map_err(|source| AudioOutputError::DefaultConfig { host_id: resolved_host_id.clone(), source })?;
            
            let fallback_stream_config: cpal::StreamConfig = fallback_config.into();

            let state_for_fallback = state.clone();
            fallback_device.build_output_stream(
                &fallback_stream_config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let mut state = state_for_fallback.lock().unwrap();
                    let volume = state.volume;
                    
                    for sample in data.iter_mut() {
                        *sample = 0.0;
                    }

                    state.sources.retain_mut(|(source, source_volume)| {
                        let mut finished = false;
                        let combined_volume = volume * *source_volume;
                        for sample_out in data.iter_mut() {
                            if let Some(sample_in) = source.next() {
                                *sample_out += sample_in * combined_volume;
                            } else {
                                finished = true;
                                break;
                            }
                        }
                        !finished
                    });
                },
                |err| tracing::error!("Fallback stream error: {}", err),
                None,
            ).map_err(|source| AudioOutputError::BuildDefaultStream { source })?
        }
    };

    stream.play().map_err(|source| AudioOutputError::PlayStream { source })?;

    let resolved_sample_rate = stream_config.sample_rate;
    let applied_buffer = match stream_config.buffer_size {
        cpal::BufferSize::Default => None,
        cpal::BufferSize::Fixed(size) => Some(size),
    };

    let resolved = ResolvedOutput {
        host_id: resolved_host_id,
        device_name: resolved_device_name,
        sample_rate: resolved_sample_rate,
        buffer_size_frames: applied_buffer,
        channel_count: stream_config.channels,
        used_fallback,
    };
    info!(
        "Audio output ready: host={} device=\"{}\" rate={}Hz channels={} buffer={:?} fallback={}",
        resolved.host_id,
        resolved.device_name,
        resolved.sample_rate,
        resolved.channel_count,
        resolved.buffer_size_frames,
        resolved.used_fallback
    );
    Ok(OpenStreamOutcome { stream: CpalAudioStream::new(stream, state), resolved })
}

fn resolve_host(id: Option<&str>) -> Result<(cpal::Host, String, bool), AudioOutputError> {
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
) -> Result<(cpal::Device, String, bool), AudioOutputError> {
    let default_device = host
        .default_output_device()
        .ok_or(AudioOutputError::NoOutputDevices)?;
    let default_name = device_label(&default_device).unwrap_or_else(|| "Default device".into());
    let requested_name = name.unwrap_or(&default_name);
    let devices = host
        .output_devices()
        .map_err(|source| AudioOutputError::ListOutputDevices { source })?;
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
