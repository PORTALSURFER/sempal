use super::hotkey_runtime::format_keypress;
use super::style;
use super::*;
use eframe::egui::{self, Frame, Margin, RichText, SliderClamping, StrokeKind};

impl EguiApp {
    fn log_viewport_info(&mut self, ctx: &egui::Context) {
        let (inner, monitor, fullscreen, maximized) = ctx.input(|i| {
            (
                i.viewport().inner_rect,
                i.viewport().monitor_size,
                i.viewport().fullscreen,
                i.viewport().maximized,
            )
        });
        if let (Some(inner), Some(mon)) = (inner, monitor) {
            let mode = if fullscreen == Some(true) {
                "fullscreen"
            } else if maximized == Some(true) {
                "maximized"
            } else {
                "windowed"
            };
            let dims = (
                inner.width().round() as u32,
                inner.height().round() as u32,
                mon.x.round() as u32,
                mon.y.round() as u32,
                mode,
            );
            if Some(dims) != self.last_viewport_log {
                println!(
                    "mode: {:<10} | viewport: {} x {} | monitor: {} x {}",
                    dims.4, dims.0, dims.1, dims.2, dims.3
                );
                self.last_viewport_log = Some(dims);
            }
        }
    }

    pub(super) fn render_status(&mut self, ctx: &egui::Context) {
        self.log_viewport_info(ctx);
        let palette = style::palette();
        egui::TopBottomPanel::bottom("status_bar")
            .frame(
                Frame::new()
                    .fill(palette.bg_primary)
                    .stroke(style::section_stroke())
                    .inner_margin(Margin::symmetric(8, 4)),
            )
            .show(ctx, |ui| {
                let status = self.controller.ui.status.clone();
                let chord_label = self.chord_status_label();
                let key_label = format_keypress(&self.key_feedback.last_key);
                ui.columns(3, |columns| {
                    columns[0].vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.add_space(6.0);
                            let (badge_rect, _) = ui
                                .allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                            ui.painter()
                                .rect_filled(badge_rect, 0.0, status.badge_color);
                            ui.painter().rect_stroke(
                                badge_rect,
                                0.0,
                                style::inner_border(),
                                StrokeKind::Inside,
                            );
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(&status.badge_label).color(palette.text_primary),
                            );
                            ui.separator();
                            ui.label(RichText::new(&status.text).color(palette.text_primary));
                        });
                    });
                    columns[1].horizontal(|ui| {
                        ui.add_space(6.0);
                        ui.label(RichText::new("Key").color(palette.text_primary));
                        ui.separator();
                        ui.label(RichText::new(key_label).color(palette.text_primary));
                        ui.separator();
                        ui.label(RichText::new("Chord").color(palette.text_primary));
                        ui.separator();
                        ui.label(RichText::new(chord_label).color(palette.text_primary));
                    });
                    columns[2].with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            let mut close_menu = false;
                            ui.menu_button("Options", |ui| {
                                let palette = style::palette();
                                ui.label(
                                    RichText::new("Collection export root")
                                        .color(palette.text_primary),
                                );
                                let export_label = self
                                    .controller
                                    .ui
                                    .collection_export_root
                                    .as_ref()
                                    .map(|p| p.display().to_string())
                                    .unwrap_or_else(|| "Not set".to_string());
                                ui.label(RichText::new(export_label).color(palette.text_muted));
                                if ui.button("Choose collection export root...").clicked() {
                                    self.controller.pick_collection_export_root();
                                    close_menu = true;
                                }
                                if ui.button("Open collection export root").clicked() {
                                    self.controller.open_collection_export_root();
                                    close_menu = true;
                                }
                                if ui.button("Clear collection export root").clicked() {
                                    self.controller.clear_collection_export_root();
                                    close_menu = true;
                                }
                                ui.separator();
                                ui.label(RichText::new("Trash folder").color(palette.text_primary));
                                let trash_label = self
                                    .controller
                                    .ui
                                    .trash_folder
                                    .as_ref()
                                    .map(|p| p.display().to_string())
                                    .unwrap_or_else(|| "Not set".to_string());
                                ui.label(RichText::new(trash_label).color(palette.text_muted));
                                if ui.button("Choose trash folder...").clicked() {
                                    self.controller.pick_trash_folder();
                                    close_menu = true;
                                }
                                if ui.button("Open trash folder").clicked() {
                                    self.controller.open_trash_folder();
                                    close_menu = true;
                                }
                                if ui.button("Open config folder").clicked() {
                                    self.controller.open_config_folder();
                                    close_menu = true;
                                }
                                if ui.button("Check for updates").clicked() {
                                    self.controller.check_for_updates_now();
                                    close_menu = true;
                                }
                                ui.separator();
                                self.render_audio_options_menu(ui);
                                ui.separator();
                                self.render_analysis_options_menu(ui);
                                ui.separator();
                                self.render_model_options_menu(ui);
                                ui.separator();
                                if ui.button("Move trashed samples to folder").clicked() {
                                    self.controller.move_all_trashed_to_folder();
                                    close_menu = true;
                                }
                                let take_out = egui::Button::new(
                                    RichText::new("Take out trash")
                                        .color(style::destructive_text()),
                                );
                                if ui.add(take_out).clicked() {
                                    self.controller.take_out_trash();
                                    close_menu = true;
                                }
                                if close_menu {
                                    ui.close();
                                }
                            });
                            ui.add_space(10.0);
                            const APP_VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));
                            match self.controller.ui.update.status {
                                crate::egui_app::state::UpdateStatus::Checking => {
                                    ui.label(
                                        RichText::new("Checking updates…")
                                            .color(palette.text_muted),
                                    );
                                    ui.add_space(10.0);
                                }
                                crate::egui_app::state::UpdateStatus::UpdateAvailable => {
                                    let label = self
                                        .controller
                                        .ui
                                        .update
                                        .available_tag
                                        .clone()
                                        .unwrap_or_else(|| "Update available".to_string());
                                    ui.label(
                                        RichText::new("Update available")
                                            .color(style::destructive_text())
                                            .strong(),
                                    );
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new("Current:").color(palette.text_muted),
                                        );
                                        ui.label(
                                            RichText::new(APP_VERSION).color(palette.text_muted),
                                        );
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("New:").color(palette.text_muted));
                                        ui.label(
                                            RichText::new(&label)
                                                .color(style::destructive_text())
                                                .strong(),
                                        );
                                    });
                                    if ui.button("Open update page").clicked() {
                                        self.controller.open_update_link();
                                    }
                                    if ui.button("Install").clicked() {
                                        self.controller.install_update_and_exit();
                                    }
                                    if ui.button("Dismiss").clicked() {
                                        self.controller.dismiss_update_notification();
                                    }
                                    ui.add_space(10.0);
                                }
                                crate::egui_app::state::UpdateStatus::Error => {
                                    if ui.button("Update check failed").clicked() {
                                        self.controller.check_for_updates_now();
                                    }
                                    ui.add_space(10.0);
                                }
                                crate::egui_app::state::UpdateStatus::Idle => {}
                            }
                            if !matches!(
                                self.controller.ui.update.status,
                                crate::egui_app::state::UpdateStatus::UpdateAvailable
                            ) {
                                ui.label(RichText::new(APP_VERSION).color(palette.text_muted));
                            }
                            ui.add_space(10.0);
                            let mut volume = self.controller.ui.volume;
                            let slider = egui::Slider::new(&mut volume, 0.0..=1.0)
                                .text("Vol")
                                .clamping(SliderClamping::Always);
                            if ui.add(slider).changed() {
                                self.controller.set_volume(volume);
                            }
                            if self.controller.ui.progress.visible {
                                ui.add_space(10.0);
                                let progress = &self.controller.ui.progress;
                                let fraction = progress.fraction();
                                let mut bar = egui::ProgressBar::new(fraction)
                                    .desired_width(180.0)
                                    .animate(true);
                                bar = bar.fill(style::status_badge_color(style::StatusTone::Busy));
                                bar = if progress.total > 0 {
                                    bar.text(format!(
                                        "{} / {}",
                                        progress.completed.min(progress.total),
                                        progress.total
                                    ))
                                } else if progress.task
                                    == Some(crate::egui_app::state::ProgressTaskKind::Scan)
                                    && progress.completed > 0
                                {
                                    bar.text(format!("{} files", progress.completed))
                                } else {
                                    bar.text("Working…")
                                };
                                let tooltip = match progress.detail.as_deref() {
                                    Some(detail) => format!("{}\n{}", progress.title, detail),
                                    None => progress.title.clone(),
                                };
                                ui.add(bar).on_hover_text(tooltip);
                                if progress.cancelable {
                                    let label = if progress.cancel_requested {
                                        "Canceling…"
                                    } else {
                                        "Cancel"
                                    };
                                    if ui
                                        .add_enabled(
                                            !progress.cancel_requested,
                                            egui::Button::new(label),
                                        )
                                        .clicked()
                                    {
                                        self.controller.ui.progress.cancel_requested = true;
                                    }
                                }
                            }
                        },
                    );
                });
            });
        self.render_audio_settings_window(ctx);
    }

    fn chord_status_label(&self) -> String {
        if let Some(pending) = self.key_feedback.pending_root {
            return format!("{} …", format_keypress(&Some(pending)));
        }
        if let Some((first, second)) = self.key_feedback.last_chord {
            return format!(
                "{} + {}",
                format_keypress(&Some(first)),
                format_keypress(&Some(second))
            );
        }
        "—".to_string()
    }

    fn render_audio_options_menu(&mut self, ui: &mut egui::Ui) {
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

    fn render_analysis_options_menu(&mut self, ui: &mut egui::Ui) {
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
        ui.label(
            RichText::new("Analysis workers (0 = auto):").color(palette.text_muted),
        );
        let mut workers = self.controller.analysis_worker_count() as i64;
        let drag = egui::DragValue::new(&mut workers).range(0..=64);
        let response = ui
            .add(drag)
            .on_hover_text("Limit background CPU usage (change takes effect on next start)");
        if response.changed() {
            self.controller
                .set_analysis_worker_count(workers.max(0) as u32);
        }
    }

    fn render_model_options_menu(&mut self, ui: &mut egui::Ui) {
        let palette = style::palette();
        ui.label(RichText::new("Model").strong().color(palette.text_primary));
        ui.label(RichText::new("Assign UNKNOWN below confidence:").color(palette.text_muted));
        let mut unknown = self.controller.unknown_confidence_threshold();
        let slider = egui::Slider::new(&mut unknown, 0.0..=1.0)
            .text("Unknown")
            .clamping(SliderClamping::Always);
        if ui.add(slider).changed() {
            self.controller.set_unknown_confidence_threshold(unknown);
        }
        let mut use_overrides = self.controller.use_user_overrides_in_browser();
        if ui
            .checkbox(&mut use_overrides, "Prefer user overrides")
            .on_hover_text("Show manual category overrides instead of predictions")
            .clicked()
        {
            self.controller.set_use_user_overrides_in_browser(use_overrides);
        }
        if ui
            .button("Re-run inference (loaded sources)")
            .on_hover_text("Clear old predictions and recompute for loaded sources")
            .clicked()
        {
            self.controller.rerun_inference_for_loaded_sources();
        }
    }

    fn render_audio_settings_window(&mut self, ctx: &egui::Context) {
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
                self.render_model_options_menu(ui);
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
                let mut yolo_mode = self.controller.ui.controls.destructive_yolo_mode;
                let yolo_label = RichText::new("Yolo mode: apply destructive edits without confirmation")
                    .color(style::destructive_text());
                if ui.checkbox(&mut yolo_mode, yolo_label).changed() {
                    self.controller.set_destructive_yolo_mode(yolo_mode);
                }
                ui.label(
                    RichText::new("When off, crop/trim/fade/mute/normalize will ask before overwriting audio.")
                        .color(style::status_badge_color(style::StatusTone::Warning)),
                );
            });
        self.controller.ui.audio.panel_open = open;
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
