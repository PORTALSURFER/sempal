use super::*;
use crate::egui_app::state::{DragSource, WaveformView};
use crate::selection::{SelectionEdge, SelectionRange};
use eframe::egui::{self, CursorIcon};

pub(super) fn handle_selection_handle_drag(
    app: &mut EguiApp,
    ui: &mut egui::Ui,
    selection: SelectionRange,
    handle_response: &egui::Response,
) {
    let primary_down = ui.input(|i| i.pointer.button_down(egui::PointerButton::Primary));
    if handle_response.drag_started() && primary_down {
        if let Some(pos) = handle_response.interact_pointer_pos() {
            app.controller
                .start_selection_drag_payload(selection, pos, true);
            app.controller.ui.drag.origin_source = Some(DragSource::Waveform);
        }
    } else if handle_response.dragged_by(egui::PointerButton::Primary) {
        if let Some(pos) = handle_response.interact_pointer_pos() {
            let alt_down = ui.input(|i| i.modifiers.alt);
            app.controller.refresh_drag_position(pos, false, alt_down);
        }
    } else if handle_response.drag_stopped() && !primary_down {
        app.controller.finish_active_drag();
    }

    if handle_response.dragged_by(egui::PointerButton::Primary) {
        ui.output_mut(|o| o.cursor_icon = CursorIcon::Grabbing);
    } else if handle_response.hovered() {
        ui.output_mut(|o| o.cursor_icon = CursorIcon::Grab);
    }
}

pub(super) fn handle_selection_slide_drag(
    app: &mut EguiApp,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    view: WaveformView,
    view_width: f64,
    selection: SelectionRange,
    response: &egui::Response,
) {
    let primary_down = ui.input(|i| i.pointer.button_down(egui::PointerButton::Primary));
    let to_wave_pos = |pos: egui::Pos2| {
        let normalized = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64;
        normalized.mul_add(view_width, view.start).clamp(0.0, 1.0) as f32
    };
    if response.drag_started() && primary_down {
        if let Some(pos) = response.interact_pointer_pos() {
            let anchor = to_wave_pos(pos);
            app.selection_slide = Some(super::SelectionSlide {
                anchor,
                range: selection,
            });
            app.controller.begin_selection_undo("Selection");
            app.controller.cancel_active_drag();
        }
    } else if response.dragged_by(egui::PointerButton::Primary) {
        if let Some(pos) = response.interact_pointer_pos() {
            if app.selection_slide.is_none() {
                let anchor = to_wave_pos(pos);
                app.selection_slide = Some(super::SelectionSlide {
                    anchor,
                    range: selection,
                });
                app.controller.begin_selection_undo("Selection");
                app.controller.cancel_active_drag();
            }
            if let Some(slide) = app.selection_slide {
                let cursor = to_wave_pos(pos);
                let delta = cursor - slide.anchor;
                let snap_step = if app.controller.ui.waveform.bpm_snap_enabled
                    && !ui.input(|i| i.modifiers.shift)
                {
                    bpm_snap_step(app)
                } else {
                    None
                };
                let mut adjusted_delta = snap_step
                    .filter(|step| step.is_finite() && *step > 0.0)
                    .map(|step| snap_delta(delta, step))
                    .unwrap_or(delta);
                if snap_step.is_none() {
                    if let Some(snapped_start) =
                        snap_selection_start_to_transient(app, slide.range.start() + adjusted_delta)
                    {
                        adjusted_delta = snapped_start - slide.range.start();
                    }
                }
                app.controller
                    .set_selection_range(slide.range.shift(adjusted_delta));
            }
        }
    } else if response.drag_stopped() && !primary_down {
        if app.selection_slide.take().is_some() {
            app.controller.finish_selection_drag();
        }
    }

    if response.dragged_by(egui::PointerButton::Primary) {
        ui.output_mut(|o| o.cursor_icon = CursorIcon::Grabbing);
    } else if response.hovered() {
        ui.output_mut(|o| o.cursor_icon = CursorIcon::Grab);
    }
}

fn bpm_snap_step(app: &EguiApp) -> Option<f32> {
    let bpm = app.controller.ui.waveform.bpm_value?;
    if !bpm.is_finite() || bpm <= 0.0 {
        return None;
    }
    let duration = app.controller.loaded_audio_duration_seconds()?;
    if !duration.is_finite() || duration <= 0.0 {
        return None;
    }
    let step = 60.0 / bpm / duration;
    if step.is_finite() && step > 0.0 {
        Some(step)
    } else {
        None
    }
}

fn snap_delta(delta: f32, step: f32) -> f32 {
    if !delta.is_finite() || !step.is_finite() || step <= 0.0 {
        return delta;
    }
    (delta / step).round() * step
}

fn snap_selection_start_to_transient(app: &EguiApp, start: f32) -> Option<f32> {
    const TRANSIENT_SNAP_RADIUS: f32 = 0.01;
    if !app.controller.ui.waveform.transient_markers_enabled
        || !app.controller.ui.waveform.transient_snap_enabled
    {
        return None;
    }
    let mut closest = None;
    let mut best_distance = TRANSIENT_SNAP_RADIUS;
    for &marker in &app.controller.ui.waveform.transients {
        let distance = (marker - start).abs();
        if distance <= best_distance {
            best_distance = distance;
            closest = Some(marker);
        }
    }
    closest
}

pub(super) fn handle_selection_edge_drag(
    app: &mut EguiApp,
    rect: egui::Rect,
    view: WaveformView,
    view_width: f64,
    edge: SelectionEdge,
    alt_down: bool,
    shift_down: bool,
    primary_down: bool,
    edge_response: &egui::Response,
    selection_edge_x: f32,
) {
    let pointer_down = primary_down && edge_response.is_pointer_button_down_on();
    if (edge_response.drag_started() && primary_down)
        || (pointer_down && !app.controller.is_selection_dragging())
    {
        app.controller.start_selection_edge_drag(edge, alt_down);
        app.selection_edge_alt_scale = alt_down;
        if app.selection_edge_offset.is_none() {
            if let Some(pos) = edge_response.interact_pointer_pos() {
                app.selection_edge_offset = Some(pos.x - selection_edge_x);
            } else {
                app.selection_edge_offset = Some(0.0);
            }
        }
    }
    if (pointer_down || edge_response.dragged_by(egui::PointerButton::Primary))
        && let Some(pos) = edge_response.interact_pointer_pos()
    {
        let offset = app.selection_edge_offset.unwrap_or(0.0);
        let view_fraction = ((pos.x - offset - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64;
        let absolute = view.start + view_width.max(1e-9) * view_fraction;
        let clamped = absolute.clamp(0.0, 1.0) as f32;
        app.controller.update_selection_drag(clamped, shift_down);
    }
    if edge_response.drag_stopped() && !primary_down {
        app.selection_edge_offset = None;
        app.selection_edge_alt_scale = false;
        app.controller.finish_selection_drag();
    }
}

pub(super) fn handle_edit_fade_handle_drag(
    app: &mut EguiApp,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    view: WaveformView,
    view_width: f64,
    selection: SelectionRange,
    selection_rect: egui::Rect,
    fade_in_response: &egui::Response,
    fade_out_response: &egui::Response,
    fade_in_lower_response: &egui::Response,
    fade_out_lower_response: &egui::Response,
    fade_in_region_response: &egui::Response,
    fade_out_region_response: &egui::Response,
) {
    let primary_down = ui.input(|i| i.pointer.button_down(egui::PointerButton::Primary));
    let alt_down = ui.input(|i| i.modifiers.alt);
    let cursor_to_wave = |pos: egui::Pos2| {
        let normalized = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64;
        (view.start + view_width.max(1e-9) * normalized).clamp(0.0, 1.0) as f32
    };
    
    // Handle fade-in edge drag (move selection start while keeping fade end anchored)
    if (fade_in_lower_response.drag_started() && primary_down)
        || (primary_down && fade_in_lower_response.is_pointer_button_down_on())
    {
        app.controller.begin_selection_undo("Fade In");
    }

    let fade_in_lower_active = fade_in_lower_response.dragged_by(egui::PointerButton::Primary)
        || (primary_down && fade_in_lower_response.is_pointer_button_down_on());

    if fade_in_lower_active {
        if let Some(pos) = fade_in_lower_response.interact_pointer_pos() {
            let anchor = selection.start() + selection.width() * selection.fade_in_length();
            let new_start = cursor_to_wave(pos).min(selection.end());
            let new_width = (selection.end() - new_start).max(0.0);
            let current_curve = selection.fade_in().map(|f| f.curve).unwrap_or(0.5);
            let mut new_selection = SelectionRange::new(new_start, selection.end());
            if let Some(fade_out) = selection.fade_out() {
                new_selection = new_selection.with_fade_out(fade_out.length, fade_out.curve);
            }
            let new_length = if new_width > 0.0 {
                ((anchor - new_start) / new_width).clamp(0.0, 1.0)
            } else {
                0.0
            };
            new_selection = new_selection.with_fade_in(new_length, current_curve);
            app.controller.set_edit_selection_range(new_selection);
        }
    }

    if fade_in_lower_response.drag_stopped() && !primary_down {
        app.controller.finish_selection_drag();
    }

    let fade_in_region_active = alt_down
        && (fade_in_region_response.dragged_by(egui::PointerButton::Primary)
            || (primary_down && fade_in_region_response.is_pointer_button_down_on()));

    if !fade_in_lower_active && fade_in_region_active {
        if (fade_in_region_response.drag_started() && primary_down)
            || (primary_down && fade_in_region_response.is_pointer_button_down_on())
        {
            app.controller.begin_selection_undo("Fade In");
        }

        let current_curve = selection.fade_in().map(|f| f.curve).unwrap_or(0.5);
        let current_length = selection.fade_in_length();
        if current_length > 0.0 {
            let delta_y = fade_in_region_response.drag_delta().y;
            let curve_delta = -delta_y / 100.0;
            let new_curve = (current_curve + curve_delta).clamp(0.0, 1.0);
            let new_selection = selection.with_fade_in(current_length, new_curve);
            app.controller.set_edit_selection_range(new_selection);
        }

        if fade_in_region_response.drag_stopped() && !primary_down {
            app.controller.finish_selection_drag();
        }
    }

    if !fade_in_lower_active && !fade_in_region_active {
        // Handle fade-in drag
        if (fade_in_response.drag_started() && primary_down)
            || (primary_down && fade_in_response.is_pointer_button_down_on())
        {
            app.controller.begin_selection_undo("Fade In");
        }

        // Use either the handle or the region for dragging
        let fade_in_active = fade_in_response.dragged_by(egui::PointerButton::Primary)
            || (primary_down && fade_in_response.is_pointer_button_down_on());

        if fade_in_active {
            let pos = fade_in_response.interact_pointer_pos();

            if let Some(pos) = pos {
                // Normal drag: adjust fade length (horizontal movement)
                let delta_x = pos.x - selection_rect.left();
                let fade_fraction = (delta_x / selection_rect.width()).clamp(0.0, 1.0);
                let current_curve = selection.fade_in().map(|f| f.curve).unwrap_or(0.5);
                let new_selection = selection.with_fade_in(fade_fraction, current_curve);
                app.controller.set_edit_selection_range(new_selection);
            }
        }

        if fade_in_response.drag_stopped() && !primary_down {
            app.controller.finish_selection_drag();
        }
    }

    // Handle fade-out edge drag (move selection end while keeping fade start anchored)
    if (fade_out_lower_response.drag_started() && primary_down)
        || (primary_down && fade_out_lower_response.is_pointer_button_down_on())
    {
        app.controller.begin_selection_undo("Fade Out");
    }

    let fade_out_lower_active = fade_out_lower_response.dragged_by(egui::PointerButton::Primary)
        || (primary_down && fade_out_lower_response.is_pointer_button_down_on());

    if fade_out_lower_active {
        if let Some(pos) = fade_out_lower_response.interact_pointer_pos() {
            let anchor = selection.end() - selection.width() * selection.fade_out_length();
            let new_end = cursor_to_wave(pos).max(selection.start());
            let new_width = (new_end - selection.start()).max(0.0);
            let current_curve = selection.fade_out().map(|f| f.curve).unwrap_or(0.5);
            let mut new_selection = SelectionRange::new(selection.start(), new_end);
            if let Some(fade_in) = selection.fade_in() {
                new_selection = new_selection.with_fade_in(fade_in.length, fade_in.curve);
            }
            let new_length = if new_width > 0.0 {
                ((new_end - anchor) / new_width).clamp(0.0, 1.0)
            } else {
                0.0
            };
            new_selection = new_selection.with_fade_out(new_length, current_curve);
            app.controller.set_edit_selection_range(new_selection);
        }
    }

    if fade_out_lower_response.drag_stopped() && !primary_down {
        app.controller.finish_selection_drag();
    }

    let fade_out_region_active = alt_down
        && (fade_out_region_response.dragged_by(egui::PointerButton::Primary)
            || (primary_down && fade_out_region_response.is_pointer_button_down_on()));

    if !fade_out_lower_active && fade_out_region_active {
        if (fade_out_region_response.drag_started() && primary_down)
            || (primary_down && fade_out_region_response.is_pointer_button_down_on())
        {
            app.controller.begin_selection_undo("Fade Out");
        }

        let current_curve = selection.fade_out().map(|f| f.curve).unwrap_or(0.5);
        let current_length = selection.fade_out_length();
        if current_length > 0.0 {
            let delta_y = fade_out_region_response.drag_delta().y;
            let curve_delta = -delta_y / 100.0;
            let new_curve = (current_curve + curve_delta).clamp(0.0, 1.0);
            let new_selection = selection.with_fade_out(current_length, new_curve);
            app.controller.set_edit_selection_range(new_selection);
        }

        if fade_out_region_response.drag_stopped() && !primary_down {
            app.controller.finish_selection_drag();
        }
    }

    if !fade_out_lower_active && !fade_out_region_active {
        // Handle fade-out drag
        if (fade_out_response.drag_started() && primary_down)
            || (primary_down && fade_out_response.is_pointer_button_down_on())
        {
            app.controller.begin_selection_undo("Fade Out");
        }

        // Use either the handle or the region for dragging
        let fade_out_active = fade_out_response.dragged_by(egui::PointerButton::Primary)
            || (primary_down && fade_out_response.is_pointer_button_down_on());

        if fade_out_active {
            let pos = fade_out_response.interact_pointer_pos();

            if let Some(pos) = pos {
                // Normal drag: adjust fade length (horizontal movement)
                let delta_x = selection_rect.right() - pos.x;
                let fade_fraction = (delta_x / selection_rect.width()).clamp(0.0, 1.0);
                let current_curve = selection.fade_out().map(|f| f.curve).unwrap_or(0.5);
                let new_selection = selection.with_fade_out(fade_fraction, current_curve);
                app.controller.set_edit_selection_range(new_selection);
            }
        }

        if fade_out_response.drag_stopped() && !primary_down {
            app.controller.finish_selection_drag();
        }
    }
}



pub(super) fn sync_selection_edge_drag_release(app: &mut EguiApp, ctx: &egui::Context) {
    if !ctx.input(|i| i.pointer.primary_down()) {
        if app.controller.is_selection_dragging() {
            app.controller.finish_selection_drag();
        }
        app.selection_edge_offset = None;
        app.selection_edge_alt_scale = false;
    }
}
