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
        let waveform = &mut app.controller.ui.waveform;
        let mut bpm_enabled = waveform.bpm_snap_enabled;
        if ui.checkbox(&mut bpm_enabled, "BPM snap").clicked() {
            app.controller.set_bpm_snap_enabled(bpm_enabled);
            if bpm_enabled && waveform.bpm_value.is_none() {
                let fallback = 142.0;
                app.controller.set_bpm_value(fallback);
                waveform.bpm_value = Some(fallback);
                waveform.bpm_input = format!("{fallback:.0}");
            }
        }
        let bpm_edit = ui.add_enabled(
            waveform.bpm_snap_enabled,
            egui::TextEdit::singleline(&mut waveform.bpm_input)
                .desired_width(64.0)
                .hint_text("120"),
        );
        if bpm_edit.changed() {
            let parsed = parse_bpm_input(&waveform.bpm_input);
            waveform.bpm_value = parsed;
            if let Some(value) = parsed {
                app.controller.set_bpm_value(value);
            }
        }
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
