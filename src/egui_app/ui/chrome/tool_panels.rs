use eframe::egui::{self, RichText, SliderClamping};

use super::super::EguiApp;
use super::super::style;

impl EguiApp {
    pub(super) fn render_audio_settings_window(&mut self, ctx: &egui::Context) {
        if !self.controller.ui.audio.panel_open {
            return;
        }
        let mut open = true;
        egui::Window::new("Options")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(320.0)
            .show(ctx, |ui| {
                ui.set_min_width(300.0);
                self.render_audio_host_combo(ui);
                self.render_audio_device_combo(ui);
                self.render_audio_sample_rate_combo(ui);
                self.render_audio_buffer_combo(ui);
                if let Some(applied) = &self.controller.ui.audio.applied {
                    let buffer = applied
                        .buffer_size_frames
                        .map(|frames| format!(", buffer {frames}"))
                        .unwrap_or_default();
                    let host_label = applied.host_id.to_uppercase();
                    ui.label(
                        RichText::new(format!(
                            "Active: {} via {} @ {} Hz ({} ch{buffer})",
                            applied.device_name,
                            host_label,
                            applied.sample_rate,
                            applied.channel_count
                        ))
                        .color(style::palette().text_muted),
                    );
                }
                if let Some(current_warning) = self.controller.ui.audio.warning.as_ref() {
                    ui.label(
                        RichText::new(current_warning.clone())
                            .color(style::status_badge_color(style::StatusTone::Warning)),
                    );
                }
                ui.separator();
                ui.label(
                    RichText::new("Waveform & Zoom")
                        .strong()
                        .color(style::palette().text_primary),
                );
                let mut invert_scroll = self.controller.ui.controls.invert_waveform_scroll;
                if ui
                    .checkbox(
                        &mut invert_scroll,
                        "Invert horizontal scroll (Shift + wheel)",
                    )
                    .clicked()
                {
                    self.controller.set_invert_waveform_scroll(invert_scroll);
                }
                let mut scroll_speed = self.controller.ui.controls.waveform_scroll_speed;
                let scroll_slider = egui::Slider::new(&mut scroll_speed, 0.2..=3.0)
                    .logarithmic(true)
                    .text("Scroll speed")
                    .suffix("×");
                if ui.add(scroll_slider).changed() {
                    self.controller.set_waveform_scroll_speed(scroll_speed);
                }
                let mut wheel_zoom_speed = self.controller.wheel_zoom_speed();
                let wheel_slider = egui::Slider::new(&mut wheel_zoom_speed, 0.1..=20.0)
                    .logarithmic(true)
                    .text("Wheel zoom speed")
                    .suffix("×")
                    .clamping(SliderClamping::Always);
                if ui.add(wheel_slider).changed() {
                    self.controller.set_wheel_zoom_speed(wheel_zoom_speed);
                }
                let mut keyboard_zoom = self.controller.ui.controls.keyboard_zoom_factor;
                let keyboard_slider = egui::Slider::new(&mut keyboard_zoom, 0.5..=0.995)
                    .text("Keyboard zoom factor")
                    .clamping(SliderClamping::Always);
                if ui.add(keyboard_slider).changed() {
                    self.controller.set_keyboard_zoom_factor(keyboard_zoom);
                }
                ui.add_space(6.0);
                ui.separator();
                ui.label(
                    RichText::new("Playback")
                        .strong()
                        .color(style::palette().text_primary),
                );
                let mut anti_clip_enabled = self.controller.ui.controls.anti_clip_fade_enabled;
                if ui.checkbox(&mut anti_clip_enabled, "Anti-click fade").changed() {
                    self.controller.set_anti_clip_fade_enabled(anti_clip_enabled);
                }
                let mut anti_clip_fade_ms = self.controller.ui.controls.anti_clip_fade_ms;
                let anti_clip_slider = egui::Slider::new(&mut anti_clip_fade_ms, 0.0..=20.0)
                    .text("Fade length")
                    .suffix(" ms");
                if ui.add_enabled(anti_clip_enabled, anti_clip_slider).changed() {
                    self.controller.set_anti_clip_fade_ms(anti_clip_fade_ms);
                }
                ui.add_space(6.0);
                let mut yolo_mode = self.controller.ui.controls.destructive_yolo_mode;
                let yolo_label = RichText::new(
                    "Yolo mode: apply destructive edits without confirmation",
                )
                .color(style::destructive_text());
                if ui.checkbox(&mut yolo_mode, yolo_label).changed() {
                    self.controller.set_destructive_yolo_mode(yolo_mode);
                }
                ui.label(
                    RichText::new(
                        "When off, crop/trim/fade/mute/normalize will ask before overwriting audio.",
                    )
                    .color(style::status_badge_color(style::StatusTone::Warning)),
                );
            });
        self.controller.ui.audio.panel_open = open;
    }

    pub(super) fn render_audio_options_menu(&mut self, ui: &mut egui::Ui) {
        let palette = style::palette();
        ui.label(
            RichText::new("Audio output")
                .strong()
                .color(palette.text_primary),
        );
        let summary = self.controller.ui.audio.applied.as_ref().map_or_else(
            || "Not initialized".to_string(),
            |applied| {
                let buffer = applied
                    .buffer_size_frames
                    .map(|frames| format!(", buffer {frames}"))
                    .unwrap_or_default();
                format!(
                    "{} via {} @ {} Hz ({} ch{buffer})",
                    applied.device_name,
                    applied.host_id.to_uppercase(),
                    applied.sample_rate,
                    applied.channel_count
                )
            },
        );
        ui.label(RichText::new(summary).color(palette.text_muted));
        if ui.button("Open options…").clicked() {
            self.controller.ui.audio.panel_open = true;
            self.controller.refresh_audio_options();
        }
        if let Some(warning) = &self.controller.ui.audio.warning {
            ui.label(
                RichText::new(warning).color(style::status_badge_color(style::StatusTone::Warning)),
            );
        }
    }

    pub(super) fn render_analysis_options_menu(&mut self, ui: &mut egui::Ui) {
        let palette = style::palette();
        ui.label(
            RichText::new("Analysis")
                .strong()
                .color(palette.text_primary),
        );
        ui.label(
            RichText::new("Skip feature extraction for files longer than:")
                .color(palette.text_muted),
        );
        let mut seconds = self.controller.max_analysis_duration_seconds();
        let drag = egui::DragValue::new(&mut seconds)
            .speed(1.0)
            .range(1.0..=3600.0)
            .suffix(" s");
        let response = ui
            .add(drag)
            .on_hover_text("Long songs/loops can be expensive to decode and analyze");
        if response.changed() {
            self.controller.set_max_analysis_duration_seconds(seconds);
        }

        ui.add_space(ui.spacing().item_spacing.y);
        ui.label(RichText::new("Analysis workers (0 = auto):").color(palette.text_muted));
        let mut workers = self.controller.analysis_worker_count() as i64;
        let drag = egui::DragValue::new(&mut workers).range(0..=64);
        let response = ui
            .add(drag)
            .on_hover_text("Limit background CPU usage (change takes effect on next start)");
        if response.changed() {
            self.controller
                .set_analysis_worker_count(workers.max(0) as u32);
        }

        ui.add_space(ui.spacing().item_spacing.y);
        ui.separator();
        ui.label(
            RichText::new("GPU embeddings")
                .strong()
                .color(palette.text_primary),
        );
        let mut backend = self.controller.panns_backend();
        egui::ComboBox::from_id_salt("panns_backend_combo")
            .selected_text(match backend {
                crate::sample_sources::config::PannsBackendChoice::Wgpu => "WGPU (Vulkan)",
                crate::sample_sources::config::PannsBackendChoice::Cuda => "CUDA",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut backend,
                    crate::sample_sources::config::PannsBackendChoice::Wgpu,
                    "WGPU (Vulkan)",
                );
                let cuda_enabled = cfg!(feature = "panns-cuda");
                ui.add_enabled(
                    cuda_enabled,
                    egui::SelectableLabel::new(
                        backend == crate::sample_sources::config::PannsBackendChoice::Cuda,
                        "CUDA",
                    ),
                )
                .on_disabled_hover_text("CUDA backend not enabled in this build");
            });
        if backend != self.controller.panns_backend() {
            self.controller.set_panns_backend(backend);
        }

        let wgpu_active = backend == crate::sample_sources::config::PannsBackendChoice::Wgpu;
        ui.add_enabled_ui(wgpu_active, |ui| {
            let mut power = self.controller.wgpu_power_preference();
            let power_combo = egui::ComboBox::from_id_salt("wgpu_power_combo")
                .selected_text(match power {
                    crate::sample_sources::config::WgpuPowerPreference::Default => "Default",
                    crate::sample_sources::config::WgpuPowerPreference::Low => "Low power",
                    crate::sample_sources::config::WgpuPowerPreference::High => "High performance",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut power,
                        crate::sample_sources::config::WgpuPowerPreference::Default,
                        "Default",
                    );
                    ui.selectable_value(
                        &mut power,
                        crate::sample_sources::config::WgpuPowerPreference::Low,
                        "Low power",
                    );
                    ui.selectable_value(
                        &mut power,
                        crate::sample_sources::config::WgpuPowerPreference::High,
                        "High performance",
                    );
                })
                .response;
            if power_combo.changed() {
                self.controller.set_wgpu_power_preference(power);
            }

            let mut adapter_name = self
                .controller
                .wgpu_adapter_name()
                .unwrap_or_default()
                .to_string();
            let adapter_edit = ui
                .add(
                    egui::TextEdit::singleline(&mut adapter_name)
                        .hint_text("Adapter name filter (optional)"),
                )
                .on_hover_text("Match a GPU adapter name substring (WGPU only)");
            if adapter_edit.changed() {
                self.controller.set_wgpu_adapter_name(adapter_name);
            }
        });

        ui.label(
            RichText::new("Changes apply on next start.")
                .color(palette.text_muted),
        );
        if let Ok(value) = std::env::var("SEMPAL_PANNS_BACKEND") {
            if !value.trim().is_empty() {
                ui.label(
                    RichText::new(format!("Env override: SEMPAL_PANNS_BACKEND={}", value.trim()))
                        .color(palette.text_muted),
                );
            }
        }
        if let Ok(value) = std::env::var("WGPU_ADAPTER_NAME") {
            if !value.trim().is_empty() {
                ui.label(
                    RichText::new(format!("Env override: WGPU_ADAPTER_NAME={}", value.trim()))
                        .color(palette.text_muted),
                );
            }
        }
        if let Ok(value) = std::env::var("WGPU_POWER_PREFERENCE") {
            if !value.trim().is_empty() {
                ui.label(
                    RichText::new(format!(
                        "Env override: WGPU_POWER_PREFERENCE={}",
                        value.trim()
                    ))
                    .color(palette.text_muted),
                );
            }
        }
    }

    fn render_audio_host_combo(&mut self, ui: &mut egui::Ui) {
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

    fn render_audio_device_combo(&mut self, ui: &mut egui::Ui) {
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

    fn render_audio_sample_rate_combo(&mut self, ui: &mut egui::Ui) {
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

    fn render_audio_buffer_combo(&mut self, ui: &mut egui::Ui) {
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
