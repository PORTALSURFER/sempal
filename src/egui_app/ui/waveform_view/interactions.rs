use super::style;
use super::*;
use crate::egui_app::state::WaveformView;
use eframe::egui::{self, Ui};

pub(super) fn handle_waveform_interactions(
    app: &mut EguiApp,
    ui: &mut Ui,
    rect: egui::Rect,
    response: &egui::Response,
    view: WaveformView,
    view_width: f32,
) {
    // Waveform interactions: scroll to zoom, click to seek, drag to select.
    if response.hovered() {
        let scroll_delta = ui.input(|i| i.raw_scroll_delta);
        if scroll_delta != egui::Vec2::ZERO {
            let shift_down = ui.input(|i| i.modifiers.shift);
            if shift_down && view_width < 1.0 {
                // Pan the zoomed view horizontally when shift is held.
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
            } else {
                let zoom_delta = scroll_delta * 0.6;
                let zoom_in = zoom_delta.y > 0.0;
                let per_step_factor = app.controller.ui.controls.wheel_zoom_factor;
                // Use playhead when visible, otherwise pointer if available, otherwise center.
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
        }
    }
}

pub(super) fn handle_waveform_pointer_interactions(
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
    // Anchor creation to the initial press so quick drags keep the original start.
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
        } else if let Some(value) = normalized {
            app.controller.seek_to(value);
        }
    }
}

pub(super) fn render_waveform_scrollbar(
    app: &mut EguiApp,
    ui: &mut Ui,
    rect: egui::Rect,
    view: WaveformView,
    view_width: f32,
) {
    let palette = style::palette();
    let bar_height = 12.0;
    let scroll_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left(), rect.bottom() - bar_height),
        egui::vec2(rect.width(), bar_height),
    );
    let scroll_resp = ui.interact(
        scroll_rect,
        ui.id().with("waveform_scrollbar"),
        egui::Sense::click_and_drag(),
    );
    let scroll_bg = style::with_alpha(palette.bg_primary, 140);
    ui.painter().rect_filled(scroll_rect, 0.0, scroll_bg);
    let indicator_width = scroll_rect.width() * view_width;
    let indicator_x = scroll_rect.left() + scroll_rect.width() * view.start;
    let indicator_rect = egui::Rect::from_min_size(
        egui::pos2(indicator_x, scroll_rect.top()),
        egui::vec2(indicator_width.max(8.0), scroll_rect.height()),
    );
    let thumb_color = style::with_alpha(palette.accent_ice, 200);
    ui.painter().rect_filled(indicator_rect, 0.0, thumb_color);
    if (scroll_resp.dragged() || scroll_resp.clicked())
        && scroll_rect.width() > f32::EPSILON
        && let Some(pos) = scroll_resp.interact_pointer_pos()
    {
        let frac = ((pos.x - scroll_rect.left()) / scroll_rect.width()).clamp(0.0, 1.0);
        app.controller.scroll_waveform_view(frac);
    }
}

