use super::style;
use super::*;
use crate::egui_app::view_model;
use eframe::egui;

const MAP_POINT_LIMIT: usize = 50_000;
const MAP_HEATMAP_BINS: usize = 64;
const MAP_ZOOM_MIN: f32 = 0.2;
const MAP_ZOOM_MAX: f32 = 20.0;
const MAP_ZOOM_SPEED: f32 = 0.0015;

impl EguiApp {
    pub(super) fn render_map_window(&mut self, ctx: &egui::Context) {
        if !self.controller.ui.map.open {
            return;
        }
        egui::Window::new("Sample Map")
            .collapsible(false)
            .resizable(true)
            .default_size([640.0, 420.0])
            .show(ctx, |ui| {
                self.render_map_canvas(ui);
            });
    }

    fn render_map_canvas(&mut self, ui: &mut egui::Ui) {
        let palette = style::palette();
        let available = ui.available_size();
        let (rect, response) = ui.allocate_exact_size(available, egui::Sense::drag());
        let model_id = crate::analysis::embedding::EMBEDDING_MODEL_ID;
        let umap_version = self.controller.ui.map.umap_version.clone();

        if self.controller.ui.map.bounds.is_none() {
            match self.controller.umap_bounds(model_id, &umap_version) {
                Ok(bounds) => {
                    self.controller.ui.map.bounds = bounds.map(|b| {
                        crate::egui_app::state::MapBounds {
                            min_x: b.min_x,
                            max_x: b.max_x,
                            min_y: b.min_y,
                            max_y: b.max_y,
                        }
                    });
                }
                Err(err) => {
                    self.controller
                        .set_status(format!("UMAP bounds failed: {err}"), style::StatusTone::Error);
                }
            }
        }

        let Some(bounds) = self.controller.ui.map.bounds else {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "No layout data. Run sempal-umap first.",
                egui::TextStyle::Body.resolve(ui.style()),
                palette.text_muted,
            );
            return;
        };

        let scroll_delta = ui.input(|i| i.smooth_scroll_delta.y);
        if response.hovered() && scroll_delta.abs() > 0.0 {
            let zoom_delta = 1.0 + scroll_delta * MAP_ZOOM_SPEED;
            self.controller.ui.map.zoom =
                (self.controller.ui.map.zoom * zoom_delta).clamp(MAP_ZOOM_MIN, MAP_ZOOM_MAX);
        }

        let pointer = response.interact_pointer_pos();
        if response.dragged() {
            if let Some(pos) = pointer {
                let last = self.controller.ui.map.last_drag_pos.unwrap_or(pos);
                let delta = pos - last;
                self.controller.ui.map.pan += delta;
                self.controller.ui.map.last_drag_pos = Some(pos);
            }
        } else {
            self.controller.ui.map.last_drag_pos = None;
        }

        let scale = map_scale(rect, bounds, self.controller.ui.map.zoom);
        let center = egui::pos2(
            (bounds.min_x + bounds.max_x) * 0.5,
            (bounds.min_y + bounds.max_y) * 0.5,
        );
        let world_bounds = world_bounds_from_view(rect, center, scale, self.controller.ui.map.pan);
        if should_requery(&self.controller.ui.map.last_query, &world_bounds) {
            match self.controller.umap_points_in_bounds(
                model_id,
                &umap_version,
                world_bounds,
                MAP_POINT_LIMIT,
            ) {
                Ok(points) => {
                    self.controller.ui.map.cached_points = points
                        .into_iter()
                        .map(|p| crate::egui_app::state::MapPoint {
                            sample_id: p.sample_id,
                            x: p.x,
                            y: p.y,
                        })
                        .collect();
                    self.controller.ui.map.last_query = Some(world_bounds);
                }
                Err(err) => {
                    self.controller.set_status(
                        format!("UMAP query failed: {err}"),
                        style::StatusTone::Error,
                    );
                }
            }
        }

        let points = self.controller.ui.map.cached_points.clone();
        let painter = ui.painter_at(rect);
        let hovered = find_hover_point(&points, rect, center, scale, self.controller.ui.map.pan, pointer);
        self.controller.ui.map.hovered_sample_id = hovered.as_ref().map(|(point, _)| point.sample_id.clone());

        if let Some((point, pos)) = hovered.as_ref() {
            painter.circle_stroke(*pos, 4.0, egui::Stroke::new(1.5, palette.accent_mint));
            egui::Tooltip::always_open(
                ui.ctx().clone(),
                ui.layer_id(),
                egui::Id::new("map_hover_tooltip"),
                egui::PopupAnchor::Pointer,
            )
            .show(|ui| {
                ui.label(sample_label_from_id(&point.sample_id));
                ui.label("Click to audition");
            });
        }

        if response.clicked() {
            if let Some((point, _)) = hovered.as_ref() {
                self.controller.ui.map.selected_sample_id = Some(point.sample_id.clone());
                if let Err(err) = self.controller.preview_sample_by_id(&point.sample_id) {
                    self.controller
                        .set_status(format!("Preview failed: {err}"), style::StatusTone::Error);
                } else if let Err(err) = self.controller.play_audio(false, None) {
                    self.controller
                        .set_status(format!("Playback failed: {err}"), style::StatusTone::Error);
                }
            }
        }

        let context_point = hovered.as_ref().map(|(point, _)| point.sample_id.clone());
        response.context_menu(|ui| {
            let Some(sample_id) = context_point.as_ref() else {
                ui.label("Hover a point to see map actions.");
                return;
            };
            let Some((point, _)) = hovered.as_ref() else {
                ui.label("Hover a point to see map actions.");
                return;
            };
            ui.label(sample_label_from_id(&point.sample_id));
            if ui.button("Preview").clicked() {
                if let Err(err) = self.controller.preview_sample_by_id(sample_id) {
                    self.controller
                        .set_status(format!("Preview failed: {err}"), style::StatusTone::Error);
                } else if let Err(err) = self.controller.play_audio(false, None) {
                    self.controller
                        .set_status(format!("Playback failed: {err}"), style::StatusTone::Error);
                }
                ui.close();
            }
            let labels = match self.controller.list_tf_labels() {
                Ok(labels) => labels,
                Err(err) => {
                    self.controller.set_status(
                        format!("Load labels failed: {err}"),
                        style::StatusTone::Error,
                    );
                    Vec::new()
                }
            };
            ui.menu_button("Add as anchor to...", |ui| {
                if labels.is_empty() {
                    ui.label("No labels yet");
                }
                for label in &labels {
                    if ui.button(&label.name).clicked() {
                        if let Err(err) = self.controller.add_tf_anchor(
                            &label.label_id,
                            sample_id,
                            1.0,
                        ) {
                            self.controller.set_status(
                                format!("Add anchor failed: {err}"),
                                style::StatusTone::Error,
                            );
                        } else {
                            self.controller.set_status(
                                format!("Added anchor to {}", label.name),
                                style::StatusTone::Info,
                            );
                            self.controller.clear_tf_label_score_cache();
                            ui.close();
                        }
                    }
                }
            });
        });

        if points.len() > 8000 || self.controller.ui.map.zoom < 0.6 {
            render_heatmap(&painter, rect, points, center, scale, self.controller.ui.map.pan);
        } else {
            for point in points {
                let pos = map_to_screen(point.x, point.y, rect, center, scale, self.controller.ui.map.pan);
                if rect.contains(pos) {
                    let radius = if self.controller.ui.map.selected_sample_id.as_deref()
                        == Some(point.sample_id.as_str())
                    {
                        3.5
                    } else {
                        2.0
                    };
                    painter.circle_filled(pos, radius, palette.accent_mint);
                }
            }
        }
    }
}

fn map_scale(rect: egui::Rect, bounds: crate::egui_app::state::MapBounds, zoom: f32) -> f32 {
    let world_w = (bounds.max_x - bounds.min_x).max(1e-3);
    let world_h = (bounds.max_y - bounds.min_y).max(1e-3);
    let scale_x = rect.width() / world_w;
    let scale_y = rect.height() / world_h;
    let base = scale_x.min(scale_y) * 0.9;
    base * zoom
}

fn map_to_screen(
    x: f32,
    y: f32,
    rect: egui::Rect,
    center: egui::Pos2,
    scale: f32,
    pan: egui::Vec2,
) -> egui::Pos2 {
    let dx = (x - center.x) * scale;
    let dy = (y - center.y) * scale;
    egui::pos2(rect.center().x + dx + pan.x, rect.center().y + dy + pan.y)
}

fn world_bounds_from_view(
    rect: egui::Rect,
    center: egui::Pos2,
    scale: f32,
    pan: egui::Vec2,
) -> crate::egui_app::state::MapQueryBounds {
    let to_world = |pos: egui::Pos2| {
        let dx = (pos.x - rect.center().x - pan.x) / scale;
        let dy = (pos.y - rect.center().y - pan.y) / scale;
        (center.x + dx, center.y + dy)
    };
    let (min_x, min_y) = to_world(rect.min);
    let (max_x, max_y) = to_world(rect.max);
    crate::egui_app::state::MapQueryBounds {
        min_x: min_x.min(max_x),
        max_x: min_x.max(max_x),
        min_y: min_y.min(max_y),
        max_y: min_y.max(max_y),
    }
}

fn should_requery(
    last: &Option<crate::egui_app::state::MapQueryBounds>,
    next: &crate::egui_app::state::MapQueryBounds,
) -> bool {
    match last {
        None => true,
        Some(prev) => {
            let dx = (prev.min_x - next.min_x).abs()
                + (prev.max_x - next.max_x).abs();
            let dy = (prev.min_y - next.min_y).abs()
                + (prev.max_y - next.max_y).abs();
            dx + dy > 0.05
        }
    }
}

fn render_heatmap(
    painter: &egui::Painter,
    rect: egui::Rect,
    points: &[crate::egui_app::state::MapPoint],
    center: egui::Pos2,
    scale: f32,
    pan: egui::Vec2,
) {
    let mut bins = vec![0u32; MAP_HEATMAP_BINS * MAP_HEATMAP_BINS];
    let width = rect.width().max(1.0);
    let height = rect.height().max(1.0);
    for point in points {
        let pos = map_to_screen(point.x, point.y, rect, center, scale, pan);
        if !rect.contains(pos) {
            continue;
        }
        let nx = ((pos.x - rect.min.x) / width).clamp(0.0, 0.999);
        let ny = ((pos.y - rect.min.y) / height).clamp(0.0, 0.999);
        let ix = (nx * MAP_HEATMAP_BINS as f32) as usize;
        let iy = (ny * MAP_HEATMAP_BINS as f32) as usize;
        let idx = iy * MAP_HEATMAP_BINS + ix;
        if let Some(cell) = bins.get_mut(idx) {
            *cell = cell.saturating_add(1);
        }
    }
    let max_count = bins.iter().copied().max().unwrap_or(1).max(1) as f32;
    for iy in 0..MAP_HEATMAP_BINS {
        for ix in 0..MAP_HEATMAP_BINS {
            let idx = iy * MAP_HEATMAP_BINS + ix;
            let count = bins[idx] as f32;
            if count <= 0.0 {
                continue;
            }
            let intensity = (count / max_count).clamp(0.0, 1.0);
            let alpha = (intensity * 200.0) as u8;
            let color = egui::Color32::from_rgba_premultiplied(80, 180, 255, alpha);
            let cell_w = rect.width() / MAP_HEATMAP_BINS as f32;
            let cell_h = rect.height() / MAP_HEATMAP_BINS as f32;
            let min = egui::pos2(
                rect.min.x + ix as f32 * cell_w,
                rect.min.y + iy as f32 * cell_h,
            );
            let max = egui::pos2(min.x + cell_w, min.y + cell_h);
            painter.rect_filled(egui::Rect::from_min_max(min, max), 0.0, color);
        }
    }
}

fn find_hover_point(
    points: &[crate::egui_app::state::MapPoint],
    rect: egui::Rect,
    center: egui::Pos2,
    scale: f32,
    pan: egui::Vec2,
    pointer: Option<egui::Pos2>,
) -> Option<(crate::egui_app::state::MapPoint, egui::Pos2)> {
    let pointer = pointer?;
    if !rect.contains(pointer) {
        return None;
    }
    let mut best: Option<(crate::egui_app::state::MapPoint, egui::Pos2, f32)> = None;
    for point in points {
        let pos = map_to_screen(point.x, point.y, rect, center, scale, pan);
        let dist_sq = pos.distance_sq(pointer);
        if dist_sq > 36.0 {
            continue;
        }
        match best {
            Some((_, _, best_sq)) if dist_sq >= best_sq => {}
            _ => {
                best = Some((point.clone(), pos, dist_sq));
            }
        }
    }
    best.map(|(point, pos, _)| (point, pos))
}

fn sample_label_from_id(sample_id: &str) -> String {
    if let Some((_, rel)) = sample_id.split_once("::") {
        let path = std::path::Path::new(rel);
        return view_model::sample_display_label(path);
    }
    sample_id.to_string()
}
