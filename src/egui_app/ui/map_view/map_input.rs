use super::map_interactions;
use super::map_state;
use super::style;
use super::EguiApp;
use crate::egui_app::state::{FocusContext, SampleBrowserTab};
use eframe::egui::{self, popup::show_context_menu};

pub(super) fn handle_zoom(app: &mut EguiApp, ui: &egui::Ui, response: &egui::Response) {
    let scroll_delta = ui.input(|i| i.smooth_scroll_delta.y);
    if response.hovered() && scroll_delta.abs() > 0.0 {
        let zoom_delta = 1.0 + scroll_delta * super::MAP_ZOOM_SPEED;
        app.controller.ui.map.zoom =
            (app.controller.ui.map.zoom * zoom_delta).clamp(super::MAP_ZOOM_MIN, super::MAP_ZOOM_MAX);
    }
}

pub(super) fn handle_pan(
    app: &mut EguiApp,
    response: &egui::Response,
    pointer: Option<egui::Pos2>,
) {
    if response.dragged_by(egui::PointerButton::Secondary) {
        if let Some(pos) = pointer {
            let last = app.controller.ui.map.last_drag_pos.unwrap_or(pos);
            let delta = pos - last;
            app.controller.ui.map.pan += delta;
            app.controller.ui.map.last_drag_pos = Some(pos);
        }
    } else {
        app.controller.ui.map.last_drag_pos = None;
    }
}

pub(super) fn handle_focus_request(
    app: &mut EguiApp,
    model_id: &str,
    umap_version: &str,
    _bounds: crate::egui_app::state::MapBounds,
    center: egui::Pos2,
    scale: f32,
) {
    if !app.controller.ui.map.focus_selected_requested {
        return;
    }
    app.controller.ui.map.focus_selected_requested = false;
    let target_id = app
        .controller
        .ui
        .map
        .selected_sample_id
        .clone()
        .or_else(|| app.controller.ui.map.hovered_sample_id.clone());
    if let Some(sample_id) = target_id {
        let mut target_point = app
            .controller
            .ui
            .map
            .cached_points
            .iter()
            .find(|point| point.sample_id == sample_id)
            .map(|point| (point.x, point.y));
        if target_point.is_none() {
            match app
                .controller
                .umap_point_for_sample(model_id, umap_version, &sample_id)
            {
                Ok(point) => {
                    target_point = point;
                }
                Err(err) => {
                    app.controller.set_status(
                        format!("Map focus failed: {err}"),
                        style::StatusTone::Error,
                    );
                }
            }
        }
        if let Some((x, y)) = target_point {
            let dx = (x - center.x) * scale;
            let dy = (y - center.y) * scale;
            app.controller.ui.map.pan = egui::vec2(-dx, -dy);
            app.controller.ui.map.last_query = None;
        } else {
            app.controller.set_status(
                "Map focus failed: sample not in layout",
                style::StatusTone::Warning,
            );
        }
    } else {
        app.controller.set_status(
            "Select a sample to focus the map",
            style::StatusTone::Info,
        );
    }
}

pub(super) fn resolve_hover(
    app: &mut EguiApp,
    rect: egui::Rect,
    center: egui::Pos2,
    scale: f32,
    pan: egui::Vec2,
    pointer: Option<egui::Pos2>,
) -> Option<(crate::egui_app::state::MapPoint, egui::Pos2)> {
    let display_points = &app.controller.ui.map.cached_filtered_points;
    let hovered = map_interactions::find_hover_point(display_points, rect, center, scale, pan, pointer);
    app.controller.ui.map.hovered_sample_id =
        hovered.as_ref().map(|(point, _)| point.sample_id.clone());
    if app.controller.ui.map.hovered_sample_id.is_none() {
        app.controller.ui.map.paint_hover_active_id = None;
    }
    hovered
}

pub(super) fn handle_paint_hover(
    app: &mut EguiApp,
    ui: &egui::Ui,
    hovered: Option<&(crate::egui_app::state::MapPoint, egui::Pos2)>,
) {
    let Some((point, _)) = hovered else {
        return;
    };
    let same_sample = app
        .controller
        .ui
        .map
        .paint_hover_active_id
        .as_deref()
        == Some(point.sample_id.as_str());
    if same_sample {
        return;
    }
    app.controller.ui.map.paint_hover_active_id = Some(point.sample_id.clone());
    app.controller.ui.map.selected_sample_id = Some(point.sample_id.clone());
    if let Err(err) = app.controller.focus_sample_from_map(&point.sample_id) {
        app.controller.set_status(
            format!("Map focus failed: {err}"),
            style::StatusTone::Error,
        );
    }
    if let Err(err) = app.controller.preview_sample_by_id(&point.sample_id) {
        app.controller
            .set_status(format!("Preview failed: {err}"), style::StatusTone::Error);
    } else if let Err(err) = app.controller.play_audio(false, None) {
        app.controller
            .set_status(format!("Playback failed: {err}"), style::StatusTone::Error);
    }
    ui.ctx().request_repaint();
}

pub(super) fn handle_click(
    app: &mut EguiApp,
    hovered: Option<&(crate::egui_app::state::MapPoint, egui::Pos2)>,
) {
    if let Some((point, _)) = hovered {
        app.controller.ui.map.selected_sample_id = Some(point.sample_id.clone());
        if let Err(err) = app.controller.focus_sample_from_map(&point.sample_id) {
            app.controller.set_status(
                format!("Map focus failed: {err}"),
                style::StatusTone::Error,
            );
        }
        if let Err(err) = app.controller.preview_sample_by_id(&point.sample_id) {
            app.controller
                .set_status(format!("Preview failed: {err}"), style::StatusTone::Error);
        } else if let Err(err) = app.controller.play_audio(false, None) {
            app.controller
                .set_status(format!("Playback failed: {err}"), style::StatusTone::Error);
        }
    }
}

pub(super) fn handle_context_menu(
    app: &mut EguiApp,
    ui: &mut egui::Ui,
    response: &egui::Response,
    hovered: Option<&(crate::egui_app::state::MapPoint, egui::Pos2)>,
) {
    if !response.secondary_clicked() {
        return;
    }
    let context_sample = hovered.map(|(point, _)| point.sample_id.clone());
    show_context_menu(ui.ctx(), ui.id().with("similarity_map_context"), |ui| {
        let Some(sample_id) = context_sample.as_ref() else {
            ui.label("Hover a point to see map actions.");
            return;
        };
        ui.label(map_state::sample_label_from_id(sample_id));

        if ui.button("Preview").clicked() {
            if let Err(err) = app.controller.preview_sample_by_id(sample_id) {
                app.controller
                    .set_status(format!("Preview failed: {err}"), style::StatusTone::Error);
            } else if let Err(err) = app.controller.play_audio(false, None) {
                app.controller
                    .set_status(format!("Playback failed: {err}"), style::StatusTone::Error);
            }
            ui.close_menu();
            return;
        }

        ui.separator();
        if ui.button("List similar").clicked() {
            app.controller.ui.browser.active_tab = SampleBrowserTab::List;
            if let Err(err) = app.controller.find_similar_for_sample_id(sample_id) {
                app.controller.set_status(
                    format!("Find similar failed: {err}"),
                    style::StatusTone::Error,
                );
            } else {
                app.controller
                    .focus_context_from_ui(FocusContext::SampleBrowser);
            }
            ui.close_menu();
        }
    });
}
