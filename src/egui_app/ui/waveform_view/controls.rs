use super::helpers;
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
        mono.on_hover_text("Downmix channels to a single mono waveform");
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
        let audition_enabled = app.controller.ui.waveform.normalized_audition_enabled;
        let audition_label = if audition_enabled {
            RichText::new("Audition: Norm").color(palette.accent_mint)
        } else {
            RichText::new("Audition: Raw").color(palette.text_muted)
        };
        if ui
            .add(egui::Button::new(audition_label))
            .on_hover_text("Normalize playback to full scale for the current span")
            .clicked()
        {
            app.controller
                .set_normalized_audition_enabled(!audition_enabled);
        }
        ui.add_space(10.0);
        let is_recording = app.controller.is_recording();
        let has_source = app.controller.current_source().is_some();
        let record_size = eframe::egui::Vec2::new(32.0, 24.0);
        let record_sense = if is_recording || has_source {
            eframe::egui::Sense::click()
        } else {
            eframe::egui::Sense::hover()
        };
        let (record_rect, record_response) = ui.allocate_exact_size(record_size, record_sense);
        
        // Custom painting for button frame
        let record_visuals = ui.style().interact(&record_response);
        ui.painter().rect(
            record_rect,
            record_visuals.corner_radius,
            record_visuals.bg_fill,
            record_visuals.bg_stroke,
            eframe::egui::StrokeKind::Inside,
        );

        // Custom painting for Circle icon
        let circle_color = if is_recording {
             style::destructive_text()
        } else if has_source {
             palette.text_muted
        } else {
            ui.visuals().widgets.noninteractive.fg_stroke.color.linear_multiply(0.3)
        };
        
        let center = record_rect.center();
        let radius = 6.0;
        ui.painter().circle_filled(center, radius, circle_color);

        let record_button = record_response.on_hover_text("Record into the selected source folder");
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
        let mut monitor_enabled = app.controller.ui.controls.input_monitoring_enabled;
        let monitor_label = if monitor_enabled {
            RichText::new("Monitor: On").color(palette.accent_mint)
        } else {
            RichText::new("Monitor: Off").color(palette.text_muted)
        };
        let monitor_button = ui
            .add(egui::Button::new(monitor_label))
            .on_hover_text("Toggle live input monitoring while recording");
        if monitor_button.clicked() {
            monitor_enabled = !monitor_enabled;
            app.controller.set_input_monitoring_enabled(monitor_enabled);
        }
        let is_playing = app.controller.is_playing();
        let play_label = if is_playing {
            RichText::new("▶").size(18.0).color(palette.accent_mint)
        } else {
            RichText::new("▶").size(18.0).color(palette.text_muted)
        };
        let play_button = ui
            .add_enabled(!is_recording, egui::Button::new(play_label))
            .on_hover_text("Play from the current selection or cursor");
        if play_button.clicked() {
            if let Err(err) = app
                .controller
                .play_audio(app.controller.ui.waveform.loop_enabled, None)
            {
                app.controller.set_status(err, style::StatusTone::Error);
            }
        }
        let stop_label = if is_playing {
            RichText::new("Stop").color(style::destructive_text())
        } else {
            RichText::new("Stop").color(palette.text_muted)
        };
        let stop_button = ui
            .add_enabled(is_playing, egui::Button::new(stop_label))
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
        let mut bpm_lock = app.controller.ui.waveform.bpm_lock_enabled;
        let lock_toggle = ui
            .add(egui::Checkbox::new(&mut bpm_lock, "Lock"))
            .on_hover_text("Keep the current BPM value when loading samples with BPM metadata");
        if lock_toggle.clicked() {
            app.controller.set_bpm_lock_enabled(bpm_lock);
        }
        let mut bpm_stretch = app.controller.ui.waveform.bpm_stretch_enabled;
        let stretch_toggle = ui
            .add(egui::Checkbox::new(&mut bpm_stretch, "Stretch"))
            .on_hover_text("Time-stretch loaded samples to the current BPM value");
        if stretch_toggle.clicked() {
            app.controller.set_bpm_stretch_enabled(bpm_stretch);
        }
        if ui.button("⏴").on_hover_text("Decrease BPM by 1").clicked() {
            if let Some(bpm) = app.controller.ui.waveform.bpm_value {
                let next = (bpm - 1.0).max(1.0);
                app.controller.set_bpm_value(next);
                app.controller.ui.waveform.bpm_input = helpers::format_bpm_input(next);
            }
        }
        let bpm_edit = egui::TextEdit::singleline(&mut app.controller.ui.waveform.bpm_input)
            .desired_width(64.0)
            .hint_text("120")
            .show(ui);
        if ui.button("⏵").on_hover_text("Increase BPM by 1").clicked() {
            if let Some(bpm) = app.controller.ui.waveform.bpm_value {
                let next = bpm + 1.0;
                app.controller.set_bpm_value(next);
                app.controller.ui.waveform.bpm_input = helpers::format_bpm_input(next);
            }
        }
        app.controller.ui.hotkeys.suppress_for_bpm_input = bpm_edit.response.has_focus();
        if bpm_edit.response.gained_focus() {
            let mut state = bpm_edit.state;
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::select_all(
                    &bpm_edit.galley,
                )));
            state.store(ui.ctx(), bpm_edit.response.id);
        }
        let parsed = if bpm_edit.response.changed() {
            let parsed = helpers::parse_bpm_input(&app.controller.ui.waveform.bpm_input);
            app.controller.ui.waveform.bpm_value = parsed;
            parsed
        } else {
            None
        };
        if bpm_edit.response.lost_focus() {
            let submitted = parsed.or_else(|| {
                helpers::parse_bpm_input(&app.controller.ui.waveform.bpm_input)
            });
            if let Some(value) = submitted {
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
            app.controller
                .set_transient_markers_enabled(show_transients);
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
        ui.add_space(10.0);
        let slices_ready = !app.controller.ui.waveform.slices.is_empty();
        let has_audio = app.controller.ui.loaded_wav.is_some();
        let slice_mode_enabled = app.controller.ui.waveform.slice_mode_enabled;
        let slice_mode_label = if slice_mode_enabled {
            RichText::new("Slice mode: On").color(palette.accent_mint)
        } else {
            RichText::new("Slice mode: Off").color(palette.text_muted)
        };
        let slice_mode_button = ui
            .add(egui::Button::new(slice_mode_label))
            .on_hover_text("Drag on the waveform to paint slice ranges");
        if slice_mode_button.clicked() {
            app.controller.ui.waveform.slice_mode_enabled = !slice_mode_enabled;
            app.slice_paint = None;
            if app.controller.ui.waveform.slice_mode_enabled {
                app.controller.clear_selection();
            } else {
                app.controller.ui.waveform.selected_slices.clear();
            }
        }
        let detect_button = ui
            .add_enabled(has_audio, egui::Button::new("Detect slices"))
            .on_hover_text("Detect non-silent slices from the loaded sample");
        if detect_button.clicked() {
            match app.controller.detect_waveform_slices_from_silence() {
                Ok(count) => app
                    .controller
                    .set_status(format!("Detected {count} slices"), style::StatusTone::Info),
                Err(err) => app.controller.set_status(err, style::StatusTone::Error),
            }
            app.controller.ui.waveform.slice_mode_enabled = true;
            app.controller.clear_selection();
            app.slice_paint = None;
        }
        let clear_button = ui
            .add_enabled(slices_ready, egui::Button::new("Clear slices"))
            .on_hover_text("Clear detected slice overlays");
        if clear_button.clicked() {
            app.controller.clear_waveform_slices();
        }
        if slices_ready {
            ui.label(format!(
                "Slices: {}",
                app.controller.ui.waveform.slices.len()
            ));
        }
    });
    if view_mode != app.controller.ui.waveform.channel_view {
        app.controller.set_waveform_channel_view(view_mode);
    }
}
