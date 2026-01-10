use super::super::*;
use super::super::selection_geometry;
use crate::egui_app::state::WaveformView;
use crate::egui_app::ui::style::StatusTone;
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
    let primary_down = ui.input(|i| i.pointer.button_down(egui::PointerButton::Primary));
    let secondary_down = ui.input(|i| i.pointer.button_down(egui::PointerButton::Secondary));
    let modifiers = ui.input(|i| i.modifiers);
    let slide_modifiers = modifiers.ctrl && modifiers.shift && modifiers.alt;
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
    if response.drag_started() && slide_modifiers && primary_down {
        if let Some(value) = drag_start_normalized.or(normalized) {
            if app.controller.ui.waveform.image.is_some() {
                app.controller.focus_waveform_context();
            }
            if let Err(err) = app.controller.start_waveform_circular_slide(value) {
                app.controller.set_status(err, StatusTone::Error);
            }
        }
        return;
    }
    if response.dragged() && app.controller.is_waveform_circular_slide_active() {
        if let Some(value) = normalized {
            app.controller.update_waveform_circular_slide(value);
        }
        return;
    }
    if response.drag_stopped() && app.controller.is_waveform_circular_slide_active() {
        if let Err(err) = app.controller.finish_waveform_circular_slide() {
            app.controller.set_status(err, StatusTone::Error);
        }
        return;
    }
    if response.drag_started() {
        if let Some(value) = drag_start_normalized {
            if secondary_down {
                app.controller.start_edit_selection_drag(value);
            } else if app.controller.ui.waveform.slice_mode_enabled {
                start_slice_paint(app, value);
            } else if primary_down {
                app.controller.start_selection_drag(value);
            }
        }
    } else if response.dragged() {
        if let Some(value) = normalized {
            if app.controller.ui.waveform.image.is_some() {
                app.controller.focus_waveform_context();
            }
            if response.dragged_by(egui::PointerButton::Secondary) {
                app.controller.update_edit_selection_drag(value, false);
            } else if app.controller.ui.waveform.slice_mode_enabled {
                update_slice_paint(app, value);
            } else if response.dragged_by(egui::PointerButton::Primary) {
                app.controller.update_selection_drag(value, false);
            }
        }
    } else if response.drag_stopped() {
        if app.controller.is_edit_selection_dragging() && !secondary_down {
            app.controller.finish_edit_selection_drag();
        } else if app.controller.ui.waveform.slice_mode_enabled {
            finish_slice_paint(app);
        } else if app.controller.is_selection_dragging() {
            app.controller.finish_selection_drag();
        }
    } else if response.clicked_by(egui::PointerButton::Secondary) {
        if app.controller.ui.waveform.edit_selection.is_some() {
            let clicked_pos = pointer_pos.or_else(|| response.hover_pos());
            let on_selection = clicked_pos
                .map(|pos| {
                    app.controller
                        .ui
                        .waveform
                        .selection
                        .map(|selection| {
                            selection_geometry::selection_rect_for_view(
                                selection,
                                rect,
                                view,
                                view_width,
                            )
                            .contains(pos)
                        })
                        .unwrap_or(false)
                        || app
                            .controller
                            .ui
                            .waveform
                            .edit_selection
                            .map(|selection| {
                                selection_geometry::selection_rect_for_view(
                                    selection,
                                    rect,
                                    view,
                                    view_width,
                                )
                                .contains(pos)
                            })
                            .unwrap_or(false)
                })
                .unwrap_or(true);
            if !on_selection {
                app.controller.clear_edit_selection();
            }
        }
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

fn start_slice_paint(app: &mut EguiApp, position: f32) {
    let snapped = app.controller.snap_slice_paint_position(position, false);
    app.slice_paint = Some(super::super::SlicePaintState {
        anchor: snapped,
        range: crate::selection::SelectionRange::new(snapped, snapped),
    });
}

fn update_slice_paint(app: &mut EguiApp, position: f32) {
    let Some(state) = app.slice_paint.as_mut() else {
        return;
    };
    let snapped = app.controller.snap_slice_paint_position(position, false);
    state.range = crate::selection::SelectionRange::new(state.anchor, snapped);
}

fn finish_slice_paint(app: &mut EguiApp) {
    let Some(state) = app.slice_paint.take() else {
        return;
    };
    app.controller.apply_painted_slice(state.range);
}
