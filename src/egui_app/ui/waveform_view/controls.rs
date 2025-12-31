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
        let is_recording = app.controller.is_recording();
        let record_label = if is_recording {
            RichText::new("Record: On").color(style::destructive_text())
        } else {
            RichText::new("Record").color(palette.text_muted)
        };
        let record_button = ui
            .add(egui::Button::new(record_label))
            .on_hover_text("Record a new stereo take into the recordings folder");
        if record_button.clicked() {
            let result = if is_recording {
                app.controller.stop_recording_and_load()
            } else {
                app.controller.start_recording()
            };
            if let Err(err) = result {
                app.controller.set_status(err, style::StatusTone::Error);
            }
        }
        let is_playing = app.controller.is_playing();
        let play_button = ui
            .add_enabled(
                !is_recording,
                egui::Button::new(RichText::new("Play").color(palette.text_muted)),
            )
            .on_hover_text("Play from the current selection or cursor");
        if play_button.clicked() {
            if let Err(err) =
                app.controller
                    .play_audio(app.controller.ui.waveform.loop_enabled, None)
            {
                app.controller.set_status(err, style::StatusTone::Error);
            }
        }
        let stop_button = ui
            .add_enabled(
                is_playing,
                egui::Button::new(RichText::new("Stop").color(palette.text_muted)),
            )
            .on_hover_text("Stop playback");
        if stop_button.clicked() {
            app.controller.stop_playback_if_active();
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
        let transient_count = app.controller.ui.waveform.transients.len();
        ui.label(format!("Transients: {transient_count}"));
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
