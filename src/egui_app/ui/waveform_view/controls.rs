use super::style;
use super::*;
use eframe::egui::{self, RichText, Ui};

pub(super) fn render_waveform_controls(app: &mut EguiApp, ui: &mut Ui, palette: &style::Palette) {
    let mut view_mode = app.controller.ui.waveform.channel_view;
    ui.horizontal(|ui| {
        let mono = ui.selectable_value(
            &mut view_mode,
            crate::waveform::WaveformChannelView::Mono,
            "Mono envelope",
        );
        mono.on_hover_text("Show peak envelope across all channels");
        let split = ui.selectable_value(
            &mut view_mode,
            crate::waveform::WaveformChannelView::SplitStereo,
            "Split L/R",
        );
        split.on_hover_text("Render the first two channels separately");
        ui.add_space(10.0);
        let loop_enabled = app.controller.ui.waveform.loop_enabled;
        let loop_label = if loop_enabled {
            RichText::new("Loop: On").color(palette.accent_mint)
        } else {
            RichText::new("Loop: Off").color(palette.text_muted)
        };
        if ui
            .add(egui::Button::new(loop_label))
            .on_hover_text("Toggle loop playback for the current selection (or whole sample)")
            .clicked()
        {
            app.controller.toggle_loop();
        }
        ui.add_space(10.0);
        let mut bpm_enabled = app.controller.ui.waveform.bpm_snap_enabled;
        if ui.checkbox(&mut bpm_enabled, "BPM snap").clicked() {
            let prev_value = app.controller.ui.waveform.bpm_value;
            app.controller.set_bpm_snap_enabled(bpm_enabled);
            if bpm_enabled && prev_value.is_none() {
                let fallback = 142.0;
                app.controller.set_bpm_value(fallback);
                app.controller.ui.waveform.bpm_value = Some(fallback);
                app.controller.ui.waveform.bpm_input = format!("{fallback:.0}");
            }
        }
        let bpm_edit = ui.add_enabled(
            app.controller.ui.waveform.bpm_snap_enabled,
            egui::TextEdit::singleline(&mut app.controller.ui.waveform.bpm_input)
                .desired_width(64.0)
                .hint_text("120"),
        );
        if bpm_edit.changed() {
            let parsed = parse_bpm_input(&app.controller.ui.waveform.bpm_input);
            app.controller.ui.waveform.bpm_value = parsed;
            if let Some(value) = parsed {
                app.controller.set_bpm_value(value);
            }
        }
    });
    ui.horizontal(|ui| {
        let mut show_transients = app.controller.ui.waveform.transient_markers_enabled;
        if ui
            .checkbox(&mut show_transients, "Show transients")
            .clicked()
        {
            app.controller.set_transient_markers_enabled(show_transients);
        }
        let mut transient_snap = app.controller.ui.waveform.transient_snap_enabled;
        let snap_toggle = ui.add_enabled(
            app.controller.ui.waveform.transient_markers_enabled,
            egui::Checkbox::new(&mut transient_snap, "Transient snap"),
        );
        if snap_toggle.clicked() {
            app.controller.set_transient_snap_enabled(transient_snap);
        }
        let custom_tuning = app.controller.ui.waveform.transient_use_custom_tuning;
        ui.add_enabled_ui(!custom_tuning, |ui| {
            let mut sensitivity = app.controller.ui.waveform.transient_sensitivity_draft;
            let slider = egui::Slider::new(&mut sensitivity, 0.0..=1.0)
                .text("Sensitivity")
                .fixed_decimals(2)
                .step_by(0.01);
            if ui.add(slider).changed() {
                app.controller.ui.waveform.transient_sensitivity_draft = sensitivity;
                if app.controller.ui.waveform.transient_realtime_enabled {
                    app.controller.apply_transient_sensitivity(sensitivity);
                }
            }
            let mut realtime = app.controller.ui.waveform.transient_realtime_enabled;
            if ui.checkbox(&mut realtime, "Realtime").clicked() {
                app.controller.set_transient_realtime_enabled(realtime);
            }
            let can_apply = (app.controller.ui.waveform.transient_sensitivity
                - app.controller.ui.waveform.transient_sensitivity_draft)
                .abs()
                > f32::EPSILON;
            let apply = ui.add_enabled(
                can_apply && !app.controller.ui.waveform.transient_realtime_enabled,
                egui::Button::new("Apply"),
            );
            if apply.clicked() {
                let value = app.controller.ui.waveform.transient_sensitivity_draft;
                app.controller.apply_transient_sensitivity(value);
            }
        });
        if custom_tuning {
            ui.label("Sensitivity disabled while custom tuning is enabled.");
        }
        let transient_count = app.controller.ui.waveform.transients.len();
        ui.label(format!("Transients: {transient_count}"));
    });
    ui.collapsing("Transient tuning", |ui| {
        let mut use_custom = app.controller.ui.waveform.transient_use_custom_tuning;
        if ui.checkbox(&mut use_custom, "Use custom tuning").clicked() {
            app.controller.set_transient_use_custom_tuning(use_custom);
        }
        let slider_enabled = app.controller.ui.waveform.transient_use_custom_tuning;
        ui.add_enabled_ui(slider_enabled, |ui| {
            let mut k_high = app.controller.ui.waveform.transient_k_high;
            if ui
                .add(
                    egui::Slider::new(&mut k_high, 1.0..=12.0)
                        .text("k high")
                        .fixed_decimals(2)
                        .step_by(0.05),
                )
                .changed()
            {
                app.controller.set_transient_k_high(k_high);
            }
            let mut k_low = app.controller.ui.waveform.transient_k_low;
            if ui
                .add(
                    egui::Slider::new(&mut k_low, 0.5..=8.0)
                        .text("k low")
                        .fixed_decimals(2)
                        .step_by(0.05),
                )
                .changed()
            {
                app.controller.set_transient_k_low(k_low);
            }
            let mut floor = app.controller.ui.waveform.transient_floor_quantile;
            if ui
                .add(
                    egui::Slider::new(&mut floor, 0.1..=0.9)
                        .text("floor quantile")
                        .fixed_decimals(2)
                        .step_by(0.01),
                )
                .changed()
            {
                app.controller.set_transient_floor_quantile(floor);
            }
            let mut min_gap = app.controller.ui.waveform.transient_min_gap_seconds;
            if ui
                .add(
                    egui::Slider::new(&mut min_gap, 0.02..=0.2)
                        .text("min gap (s)")
                        .fixed_decimals(3)
                        .step_by(0.005),
                )
                .changed()
            {
                app.controller.set_transient_min_gap_seconds(min_gap);
            }
        });
    });
    if view_mode != app.controller.ui.waveform.channel_view {
        app.controller.set_waveform_channel_view(view_mode);
    }
}

fn parse_bpm_input(input: &str) -> Option<f32> {
    let trimmed = input.trim().to_lowercase();
    let trimmed = trimmed.strip_suffix("bpm").unwrap_or(trimmed.as_str()).trim();
    let bpm = trimmed.parse::<f32>().ok()?;
    if bpm.is_finite() && bpm > 0.0 {
        Some(bpm)
    } else {
        None
    }
}
