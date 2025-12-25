use eframe::egui::{self, RichText, SliderClamping};

use super::super::EguiApp;
use super::super::style;
use super::section_label;

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
                section_label(ui, "Waveform & Zoom");
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
                section_label(ui, "Playback");
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
                ui.add_space(6.0);
                ui.separator();
                self.render_gpu_embeddings_panel(ui);
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
}
