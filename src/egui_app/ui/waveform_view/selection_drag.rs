use super::*;
use crate::egui_app::state::{DragSource, WaveformView};
use crate::selection::{SelectionEdge, SelectionRange};
use eframe::egui::{self, CursorIcon};

pub(super) fn handle_selection_handle_drag(
    app: &mut EguiApp,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    view: WaveformView,
    view_width: f32,
    selection: SelectionRange,
    handle_response: &egui::Response,
) {
    let to_wave_pos = |pos: egui::Pos2| {
        let normalized = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
        normalized.mul_add(view_width, view.start).clamp(0.0, 1.0)
    };
    if handle_response.drag_started() {
        if let Some(pos) = handle_response.interact_pointer_pos() {
            let alt = ui.input(|i| i.modifiers.alt);
            if alt {
                let anchor = to_wave_pos(pos);
                app.selection_slide = Some(super::SelectionSlide {
                    anchor,
                    range: selection,
                });
            } else {
                app.controller
                    .start_selection_drag_payload(selection, pos, true);
                app.controller.ui.drag.origin_source = Some(DragSource::Waveform);
            }
        }
    } else if handle_response.dragged() {
        if let Some(pos) = handle_response.interact_pointer_pos() {
            let alt = ui.input(|i| i.modifiers.alt);
            if alt && app.selection_slide.is_none() {
                let anchor = to_wave_pos(pos);
                app.selection_slide = Some(super::SelectionSlide {
                    anchor,
                    range: selection,
                });
                app.controller.cancel_active_drag();
            }
            if let Some(slide) = app.selection_slide {
                let cursor = to_wave_pos(pos);
                let delta = cursor - slide.anchor;
                app.controller.set_selection_range(slide.range.shift(delta));
            } else {
                app.controller.refresh_drag_position(pos, false);
            }
        }
    } else if handle_response.drag_stopped() {
        if app.selection_slide.take().is_some() {
            app.controller.finish_selection_drag();
        } else {
            app.controller.finish_active_drag();
        }
    }

    if handle_response.dragged() {
        ui.output_mut(|o| o.cursor_icon = CursorIcon::Grabbing);
    } else if handle_response.hovered() {
        ui.output_mut(|o| o.cursor_icon = CursorIcon::Grab);
    }
}

pub(super) fn handle_selection_edge_drag(
    app: &mut EguiApp,
    rect: egui::Rect,
    view: WaveformView,
    view_width: f32,
    edge: SelectionEdge,
    edge_response: &egui::Response,
    selection_edge_x: f32,
) {
    let pointer_down = edge_response.is_pointer_button_down_on();
    if edge_response.drag_started() || pointer_down {
        app.controller.start_selection_edge_drag(edge);
        if app.selection_edge_offset.is_none() {
            if let Some(pos) = edge_response.interact_pointer_pos() {
                app.selection_edge_offset = Some(pos.x - selection_edge_x);
            } else {
                app.selection_edge_offset = Some(0.0);
            }
        }
    }
    if (pointer_down || edge_response.dragged())
        && let Some(pos) = edge_response.interact_pointer_pos()
    {
        let offset = app.selection_edge_offset.unwrap_or(0.0);
        let view_fraction = ((pos.x - offset - rect.left()) / rect.width()).clamp(0.0, 1.0);
        let absolute = view.start + view_width.max(f32::EPSILON) * view_fraction;
        let clamped = absolute.clamp(0.0, 1.0);
        app.controller.update_selection_drag(clamped);
    }
    if edge_response.drag_stopped() {
        app.selection_edge_offset = None;
        app.controller.finish_selection_drag();
    }
}

pub(super) fn sync_selection_edge_drag_release(app: &mut EguiApp, ctx: &egui::Context) {
    if !ctx.input(|i| i.pointer.primary_down()) {
        if app.controller.is_selection_dragging() {
            app.controller.finish_selection_drag();
        }
        app.selection_edge_offset = None;
    }
}
