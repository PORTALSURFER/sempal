use eframe::egui;

use crate::egui_app::ui::EguiApp;

impl EguiApp {
    pub(super) fn render_audio_host_combo(&mut self, ui: &mut egui::Ui) {
        let selected_host = self.controller.ui.audio.selected.host.clone();
        let hosts = self.controller.ui.audio.hosts.clone();
        let current = selected_host
            .clone()
            .unwrap_or_else(|| "System default".to_string());
        egui::ComboBox::from_id_salt("audio_host_combo")
            .width(220.0)
            .selected_text(current)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(selected_host.is_none(), "System default")
                    .clicked()
                {
                    self.controller.set_audio_host(None);
                }
                for host in &hosts {
                    let selected = selected_host.as_deref() == Some(host.id.as_str());
                    if ui.selectable_label(selected, &host.label).clicked() {
                        self.controller.set_audio_host(Some(host.id.clone()));
                    }
                }
            });
    }

    pub(super) fn render_audio_input_host_combo(&mut self, ui: &mut egui::Ui) {
        let selected_host = self.controller.ui.audio.input_selected.host.clone();
        let hosts = self.controller.ui.audio.input_hosts.clone();
        let current = selected_host
            .clone()
            .unwrap_or_else(|| "System default".to_string());
        egui::ComboBox::from_id_salt("audio_input_host_combo")
            .width(220.0)
            .selected_text(current)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(selected_host.is_none(), "System default")
                    .clicked()
                {
                    self.controller.set_audio_input_host(None);
                }
                for host in &hosts {
                    let selected = selected_host.as_deref() == Some(host.id.as_str());
                    if ui.selectable_label(selected, &host.label).clicked() {
                        self.controller.set_audio_input_host(Some(host.id.clone()));
                    }
                }
            });
    }

    pub(super) fn render_audio_device_combo(&mut self, ui: &mut egui::Ui) {
        let selected_device = self.controller.ui.audio.selected.device.clone();
        let devices = self.controller.ui.audio.devices.clone();
        let current = selected_device
            .clone()
            .unwrap_or_else(|| "System default".to_string());
        egui::ComboBox::from_id_salt("audio_device_combo")
            .width(220.0)
            .selected_text(current)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(selected_device.is_none(), "System default")
                    .clicked()
                {
                    self.controller.set_audio_device(None);
                }
                for device in &devices {
                    let selected = selected_device.as_deref() == Some(device.name.as_str());
                    if ui.selectable_label(selected, &device.name).clicked() {
                        self.controller.set_audio_device(Some(device.name.clone()));
                    }
                }
            });
    }

    pub(super) fn render_audio_input_device_combo(&mut self, ui: &mut egui::Ui) {
        let selected_device = self.controller.ui.audio.input_selected.device.clone();
        let devices = self.controller.ui.audio.input_devices.clone();
        let current = selected_device
            .clone()
            .unwrap_or_else(|| "System default".to_string());
        egui::ComboBox::from_id_salt("audio_input_device_combo")
            .width(220.0)
            .selected_text(current)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(selected_device.is_none(), "System default")
                    .clicked()
                {
                    self.controller.set_audio_input_device(None);
                }
                for device in &devices {
                    let selected = selected_device.as_deref() == Some(device.name.as_str());
                    if ui.selectable_label(selected, &device.name).clicked() {
                        self.controller.set_audio_input_device(Some(device.name.clone()));
                    }
                }
            });
    }

    pub(super) fn render_audio_sample_rate_combo(&mut self, ui: &mut egui::Ui) {
        let selected_rate = self.controller.ui.audio.selected.sample_rate;
        let sample_rates = self.controller.ui.audio.sample_rates.clone();
        let selected = selected_rate
            .map(|rate| format!("{rate} Hz"))
            .unwrap_or_else(|| "Device default".to_string());
        egui::ComboBox::from_id_salt("audio_sample_rate_combo")
            .width(220.0)
            .selected_text(selected)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(selected_rate.is_none(), "Device default")
                    .clicked()
                {
                    self.controller.set_audio_sample_rate(None);
                }
                for rate in &sample_rates {
                    let label = format!("{rate} Hz");
                    let selected = selected_rate == Some(*rate);
                    if ui.selectable_label(selected, label).clicked() {
                        self.controller.set_audio_sample_rate(Some(*rate));
                    }
                }
            });
    }

    pub(super) fn render_audio_input_sample_rate_combo(&mut self, ui: &mut egui::Ui) {
        let selected_rate = self.controller.ui.audio.input_selected.sample_rate;
        let sample_rates = self.controller.ui.audio.input_sample_rates.clone();
        let selected = selected_rate
            .map(|rate| format!("{rate} Hz"))
            .unwrap_or_else(|| "Device default".to_string());
        egui::ComboBox::from_id_salt("audio_input_sample_rate_combo")
            .width(220.0)
            .selected_text(selected)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(selected_rate.is_none(), "Device default")
                    .clicked()
                {
                    self.controller.set_audio_input_sample_rate(None);
                }
                for rate in &sample_rates {
                    let label = format!("{rate} Hz");
                    let selected = selected_rate == Some(*rate);
                    if ui.selectable_label(selected, label).clicked() {
                        self.controller.set_audio_input_sample_rate(Some(*rate));
                    }
                }
            });
    }

    pub(super) fn render_audio_input_channel_checkboxes(&mut self, ui: &mut egui::Ui) {
        let channel_counts = self.controller.ui.audio.input_channel_counts.clone();
        if channel_counts.is_empty() {
            return;
        }
        let selected = self.controller.ui.audio.input_selected.channels;
        let has_mono = channel_counts.contains(&1);
        let has_stereo = channel_counts.contains(&2);
        let stereo_selected = selected == Some(2);
        ui.label("Input channels");
        ui.horizontal(|ui| {
            let mut mono_checked = selected == Some(1);
            let mono_response = ui.add_enabled(
                has_mono && !stereo_selected,
                egui::Checkbox::new(&mut mono_checked, "Mono (1 input)"),
            );
            if mono_response.changed() && mono_checked {
                self.controller.set_audio_input_channels(1);
            }

            let mut stereo_checked = stereo_selected;
            let stereo_response = ui.add_enabled(
                has_stereo,
                egui::Checkbox::new(&mut stereo_checked, "Stereo (2 inputs)"),
            );
            if stereo_response.changed() {
                if stereo_checked {
                    self.controller.set_audio_input_channels(2);
                } else if has_mono {
                    self.controller.set_audio_input_channels(1);
                }
            }
        });
    }

    pub(super) fn render_audio_buffer_combo(&mut self, ui: &mut egui::Ui) {
        let selected_buffer = self.controller.ui.audio.selected.buffer_size;
        let selected = selected_buffer
            .map(|frames| format!("{frames} frames"))
            .unwrap_or_else(|| "Driver default".to_string());
        egui::ComboBox::from_id_salt("audio_buffer_combo")
            .width(220.0)
            .selected_text(selected)
            .show_ui(ui, |ui| {
                let options: [Option<u32>; 6] = [
                    None,
                    Some(256),
                    Some(512),
                    Some(1024),
                    Some(2048),
                    Some(4096),
                ];
                for option in options {
                    let label = option
                        .map(|frames| format!("{frames} frames"))
                        .unwrap_or_else(|| "Driver default".to_string());
                    let selected = selected_buffer == option;
                    if ui.selectable_label(selected, label).clicked() {
                        self.controller.set_audio_buffer_size(option);
                    }
                }
            });
    }
}
