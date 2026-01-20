use super::helpers;
use super::style;
use super::*;

use eframe::egui::{self, RichText, Ui};

pub(super) fn render_waveform_controls(app: &mut EguiApp, ui: &mut Ui, palette: &style::Palette) {
    let mut view_mode = app.controller.ui.waveform.channel_view;
    let icon_off = palette.text_muted.linear_multiply(0.4);
    let tooltip_mode = app.controller.ui.controls.tooltip_mode;

    ui.horizontal(|ui| {
        // --- Group 1: View & Basic Audio ---
        ui.group(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            
            // Channel View Toggle (Mono / Split)
            let channel_view_size = egui::vec2(28.0, 24.0);
            let (view_rect, view_response) = ui.allocate_exact_size(channel_view_size, egui::Sense::click());
            let center = view_rect.center();
            
            match view_mode {
                crate::waveform::WaveformChannelView::Mono => {
                    let mut points = vec![];
                    for i in 0..=6 {
                        let x = center.x - 7.0 + (i as f32 * 2.3);
                        let y = center.y + if i % 2 == 0 { 3.0 } else { -3.0 };
                        points.push(egui::pos2(x, y));
                    }
                    ui.painter().add(egui::Shape::line(
                        points,
                        egui::Stroke::new(1.2, palette.accent_mint),
                    ));
                }
                crate::waveform::WaveformChannelView::SplitStereo => {
                    for (offset_y, color) in [(-4.0, palette.accent_mint), (4.0, palette.accent_mint)] {
                        let mut points = vec![];
                        for i in 0..=6 {
                            let x = center.x - 7.0 + (i as f32 * 2.3);
                            let y = center.y + offset_y + if i % 2 == 0 { 2.0 } else { -2.0 };
                            points.push(egui::pos2(x, y));
                        }
                        ui.painter().add(egui::Shape::line(
                            points,
                            egui::Stroke::new(1.2, color),
                        ));
                    }
                }
            }
            if view_response.hovered() {
                ui.painter().rect_filled(view_rect, 2.0, style::row_hover_fill());
            }

            let view_tip = helpers::tooltip(
                view_response,
                "Channel View",
                "Toggle between a combined Mono downmix and a Split Left/Right stereo view of the loaded audio.",
                tooltip_mode,
            );
            if view_tip.clicked() {
                view_mode = match view_mode {
                    crate::waveform::WaveformChannelView::Mono => crate::waveform::WaveformChannelView::SplitStereo,
                    crate::waveform::WaveformChannelView::SplitStereo => crate::waveform::WaveformChannelView::Mono,
                };
            }

            // Audition Toggle
            let audition_enabled = app.controller.ui.waveform.normalized_audition_enabled;
            let audition_size = egui::vec2(28.0, 24.0);
            let (audition_rect, audition_response) = ui.allocate_exact_size(audition_size, egui::Sense::click());
            let audition_color = if audition_enabled { palette.accent_mint } else { icon_off };
            let center = audition_rect.center();
            let mut wave_points = vec![];
            for i in 0..=4 {
                let x = center.x - 5.0 + (i as f32 * 2.5);
                let y = center.y + if i % 2 == 0 { 2.0 } else { -2.0 };
                wave_points.push(egui::pos2(x, y));
            }
            ui.painter().add(egui::Shape::line(wave_points, egui::Stroke::new(1.2, audition_color)));
            ui.painter().add(egui::Shape::convex_polygon(
                vec![center + egui::vec2(0.0, -7.0), center + egui::vec2(-2.5, -4.5), center + egui::vec2(2.5, -4.5)],
                audition_color, egui::Stroke::NONE
            ));
            ui.painter().add(egui::Shape::convex_polygon(
                vec![center + egui::vec2(0.0, 7.0), center + egui::vec2(-2.5, 4.5), center + egui::vec2(2.5, 4.5)],
                audition_color, egui::Stroke::NONE
            ));

            if audition_response.hovered() {
                ui.painter().rect_filled(audition_rect, 2.0, style::row_hover_fill());
            }

            if audition_response.clicked() {
                app.controller.set_normalized_audition_enabled(!audition_enabled);
            }
            helpers::tooltip(
                audition_response,
                "Normalize Audition",
                "When enabled, playback will be normalized to 0dB in real-time. This helps in auditing quiet samples without changing the source file.",
                tooltip_mode,
            );
        });

        ui.add_space(4.0);

        // --- Group 2: Snapping & Markers ---
        ui.group(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            
            // BPM Snap Icon
            let mut bpm_snap = app.controller.ui.waveform.bpm_snap_enabled;
            let (bpm_snap_rect, bpm_snap_response) = ui.allocate_exact_size(egui::vec2(28.0, 24.0), egui::Sense::click());
            let bpm_snap_color = if bpm_snap { palette.accent_mint } else { icon_off };
            let center = bpm_snap_rect.center();
            // Magnet body (U shape)
            ui.painter().add(egui::Shape::line(
                vec![center + egui::vec2(-4.0, -2.0), center + egui::vec2(-4.0, 3.0), center + egui::vec2(0.0, 5.0), center + egui::vec2(4.0, 3.0), center + egui::vec2(4.0, -2.0)],
                egui::Stroke::new(1.5, bpm_snap_color)
            ));
            // Grid element (small 4 dots)
            for dx in [-1.5, 1.5] {
                for dy in [-5.5, -2.5] {
                    ui.painter().circle_filled(center + egui::vec2(dx, dy), 0.8, bpm_snap_color.linear_multiply(0.8));
                }
            }
            if bpm_snap_response.hovered() {
                ui.painter().rect_filled(bpm_snap_rect, 2.0, style::row_hover_fill());
            }
            if bpm_snap_response.clicked() {
                bpm_snap = !bpm_snap;
                app.controller.set_bpm_snap_enabled(bpm_snap);
            }
            helpers::tooltip(
                bpm_snap_response,
                "BPM Snap",
                "Snap selection boundaries and the cursor to the current BPM grid (1/16th notes).",
                tooltip_mode,
            );

            // X-Snap Icon (Transient Snap)
            let mut transient_snap = app.controller.ui.waveform.transient_snap_enabled;
            let markers_enabled = app.controller.ui.waveform.transient_markers_enabled;
            let (x_snap_rect, x_snap_response) = ui.allocate_exact_size(egui::vec2(28.0, 24.0), if markers_enabled { egui::Sense::click() } else { egui::Sense::hover() });
            let x_snap_color = if !markers_enabled { icon_off.linear_multiply(0.3) }
                              else if transient_snap { palette.accent_mint } else { icon_off };
            let center = x_snap_rect.center();
            // Magnet body (U shape)
            ui.painter().add(egui::Shape::line(
                vec![center + egui::vec2(-4.0, -2.0), center + egui::vec2(-4.0, 3.0), center + egui::vec2(0.0, 5.0), center + egui::vec2(4.0, 3.0), center + egui::vec2(4.0, -2.0)],
                egui::Stroke::new(1.5, x_snap_color)
            ));
            // Marker element (vertical line)
            ui.painter().line_segment([center + egui::vec2(0.0, -7.0), center + egui::vec2(0.0, -2.0)], egui::Stroke::new(1.2, x_snap_color));
            if x_snap_response.hovered() {
                ui.painter().rect_filled(x_snap_rect, 2.0, style::row_hover_fill());
            }
            
            if markers_enabled && x_snap_response.clicked() {
                transient_snap = !transient_snap;
                app.controller.set_transient_snap_enabled(transient_snap);
            }
            helpers::tooltip(
                x_snap_response,
                "Transient Snap",
                "Snap selection boundaries to the nearest detected transient markers. Requires transient lines to be visible.",
                tooltip_mode,
            );

            ui.add_space(4.0);

            // Show Transients Icon
            let mut show_transients = app.controller.ui.waveform.transient_markers_enabled;
            let (transient_rect, transient_response) = ui.allocate_exact_size(egui::vec2(28.0, 24.0), egui::Sense::click());
            let trans_color = if show_transients { palette.accent_mint } else { icon_off };
            let center = transient_rect.center();
            ui.painter().add(egui::Shape::line(vec![center + egui::vec2(0.0, -7.0), center + egui::vec2(0.0, 7.0)], egui::Stroke::new(1.0, trans_color)));
            ui.painter().add(egui::Shape::convex_polygon(
                vec![center + egui::vec2(0.0, -8.0), center + egui::vec2(-2.5, -5.5), center + egui::vec2(2.5, -5.5)],
                trans_color, egui::Stroke::NONE
            ));
            if transient_response.hovered() {
                ui.painter().rect_filled(transient_rect, 2.0, style::row_hover_fill());
            }
            if transient_response.clicked() {
                show_transients = !show_transients;
                app.controller.set_transient_markers_enabled(show_transients);
            }
            helpers::tooltip(
                transient_response,
                "Show Transients",
                "Toggle visibility of detected transient markers. Transients are detected automatically in the background.",
                tooltip_mode,
            );

            // Slice Mode Icon
            let slice_mode_enabled = app.controller.ui.waveform.slice_mode_enabled;
            let (slice_rect, slice_response) = ui.allocate_exact_size(egui::vec2(28.0, 24.0), egui::Sense::click());
            let slice_color = if slice_mode_enabled { palette.accent_mint } else { icon_off };
            let center = slice_rect.center();
            ui.painter().add(egui::Shape::convex_polygon(
                vec![center + egui::vec2(-1.5, 1.5), center + egui::vec2(3.5, -3.5), center + egui::vec2(5.5, -1.5), center + egui::vec2(0.5, 3.5)],
                slice_color, egui::Stroke::NONE
            ));
            ui.painter().add(egui::Shape::convex_polygon(
                vec![center + egui::vec2(-1.5, 1.5), center + egui::vec2(-5.0, 5.0), center + egui::vec2(-3.5, 6.5), center + egui::vec2(0.5, 3.5)],
                slice_color.linear_multiply(0.5), egui::Stroke::NONE
            ));
            if slice_response.hovered() {
                ui.painter().rect_filled(slice_rect, 2.0, style::row_hover_fill());
            }
            if slice_response.clicked() {
                app.controller.ui.waveform.slice_mode_enabled = !slice_mode_enabled;
            }
            helpers::tooltip(
                slice_response,
                "Slice Mode",
                "Enable advanced slicing tools to segment the audio into playable regions.",
                tooltip_mode,
            );
        });

        ui.add_space(4.0);

        // --- Group 3: BPM Management ---
        ui.group(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            
            // BPM Input Widget
            let (bpm_edit_response, adjust) = crate::egui_app::ui::chrome::fields::NumericInput::new(
                &mut app.controller.ui.waveform.bpm_input,
                ui.id().with("bpm_input_field"),
            )
            .width(54.0)
            .hint("120")
            .show(ui);

            if let Some(adjust) = adjust {
                let current_bpm = app.controller.ui.waveform.bpm_value.or_else(|| {
                    helpers::parse_bpm_input(&app.controller.ui.waveform.bpm_input)
                });
                if let Some(bpm) = current_bpm {
                    let next = (bpm + adjust).max(1.0);
                    app.controller.set_bpm_value(next);
                    app.controller.ui.waveform.bpm_input = helpers::format_bpm_input(next);
                }
            }

            // Toggles
            let mut bpm_lock = app.controller.ui.waveform.bpm_lock_enabled;
            let (lock_rect, lock_response) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
            let lock_color = if bpm_lock { palette.accent_mint } else { icon_off };
            let center = lock_rect.center();
            ui.painter().rect_filled(egui::Rect::from_center_size(center + egui::vec2(0.0, 1.5), egui::vec2(9.0, 7.0)), 1.0, lock_color);
            ui.painter().add(egui::Shape::line(
                vec![center + egui::vec2(-2.5, -1.5), center + egui::vec2(-2.5, -4.5), center + egui::vec2(0.0, -6.5), center + egui::vec2(2.5, -4.5), center + egui::vec2(2.5, -1.5)],
                egui::Stroke::new(1.3, lock_color)
            ));
            if lock_response.hovered() {
                ui.painter().rect_filled(lock_rect, 2.0, style::row_hover_fill());
            }
            if lock_response.clicked() {
                bpm_lock = !bpm_lock;
                app.controller.set_bpm_lock_enabled(bpm_lock);
            }
            helpers::tooltip(
                lock_response,
                "BPM Lock",
                "Lock the BPM input field. When locked, loading a new sample will not automatically update the BPM value.",
                tooltip_mode,
            );

            let mut bpm_stretch = app.controller.ui.waveform.bpm_stretch_enabled;
            let (stretch_rect, stretch_response) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
            let stretch_color = if bpm_stretch { palette.accent_mint } else { icon_off };
            let center = stretch_rect.center();
            ui.painter().line_segment([center - egui::vec2(6.0, 0.0), center + egui::vec2(6.0, 0.0)], egui::Stroke::new(1.3, stretch_color));
            ui.painter().add(egui::Shape::convex_polygon(
                vec![center - egui::vec2(6.0, 0.0), center - egui::vec2(3.0, 2.5), center - egui::vec2(3.0, -2.5)],
                stretch_color, egui::Stroke::NONE
            ));
            ui.painter().add(egui::Shape::convex_polygon(
                vec![center + egui::vec2(6.0, 0.0), center + egui::vec2(3.0, 2.5), center + egui::vec2(3.0, -2.5)],
                stretch_color, egui::Stroke::NONE
            ));
            if stretch_response.hovered() {
                ui.painter().rect_filled(stretch_rect, 2.0, style::row_hover_fill());
            }
            if stretch_response.clicked() {
                bpm_stretch = !bpm_stretch;
                app.controller.set_bpm_stretch_enabled(bpm_stretch);
            }
            helpers::tooltip(
                stretch_response,
                "BPM Stretch",
                "Enable time-stretching to match the audio to the current BPM. Adjusting BPM will change playback speed without affecting pitch.",
                tooltip_mode,
            );
            
            app.controller.ui.hotkeys.suppress_for_bpm_input = bpm_edit_response.has_focus();
            if bpm_edit_response.lost_focus() || bpm_edit_response.changed() {
                if let Some(value) = helpers::parse_bpm_input(&app.controller.ui.waveform.bpm_input) {
                    app.controller.ui.waveform.bpm_value = Some(value);
                    if bpm_edit_response.lost_focus() {
                        app.controller.set_bpm_value(value);
                    }
                }
            }
        });

        // --- Group 4: Transport (Right Aligned) ---
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.group(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let is_recording = app.controller.is_recording();
                let has_source = app.controller.current_source().is_some();
                let is_playing = app.controller.is_playing();

                // Play
                let (play_rect, play_response) = ui.allocate_exact_size(egui::vec2(32.0, 24.0), if !is_recording { egui::Sense::click() } else { egui::Sense::hover() });
                let play_color = if is_recording { ui.visuals().widgets.noninteractive.fg_stroke.color.linear_multiply(0.3) }
                                else if is_playing { palette.accent_copper } else { icon_off };
                let center = play_rect.center();
                ui.painter().add(egui::Shape::convex_polygon(
                    vec![center + egui::vec2(5.0, 0.0), center + egui::vec2(-4.0, 6.0), center + egui::vec2(-4.0, -6.0)],
                    play_color, egui::Stroke::NONE
                ));
                if play_response.hovered() {
                    ui.painter().rect_filled(play_rect.shrink(2.0), 2.0, style::row_hover_fill());
                }
                if play_response.clicked() {
                    let _ = app.controller.play_audio(app.controller.ui.waveform.loop_enabled, None);
                }
                helpers::tooltip(
                    play_response,
                    "Play",
                    "Start audio playback from the current cursor or selection. Use Space to toggle.",
                    tooltip_mode,
                );

                // Stop
                let (stop_rect, stop_response) = ui.allocate_exact_size(egui::vec2(32.0, 24.0), if is_playing { egui::Sense::click() } else { egui::Sense::hover() });
                let stop_color = if is_playing { style::destructive_text() } else { ui.visuals().widgets.noninteractive.fg_stroke.color.linear_multiply(0.3) };
                ui.painter().rect_filled(egui::Rect::from_center_size(stop_rect.center(), egui::vec2(10.0, 10.0)), 1.0, stop_color);
                if stop_response.hovered() {
                    ui.painter().rect_filled(stop_rect.shrink(2.0), 2.0, style::row_hover_fill());
                }
                if stop_response.clicked() {
                    app.controller.stop_playback_if_active();
                }
                helpers::tooltip(
                    stop_response,
                    "Stop",
                    "Stop playback and return the cursor to the start of the selection or file.",
                    tooltip_mode,
                );

                // Loop Toggle (Relocated to Transport)
                let loop_enabled = app.controller.ui.waveform.loop_enabled;
                let loop_locked = app.controller.ui.waveform.loop_lock_enabled;
                let (loop_rect, loop_response) = ui.allocate_exact_size(egui::vec2(32.0, 24.0), egui::Sense::click());
                let loop_color = match (loop_locked, loop_enabled) {
                    (true, true) => style::destructive_text(),
                    (true, false) => style::warning_soft_text(),
                    (false, true) => palette.accent_mint,
                    (false, false) => icon_off,
                };
                let center = loop_rect.center();
                let radius = 5.0;
                let mut points = vec![];
                for i in 0..=12 {
                    let angle = (i as f32 / 12.0) * std::f32::consts::TAU * 0.8 - std::f32::consts::FRAC_PI_2;
                    points.push(center + egui::vec2(angle.cos() * radius, angle.sin() * radius));
                }
                ui.painter().add(egui::Shape::line(points.clone(), egui::Stroke::new(1.5, loop_color)));
                let tip = points.last().unwrap();
                let angle = (0.8 * std::f32::consts::TAU) - std::f32::consts::FRAC_PI_2;
                ui.painter().add(egui::Shape::convex_polygon(
                    vec![*tip, *tip + egui::vec2((angle + 0.5).cos() * 3.5, (angle + 0.5).sin() * 3.5), *tip + egui::vec2((angle - 0.5).cos() * 3.5, (angle - 0.5).sin() * 3.5)],
                    loop_color, egui::Stroke::NONE
                ));
                if loop_response.hovered() {
                    ui.painter().rect_filled(loop_rect.shrink(2.0), 2.0, style::row_hover_fill());
                }
                if loop_response.clicked() {
                    let modifiers = ui.input(|i| i.modifiers);
                    if modifiers.shift {
                        app.controller
                            .set_loop_lock_enabled(!app.controller.ui.waveform.loop_lock_enabled);
                    } else {
                        app.controller.toggle_loop();
                    }
                }
                helpers::tooltip(
                    loop_response,
                    "Toggle Loop",
                    "Continuously loop the current selection. Use 'L' to toggle.\nShift+Click or Shift+L locks the loop state.",
                    tooltip_mode,
                );

                // Record
                let (record_rect, record_response) = ui.allocate_exact_size(egui::vec2(32.0, 24.0), if is_recording || has_source { egui::Sense::click() } else { egui::Sense::hover() });
                let record_color = if is_recording { style::destructive_text() } else if has_source { icon_off } else { ui.visuals().widgets.noninteractive.fg_stroke.color.linear_multiply(0.3) };
                ui.painter().circle_filled(record_rect.center(), 6.0, record_color);
                if record_response.hovered() {
                    ui.painter().rect_filled(record_rect.shrink(2.0), 2.0, style::row_hover_fill());
                }
                if record_response.clicked() {
                    let _ = if is_recording { app.controller.stop_recording_and_load() } else { app.controller.start_recording() };
                }
                helpers::tooltip(
                    record_response,
                    "Record",
                    "Start or stop audio recording. Captured audio will be automatically loaded into the waveform for editing.",
                    tooltip_mode,
                );

                // Monitor
                let mut monitor = app.controller.ui.controls.input_monitoring_enabled;
                let (mon_rect, mon_response) = ui.allocate_exact_size(egui::vec2(32.0, 24.0), egui::Sense::click());
                let mon_color = if monitor { style::destructive_text() } else { icon_off };
                let center = mon_rect.center();
                ui.painter().rect_filled(egui::Rect::from_min_max(center + egui::vec2(-6.0, -3.0), center + egui::vec2(-2.0, 3.0)), 0.5, mon_color);
                ui.painter().add(egui::Shape::convex_polygon(
                    vec![center + egui::vec2(-2.0, -3.0), center + egui::vec2(3.0, -7.0), center + egui::vec2(3.0, 7.0), center + egui::vec2(-2.0, 3.0)],
                    mon_color, egui::Stroke::NONE
                ));
                if mon_response.hovered() {
                    ui.painter().rect_filled(mon_rect.shrink(2.0), 2.0, style::row_hover_fill());
                }
                if mon_response.clicked() {
                    monitor = !monitor;
                    app.controller.set_input_monitoring_enabled(monitor);
                }
                helpers::tooltip(
                    mon_response,
                    "Input Monitoring",
                    "Hear the incoming audio signal through your speakers. Useful for checking levels before and during recording.",
                    tooltip_mode,
                );
            });
        });
    });

    // --- Row 2: Contextual Slice/Transient Options ---
    if app.controller.ui.waveform.slice_mode_enabled {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.group(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let has_audio = app.controller.ui.loaded_wav.is_some();
                
                // Detect Slices
                let (det_rect, det_response) = ui.allocate_exact_size(egui::vec2(28.0, 24.0), egui::Sense::click());
                let det_color = if has_audio { icon_off } else { icon_off.linear_multiply(0.3) };
                let center = det_rect.center();
                let mut wave = vec![];
                for i in 0..=3 {
                    wave.push(egui::pos2(center.x - 5.0 + (i as f32 * 3.3), center.y + if i % 2 == 0 { 2.0 } else { -2.0 }));
                }
                ui.painter().add(egui::Shape::line(wave, egui::Stroke::new(1.2, det_color)));
                ui.painter().circle_filled(center + egui::vec2(3.0, -5.0), 1.0, palette.accent_mint);
                if det_response.hovered() {
                    ui.painter().rect_filled(det_rect, 2.0, style::row_hover_fill());
                }
                if has_audio && det_response.clicked() {
                    let _ = app.controller.detect_waveform_slices_from_silence();
                }
                helpers::tooltip(
                    det_response,
                    "Detect Slices",
                    "Automatically identify slices by analyzing silence and transients in the audio.",
                    tooltip_mode,
                );

                // Clear Slices
                if !app.controller.ui.waveform.slices.is_empty() {
                    let (clr_rect, clr_response) = ui.allocate_exact_size(egui::vec2(28.0, 24.0), egui::Sense::click());
                    let center = clr_rect.center();
                    ui.painter().rect_filled(egui::Rect::from_center_size(center, egui::vec2(10.0, 10.0)), 1.0, icon_off.linear_multiply(0.3));
                    ui.painter().add(egui::Shape::line(vec![center-egui::vec2(3.0,3.0), center+egui::vec2(3.0,3.0)], egui::Stroke::new(1.2, style::destructive_text())));
                    ui.painter().add(egui::Shape::line(vec![center+egui::vec2(3.0,-3.0), center-egui::vec2(3.0,3.0)], egui::Stroke::new(1.2, style::destructive_text())));
                    if clr_response.hovered() {
                        ui.painter().rect_filled(clr_rect, 2.0, style::row_hover_fill());
                    }
                    if clr_response.clicked() { app.controller.clear_waveform_slices(); }
                    helpers::tooltip(
                        clr_response,
                        "Clear Slices",
                        "Delete all currently defined slices. This action is destructive but affects only the in-memory analysis.",
                        tooltip_mode,
                    );
                    
                    ui.add_space(4.0);
                    ui.label(RichText::new(format!("Slices: {}", app.controller.ui.waveform.slices.len())).size(11.0).color(icon_off));
                }
            });
        });
    }

    if view_mode != app.controller.ui.waveform.channel_view {
        app.controller.set_waveform_channel_view(view_mode);
    }
}
