use super::super::*;
use crate::egui_app::state::WaveformView;
use eframe::egui::{self, Ui};

pub(in super::super) fn handle_waveform_pointer_interactions(
    app: &mut EguiApp,
    ui: &mut Ui,
    rect: egui::Rect,
    response: &egui::Response,
    view: WaveformView,
    view_width: f32,
) {
    let pointer_pos = response.interact_pointer_pos();
    let normalize_to_waveform = |pos: egui::Pos2| {
        ((pos.x - rect.left()) / rect.width())
            .mul_add(view_width, view.start)
            .clamp(0.0, 1.0)
    };
    let drag_start_normalized = if response.drag_started() {
        if app.controller.ui.waveform.image.is_some() {
            app.controller.focus_waveform_context();
        }
        let press_origin = ui.ctx().input(|i| i.pointer.press_origin());
        press_origin
            .map(|pos| {
                ui.ctx()
                    .layer_transform_from_global(response.layer_id)
                    .map(|transform| transform * pos)
                    .unwrap_or(pos)
            })
            .map(normalize_to_waveform)
            .or_else(|| pointer_pos.map(normalize_to_waveform))
    } else {
        None
    };
    let normalized = pointer_pos.map(normalize_to_waveform);
    let middle_down = ui.input(|i| i.pointer.button_down(egui::PointerButton::Middle));
    if middle_down {
        let Some(pos) = pointer_pos.or_else(|| response.interact_pointer_pos()) else {
            app.controller.ui.waveform.pan_drag_pos = None;
            return;
        };
        let last = app.controller.ui.waveform.pan_drag_pos.unwrap_or(pos);
        let delta = pos - last;
        app.controller.ui.waveform.pan_drag_pos = Some(pos);
        if response.dragged_by(egui::PointerButton::Middle) && view_width < 1.0 {
            let fraction_delta = (delta.x / rect.width()) * view_width;
            let view_center = view.start + view_width * 0.5;
            let target_center = (view_center - fraction_delta).clamp(0.0, 1.0);
            app.controller.scroll_waveform_view(target_center);
        }
        return;
    }
    app.controller.ui.waveform.pan_drag_pos = None;
    if response.drag_started() {
        if let Some(value) = drag_start_normalized {
            app.controller.start_selection_drag(value);
        }
    } else if response.dragged() {
        if let Some(value) = normalized {
            if app.controller.ui.waveform.image.is_some() {
                app.controller.focus_waveform_context();
            }
            app.controller.update_selection_drag(value);
        }
    } else if response.drag_stopped() {
        app.controller.finish_selection_drag();
    } else if response.clicked() {
        if app.controller.ui.waveform.image.is_some() {
            app.controller.focus_waveform_context();
        }
        if app.controller.ui.waveform.selection.is_some() {
            app.controller.clear_selection();
        }
        if let Some(value) = normalized {
            app.controller.seek_to(value);
        }
    }
}
