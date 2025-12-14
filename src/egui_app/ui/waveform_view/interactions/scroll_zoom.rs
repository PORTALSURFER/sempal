use super::super::*;
use crate::egui_app::state::WaveformView;
use eframe::egui::{self, Ui};

pub(in super::super) fn handle_waveform_interactions(
    app: &mut EguiApp,
    ui: &mut Ui,
    rect: egui::Rect,
    response: &egui::Response,
    view: WaveformView,
    view_width: f32,
) {
    if !response.hovered() {
        return;
    }
    let scroll_delta = ui.input(|i| i.raw_scroll_delta);
    if scroll_delta == egui::Vec2::ZERO {
        return;
    }
    let shift_down = ui.input(|i| i.modifiers.shift);
    if shift_down && view_width < 1.0 {
        let pan_delta = scroll_delta * app.controller.ui.controls.waveform_scroll_speed;
        let invert = if app.controller.ui.controls.invert_waveform_scroll {
            -1.0
        } else {
            1.0
        };
        let delta_x = if pan_delta.x.abs() > 0.0 {
            pan_delta.x
        } else {
            pan_delta.y
        } * invert;
        if delta_x.abs() > 0.0 {
            let view_center = view.start + view_width * 0.5;
            let fraction_delta = (delta_x / rect.width()) * view_width;
            let target_center = view_center + fraction_delta;
            app.controller.scroll_waveform_view(target_center);
        }
        return;
    }
    let zoom_delta = scroll_delta * 0.6;
    let zoom_in = zoom_delta.y > 0.0;
    let per_step_factor = app.controller.ui.controls.wheel_zoom_factor;
    let zoom_steps = zoom_delta.y.abs().round().max(1.0) as u32;
    let focus_override = response
        .hover_pos()
        .or_else(|| response.interact_pointer_pos())
        .map(|pos| {
            ((pos.x - rect.left()) / rect.width())
                .mul_add(view_width, view.start)
                .clamp(0.0, 1.0)
        });
    app.controller.zoom_waveform_steps_with_factor(
        zoom_in,
        zoom_steps,
        focus_override,
        Some(per_step_factor),
        false,
        false,
    );
}
