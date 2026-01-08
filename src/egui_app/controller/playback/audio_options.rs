use super::*;
use crate::egui_app::state::{ActiveAudioInput, ActiveAudioOutput, AudioDeviceView, AudioHostView};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NormalizedAudioOptions {
    pub(crate) host_id: Option<String>,
    pub(crate) device_name: Option<String>,
    pub(crate) sample_rate: Option<u32>,
    pub(crate) devices: Vec<crate::audio::AudioDeviceSummary>,
    pub(crate) sample_rates: Vec<u32>,
    pub(crate) warning: Option<String>,
}

pub(crate) fn normalize_audio_options(
    current_host: Option<String>,
    current_device: Option<String>,
    current_sample_rate: Option<u32>,
    hosts: &[crate::audio::AudioHostSummary],
    devices_for_host: impl FnOnce(&str) -> Result<Vec<crate::audio::AudioDeviceSummary>, String>,
    sample_rates_for: impl FnOnce(&str, &str) -> Vec<u32>,
    default_device_label: &str,
) -> NormalizedAudioOptions {
    let mut warning = None;
    let default_host = hosts
        .iter()
        .find(|host| host.is_default)
        .map(|host| host.id.clone());
    let mut host_id = current_host.or(default_host.clone());
    if let Some(id) = host_id.as_ref()
        && !hosts.iter().any(|host| &host.id == id)
    {
        warning = Some(format!("Host {id} unavailable; using system default"));
        host_id = default_host;
    }

    let devices = if let Some(host) = host_id.as_deref() {
        match devices_for_host(host) {
            Ok(list) => list,
            Err(err) => {
                warning = Some(err);
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };
    let default_device = devices
        .iter()
        .find(|d| d.is_default)
        .map(|d| d.name.clone())
        .or_else(|| devices.first().map(|d| d.name.clone()));
    let mut device_name = current_device;
    if let Some(name) = device_name.as_ref() {
        if !devices.iter().any(|d| &d.name == name) {
            warning.get_or_insert_with(|| {
                format!(
                    "Device {name} unavailable; using {}",
                    default_device.as_deref().unwrap_or(default_device_label)
                )
            });
            device_name = default_device.clone();
        }
    } else {
        device_name = default_device.clone();
    }

    let sample_rates = match (host_id.as_deref(), device_name.as_deref()) {
        (Some(host), Some(device)) => sample_rates_for(host, device),
        _ => Vec::new(),
    };
    let mut sample_rate = current_sample_rate;
    if let Some(rate) = sample_rate
        && !sample_rates.contains(&rate)
        && !sample_rates.is_empty()
    {
        warning.get_or_insert_with(|| {
            format!("Sample rate {rate} unsupported; using {}", sample_rates[0])
        });
        sample_rate = Some(sample_rates[0]);
    }

    NormalizedAudioOptions {
        host_id,
        device_name,
        sample_rate,
        devices,
        sample_rates,
        warning,
    }
}

impl EguiController {
    /// Refresh available audio hosts/devices and normalize the selected configuration.
    pub(crate) fn refresh_audio_options(&mut self, probe_rates: bool) {
        let previous = self.ui.audio.selected.clone();
        let hosts = crate::audio::available_hosts();
        let normalized = normalize_audio_options(
            self.settings.audio_output.host.clone(),
            self.settings.audio_output.device.clone(),
            self.settings.audio_output.sample_rate,
            &hosts,
            |host| crate::audio::available_devices(host).map_err(|err| err.to_string()),
            |host, device| {
                if probe_rates {
                    crate::audio::supported_sample_rates(host, device)
                        .unwrap_or_else(|_| Vec::new())
                } else {
                    Vec::new()
                }
            },
            "system default output",
        );
        self.settings.audio_output.host = normalized.host_id.clone();
        self.settings.audio_output.device = normalized.device_name.clone();
        self.settings.audio_output.sample_rate = normalized.sample_rate;
        let selection_changed =
            normalized.host_id != previous.host || normalized.device_name != previous.device;
        self.ui.audio.hosts = hosts
            .iter()
            .map(|host| AudioHostView {
                id: host.id.clone(),
                label: host.label.clone(),
                is_default: host.is_default,
            })
            .collect();

        self.ui.audio.devices = normalized
            .devices
            .iter()
            .map(|device| AudioDeviceView {
                host_id: device.host_id.clone(),
                name: device.name.clone(),
                is_default: device.is_default,
            })
            .collect();

        self.ui.audio.sample_rates = if probe_rates {
            normalized.sample_rates
        } else if selection_changed {
            Vec::new()
        } else {
            self.ui.audio.sample_rates.clone()
        };
        self.ui.audio.selected = self.settings.audio_output.clone();
        self.ui.audio.warning = normalized.warning;
    }

    pub(crate) fn refresh_audio_input_options(&mut self, probe_details: bool) {
        let previous = self.ui.audio.input_selected.clone();
        let hosts = crate::audio::available_input_hosts();
        let normalized = normalize_audio_options(
            self.settings.audio_input.host.clone(),
            self.settings.audio_input.device.clone(),
            self.settings.audio_input.sample_rate,
            &hosts,
            |host| crate::audio::available_input_devices(host).map_err(|err| err.to_string()),
            |host, device| {
                if probe_details {
                    crate::audio::supported_input_sample_rates(host, device)
                        .unwrap_or_else(|_| Vec::new())
                } else {
                    Vec::new()
                }
            },
            "system default input",
        );
        self.settings.audio_input.host = normalized.host_id.clone();
        self.settings.audio_input.device = normalized.device_name.clone();
        self.settings.audio_input.sample_rate = normalized.sample_rate;
        let selection_changed =
            normalized.host_id != previous.host || normalized.device_name != previous.device;
        self.ui.audio.input_hosts = hosts
            .iter()
            .map(|host| AudioHostView {
                id: host.id.clone(),
                label: host.label.clone(),
                is_default: host.is_default,
            })
            .collect();

        self.ui.audio.input_devices = normalized
            .devices
            .iter()
            .map(|device| AudioDeviceView {
                host_id: device.host_id.clone(),
                name: device.name.clone(),
                is_default: device.is_default,
            })
            .collect();

        self.ui.audio.input_sample_rates = if probe_details {
            normalized.sample_rates.clone()
        } else if selection_changed {
            Vec::new()
        } else {
            self.ui.audio.input_sample_rates.clone()
        };

        let mut warning = normalized.warning;
        let channel_count = if probe_details {
            match (
                self.settings.audio_input.host.as_deref(),
                self.settings.audio_input.device.as_deref(),
            ) {
                (Some(host), Some(device)) => {
                    match crate::audio::available_input_channel_count(host, device) {
                        Ok(count) => count,
                        Err(err) => {
                            warning.get_or_insert_with(|| err.to_string());
                            0
                        }
                    }
                }
                _ => 0,
            }
        } else if selection_changed {
            0
        } else {
            self.ui.audio.input_channel_count
        };
        let normalized = Self::normalize_input_channel_selection(
            &self.settings.audio_input.channels,
            channel_count,
        );
        if !self.settings.audio_input.channels.is_empty()
            && normalized != self.settings.audio_input.channels
        {
            warning.get_or_insert_with(|| {
                format!(
                    "Input channels {} unavailable; using {}",
                    format_channel_list(&self.settings.audio_input.channels),
                    format_channel_list(&normalized)
                )
            });
        }
        self.settings.audio_input.channels = normalized;
        self.ui.audio.input_channel_count = channel_count;
        self.ui.audio.input_selected = self.settings.audio_input.clone();
        self.ui.audio.input_warning = warning;
    }

    /// Update the selected host and rebuild the audio stream.
    pub fn set_audio_host(&mut self, host: Option<String>) {
        if self.settings.audio_output.host == host {
            return;
        }
        self.settings.audio_output.host = host;
        self.refresh_audio_options(true);
        self.apply_audio_selection();
    }

    /// Update the selected input host and persist input settings.
    pub fn set_audio_input_host(&mut self, host: Option<String>) {
        if self.settings.audio_input.host == host {
            return;
        }
        self.settings.audio_input.host = host;
        self.refresh_audio_input_options(true);
        let _ = self.persist_config("Failed to save audio input settings");
    }

    /// Update the selected device and rebuild the audio stream.
    pub fn set_audio_device(&mut self, device: Option<String>) {
        if self.settings.audio_output.device == device {
            return;
        }
        self.settings.audio_output.device = device;
        self.refresh_audio_options(true);
        self.apply_audio_selection();
    }

    /// Update the selected input device and persist input settings.
    pub fn set_audio_input_device(&mut self, device: Option<String>) {
        if self.settings.audio_input.device == device {
            return;
        }
        self.settings.audio_input.device = device;
        self.refresh_audio_input_options(true);
        let _ = self.persist_config("Failed to save audio input settings");
    }

    /// Update the selected sample rate and rebuild the audio stream.
    pub fn set_audio_sample_rate(&mut self, sample_rate: Option<u32>) {
        if self.settings.audio_output.sample_rate == sample_rate {
            return;
        }
        self.settings.audio_output.sample_rate = sample_rate;
        self.ui.audio.selected.sample_rate = sample_rate;
        self.apply_audio_selection();
    }

    /// Update the selected input sample rate and persist input settings.
    pub fn set_audio_input_sample_rate(&mut self, sample_rate: Option<u32>) {
        if self.settings.audio_input.sample_rate == sample_rate {
            return;
        }
        self.settings.audio_input.sample_rate = sample_rate;
        self.ui.audio.input_selected.sample_rate = sample_rate;
        let _ = self.persist_config("Failed to save audio input settings");
    }

    /// Update the selected input channels and persist input settings.
    pub fn set_audio_input_channels(&mut self, channels: Vec<u16>) {
        let normalized =
            Self::normalize_input_channel_selection(&channels, self.ui.audio.input_channel_count);
        if self.settings.audio_input.channels == normalized {
            return;
        }
        self.settings.audio_input.channels = normalized.clone();
        self.ui.audio.input_selected.channels = normalized;
        let _ = self.persist_config("Failed to save audio input settings");
    }

    /// Update the selected buffer size (frames) and rebuild the audio stream.
    pub fn set_audio_buffer_size(&mut self, buffer_size: Option<u32>) {
        if self.settings.audio_output.buffer_size == buffer_size {
            return;
        }
        self.settings.audio_output.buffer_size = buffer_size;
        self.ui.audio.selected.buffer_size = buffer_size;
        self.apply_audio_selection();
    }

    /// Apply current audio config to the player and persist config.
    pub(crate) fn apply_audio_selection(&mut self) {
        self.ui.audio.selected = self.settings.audio_output.clone();
        match self.rebuild_audio_player() {
            Ok(_) => {
                let _ = self.persist_config("Failed to save audio settings");
            }
            Err(err) => {
                self.set_status(err, StatusTone::Error);
            }
        }
    }

    pub(crate) fn update_audio_output_status(&mut self) {
        if let Some(player) = self.audio.player.as_ref() {
            let output = player.borrow().output_details().clone();
            self.ui.audio.applied = Some(ActiveAudioOutput::from(&output));
            self.ui.audio.warning = self.audio_fallback_message(&output);
        }
    }

    pub(crate) fn update_audio_input_status(&mut self, input: &crate::audio::ResolvedInput) {
        self.ui.audio.input_applied = Some(ActiveAudioInput::from(input));
        self.ui.audio.input_warning = self.audio_input_fallback_message(input);
    }

    pub(crate) fn rebuild_audio_player(&mut self) -> Result<(), String> {
        let loaded_audio = self.sample_view.wav.loaded_audio.clone();
        self.audio.player = None;
        let Some(player_rc) = self.ensure_player()? else {
            self.ui.audio.applied = None;
            return Err("Audio unavailable".into());
        };
        if let Some(audio) = loaded_audio {
            let mut player = player_rc.borrow_mut();
            player.stop();
            player.set_audio(audio.bytes.clone(), audio.duration_seconds);
        }
        self.update_audio_output_status();
        Ok(())
    }

    fn audio_fallback_message(&self, output: &crate::audio::ResolvedOutput) -> Option<String> {
        if !output.used_fallback {
            return None;
        }
        let mut reasons = Vec::new();
        if let Some(host) = self.settings.audio_output.host.as_deref()
            && host != output.host_id
        {
            reasons.push(format!("host {host}"));
        }
        if let Some(device) = self.settings.audio_output.device.as_deref()
            && device != output.device_name
        {
            reasons.push(format!("device {device}"));
        }
        if let Some(rate) = self.settings.audio_output.sample_rate
            && rate != output.sample_rate
        {
            reasons.push(format!("sample rate {rate}"));
        }
        if let Some(size) = self.settings.audio_output.buffer_size
            && output.buffer_size_frames != Some(size)
        {
            reasons.push(format!("buffer {size}"));
        }
        let details = if reasons.is_empty() {
            "requested settings".to_string()
        } else {
            reasons.join(", ")
        };
        Some(format!(
            "Using {} via {} ({details} unavailable)",
            output.device_name, output.host_id
        ))
    }

    fn audio_input_fallback_message(&self, input: &crate::audio::ResolvedInput) -> Option<String> {
        if !input.used_fallback {
            return None;
        }
        let mut reasons = Vec::new();
        if let Some(host) = self.settings.audio_input.host.as_deref()
            && host != input.host_id
        {
            reasons.push(format!("host {host}"));
        }
        if let Some(device) = self.settings.audio_input.device.as_deref()
            && device != input.device_name
        {
            reasons.push(format!("device {device}"));
        }
        if let Some(rate) = self.settings.audio_input.sample_rate
            && rate != input.sample_rate
        {
            reasons.push(format!("sample rate {rate}"));
        }
        if self.settings.audio_input.channels != input.selected_channels {
            reasons.push(format!(
                "inputs {}",
                format_channel_list(&self.settings.audio_input.channels)
            ));
        }
        let details = if reasons.is_empty() {
            "requested settings".to_string()
        } else {
            reasons.join(", ")
        };
        Some(format!(
            "Using {} via {} ({details} unavailable)",
            input.device_name, input.host_id
        ))
    }

    fn normalize_input_channel_selection(requested: &[u16], max_channels: u16) -> Vec<u16> {
        if max_channels == 0 {
            return Vec::new();
        }
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
        if selected.is_empty() {
            if max_channels >= 2 {
                selected = vec![1, 2];
            } else {
                selected = vec![1];
            }
        }
        selected
    }
}

fn format_channel_list(channels: &[u16]) -> String {
    if channels.is_empty() {
        return "none".to_string();
    }
    channels
        .iter()
        .map(|channel| channel.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}
