use super::*;
use crate::egui_app::state::{ActiveAudioOutput, AudioDeviceView, AudioHostView};

impl EguiController {
    /// Refresh available audio hosts/devices and normalize the selected configuration.
    pub(crate) fn refresh_audio_options(&mut self) {
        let mut warning = None;
        let hosts = crate::audio::available_hosts();
        let default_host = hosts
            .iter()
            .find(|host| host.is_default)
            .map(|host| host.id.clone());
        let mut host_id = self
            .settings
            .audio_output
            .host
            .clone()
            .or(default_host.clone());
        if let Some(id) = host_id.as_ref()
            && !hosts.iter().any(|host| &host.id == id)
        {
            warning = Some(format!("Host {id} unavailable; using system default"));
            host_id = default_host;
        }
        self.settings.audio_output.host = host_id.clone();
        self.ui.audio.hosts = hosts
            .iter()
            .map(|host| AudioHostView {
                id: host.id.clone(),
                label: host.label.clone(),
                is_default: host.is_default,
            })
            .collect();

        let devices = if let Some(host) = host_id.as_deref() {
            match crate::audio::available_devices(host) {
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
        let mut device_name = self.settings.audio_output.device.clone();
        if let Some(name) = device_name.as_ref() {
            if !devices.iter().any(|d| &d.name == name) {
                warning.get_or_insert_with(|| {
                    format!(
                        "Device {name} unavailable; using {}",
                        default_device.as_deref().unwrap_or("system default output")
                    )
                });
                device_name = default_device.clone();
            }
        } else {
            device_name = default_device.clone();
        }
        self.settings.audio_output.device = device_name.clone();
        self.ui.audio.devices = devices
            .iter()
            .map(|device| AudioDeviceView {
                host_id: device.host_id.clone(),
                name: device.name.clone(),
                is_default: device.is_default,
            })
            .collect();

        let sample_rates = match (host_id.as_deref(), device_name.as_deref()) {
            (Some(host), Some(device)) => {
                crate::audio::supported_sample_rates(host, device).unwrap_or_else(|_| Vec::new())
            }
            _ => Vec::new(),
        };
        if let Some(rate) = self.settings.audio_output.sample_rate
            && !sample_rates.contains(&rate)
            && !sample_rates.is_empty()
        {
            warning.get_or_insert_with(|| {
                format!("Sample rate {rate} unsupported; using {}", sample_rates[0])
            });
            self.settings.audio_output.sample_rate = Some(sample_rates[0]);
        }
        self.ui.audio.sample_rates = sample_rates;
        self.ui.audio.selected = self.settings.audio_output.clone();
        self.ui.audio.warning = warning;
    }

    /// Update the selected host and rebuild the audio stream.
    pub fn set_audio_host(&mut self, host: Option<String>) {
        if self.settings.audio_output.host == host {
            return;
        }
        self.settings.audio_output.host = host;
        self.refresh_audio_options();
        self.apply_audio_selection();
    }

    /// Update the selected device and rebuild the audio stream.
    pub fn set_audio_device(&mut self, device: Option<String>) {
        if self.settings.audio_output.device == device {
            return;
        }
        self.settings.audio_output.device = device;
        self.refresh_audio_options();
        self.apply_audio_selection();
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
    pub(super) fn apply_audio_selection(&mut self) {
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

    pub(super) fn update_audio_output_status(&mut self) {
        if let Some(player) = self.player.as_ref() {
            let output = player.borrow().output_details().clone();
            self.ui.audio.applied = Some(ActiveAudioOutput::from(&output));
            self.ui.audio.warning = self.audio_fallback_message(&output);
        }
    }

    fn rebuild_audio_player(&mut self) -> Result<(), String> {
        let loaded_audio = self.wav_selection.loaded_audio.clone();
        self.player = None;
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
}
