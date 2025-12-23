use super::map_clusters;
use super::map_empty;
use super::map_interactions;
use super::map_math;
use super::map_render;
use super::style;
use super::*;
use crate::egui_app::view_model;
use eframe::egui;
use std::time::Instant;
use std::sync::Arc;

const MAP_POINT_LIMIT: usize = 50_000;
const MAP_HEATMAP_BINS: usize = 64;
const MAP_ZOOM_MIN: f32 = 0.2;
const MAP_ZOOM_MAX: f32 = 20.0;
const MAP_ZOOM_SPEED: f32 = 0.0015;
impl EguiApp {
    pub(super) fn render_map_panel(&mut self, ui: &mut egui::Ui) {
        let refresh = self.render_map_controls(ui);
        if refresh {
            self.controller.ui.map.last_query = None;
        }
        ui.separator();
        self.render_map_canvas(ui);
    }

    pub(super) fn render_map_window(&mut self, ctx: &egui::Context) {
        if !self.controller.ui.map.open {
            return;
        }
        egui::Window::new("Sample Map")
            .collapsible(false)
            .resizable(true)
            .default_size([640.0, 420.0])
            .show(ctx, |ui| {
                self.render_map_panel(ui);
            });
    }

    fn render_map_controls(&mut self, ui: &mut egui::Ui) -> bool {
        let mut refresh = false;
        self.controller.ui.map.cluster_overlay = true;
        self.controller.ui.map.similarity_blend = true;
        self.controller.ui.map.similarity_blend_threshold = 0.2;
        self.controller.ui.map.cluster_filter_input.clear();
        self.controller.ui.map.cluster_filter = None;
        ui.horizontal(|ui| {
        });
        ui.horizontal(|ui| {
            let mode = match self.controller.ui.map.last_render_mode {
                crate::egui_app::state::MapRenderMode::Heatmap => "heatmap",
                crate::egui_app::state::MapRenderMode::Points => "points",
            };
            ui.label(format!(
                "Frame {:.2} ms | draw {} | points {} | {}",
                self.controller.ui.map.last_render_ms,
                self.controller.ui.map.last_draw_calls,
                self.controller.ui.map.last_points_rendered,
                mode
            ));
        });
        if self.controller.ui.map.cluster_overlay {
            if let Some(stats) = map_clusters::compute_cluster_stats(&self.controller.ui.map.cached_points) {
                ui.horizontal(|ui| {
                    ui.label(format!("Clusters: {}", stats.cluster_count));
                    if stats.missing_count > 0 {
                        let missing_ratio =
                            stats.missing_count as f32 / stats.total_count.max(1) as f32;
                        ui.label(format!("Missing: {:.1}%", missing_ratio * 100.0));
                    }
                    ui.label(format!(
                        "Size min/max: {}/{}",
                        stats.min_cluster_size, stats.max_cluster_size
                    ));
                });
            } else {
                ui.label("Clusters missing for this view (press Build clusters).");
            }
        }
        refresh
    }

    fn render_map_canvas(&mut self, ui: &mut egui::Ui) {
        let palette = style::palette();
        let available = ui.available_size();
        let (rect, response) = ui.allocate_exact_size(available, egui::Sense::click_and_drag());
        let render_started = Instant::now();
        let model_id = crate::analysis::embedding::EMBEDDING_MODEL_ID;
        let umap_version = self.controller.ui.map.umap_version.clone();
        let cluster_method_str = "umap";
        let cluster_umap_version = umap_version.as_str();

        let source_id = self.controller.current_source().map(|source| source.id);
        if self.controller.ui.map.bounds.is_none() {
            match self
                .controller
                .umap_bounds(model_id, &umap_version, source_id.as_ref())
            {
                Ok(bounds) => {
                    self.controller.ui.map.bounds =
                        bounds.map(|b| crate::egui_app::state::MapBounds {
                            min_x: b.min_x,
                            max_x: b.max_x,
                            min_y: b.min_y,
                            max_y: b.max_y,
                        });
                }
                Err(err) => {
                    self.controller.set_status(
                        format!("t-SNE bounds failed: {err}"),
                        style::StatusTone::Error,
                    );
                }
            }
        }

        let focused_sample_id = self.controller.selected_sample_id();
        if self.controller.ui.map.selected_sample_id != focused_sample_id {
            self.controller.ui.map.selected_sample_id = focused_sample_id;
        }

        let Some(bounds) = self.controller.ui.map.bounds else {
            if map_empty::render_empty_state(ui, rect, &palette) {
                self.controller.build_umap_layout(model_id, &umap_version);
            }
            return;
        };

        let scroll_delta = ui.input(|i| i.smooth_scroll_delta.y);
        if response.hovered() && scroll_delta.abs() > 0.0 {
            let zoom_delta = 1.0 + scroll_delta * MAP_ZOOM_SPEED;
            self.controller.ui.map.zoom =
                (self.controller.ui.map.zoom * zoom_delta).clamp(MAP_ZOOM_MIN, MAP_ZOOM_MAX);
        }

        let pointer = response.interact_pointer_pos();
        if response.dragged_by(egui::PointerButton::Secondary) {
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
        if self.controller.ui.map.focus_selected_requested {
            self.controller.ui.map.focus_selected_requested = false;
            let target_id = self
                .controller
                .ui
                .map
                .selected_sample_id
                .clone()
                .or_else(|| self.controller.ui.map.hovered_sample_id.clone());
            if let Some(sample_id) = target_id {
                let mut target_point = self
                    .controller
                    .ui
                    .map
                    .cached_points
                    .iter()
                    .find(|point| point.sample_id == sample_id)
                    .map(|point| (point.x, point.y));
                if target_point.is_none() {
                    match self
                        .controller
                        .umap_point_for_sample(model_id, &umap_version, &sample_id)
                    {
                        Ok(point) => {
                            target_point = point;
                        }
                        Err(err) => {
                            self.controller.set_status(
                                format!("Map focus failed: {err}"),
                                style::StatusTone::Error,
                            );
                        }
                    }
                }
                if let Some((x, y)) = target_point {
                    let dx = (x - center.x) * scale;
                    let dy = (y - center.y) * scale;
                    self.controller.ui.map.pan = egui::vec2(-dx, -dy);
                    self.controller.ui.map.last_query = None;
                } else {
                    self.controller.set_status(
                        "Map focus failed: sample not in layout",
                        style::StatusTone::Warning,
                    );
                }
            } else {
                self.controller.set_status(
                    "Select a sample to focus the map",
                    style::StatusTone::Info,
                );
            }
        }
        let world_bounds =
            map_math::world_bounds_from_view(rect, center, scale, self.controller.ui.map.pan);
        if map_math::should_requery(&self.controller.ui.map.last_query, &world_bounds) {
            match self.controller.umap_points_in_bounds(
                model_id,
                &umap_version,
                cluster_method_str,
                cluster_umap_version,
                source_id.as_ref(),
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
                            cluster_id: p.cluster_id,
                        })
                        .collect();
                    self.controller.ui.map.last_query = Some(world_bounds);
                }
                Err(err) => {
                    self.controller.set_status(
                        format!("t-SNE query failed: {err}"),
                        style::StatusTone::Error,
                    );
                }
            }
        }

        let points = self.controller.ui.map.cached_points.clone();
        let filtered_points = map_clusters::filter_points(
            &points,
            self.controller.ui.map.cluster_overlay,
            self.controller.ui.map.cluster_filter,
        );
        let cluster_overlay = self.controller.ui.map.cluster_overlay;
        let similarity_blend = self.controller.ui.map.similarity_blend;
        let blend_threshold = self.controller.ui.map.similarity_blend_threshold;
        let source_key = source_id.as_ref().map(|id| id.as_str().to_string());
        let centroids_key = format!(
            "{}|{}|{}|{}",
            umap_version,
            source_key.as_deref().unwrap_or(""),
            cluster_method_str,
            cluster_umap_version
        );
        if self.controller.ui.map.cached_cluster_centroids_key.as_deref() != Some(&centroids_key) {
            self.controller.ui.map.cached_cluster_centroids_key = Some(centroids_key);
            self.controller.ui.map.cached_cluster_centroids = None;
            self.controller.ui.map.auto_cluster_build_requested_key = None;
        }
        if cluster_overlay && self.controller.ui.map.cached_cluster_centroids.is_none() {
            match self.controller.umap_cluster_centroids(
                model_id,
                &umap_version,
                cluster_method_str,
                cluster_umap_version,
                source_id.as_ref(),
            ) {
                Ok(centroids) => {
                    self.controller.ui.map.cached_cluster_centroids = Some(Arc::new(centroids));
                }
                Err(err) => {
                    self.controller.set_status(
                        format!("Cluster centroids query failed: {err}"),
                        style::StatusTone::Error,
                    );
                }
            }
        }
        if cluster_overlay {
            let has_any_points = !points.is_empty();
            let has_missing_cluster_ids = points.iter().any(|point| point.cluster_id.is_none());
            let centroids_empty = self
                .controller
                .ui
                .map
                .cached_cluster_centroids
                .as_ref()
                .is_some_and(|centroids| centroids.is_empty());
            if has_any_points
                && (has_missing_cluster_ids || centroids_empty)
                && self
                    .controller
                    .ui
                    .map
                    .auto_cluster_build_requested_key
                    .is_none()
            {
                self.controller.ui.map.auto_cluster_build_requested_key =
                    self.controller.ui.map.cached_cluster_centroids_key.clone();
                let umap_version = umap_version.clone();
                self.controller
                    .build_umap_clusters(crate::analysis::embedding::EMBEDDING_MODEL_ID, &umap_version);
            }
        }
        let centroids_arc = if cluster_overlay {
            self.controller
                .ui
                .map
                .cached_cluster_centroids
                .clone()
                .filter(|centroids| !centroids.is_empty())
                .or_else(|| Some(Arc::new(map_clusters::cluster_centroids(&points))))
        } else {
            None
        };
        let blend_enabled = cluster_overlay && similarity_blend;
        let map_diagonal =
            ((bounds.max_x - bounds.min_x).powi(2) + (bounds.max_y - bounds.min_y).powi(2)).sqrt();
        let point_color = |point: &crate::egui_app::state::MapPoint, alpha: u8| {
            if cluster_overlay {
                if blend_enabled {
                    map_clusters::blended_cluster_color(
                        point,
                        centroids_arc
                            .as_ref()
                            .expect("centroids set for cluster overlay"),
                        &bounds,
                        &palette,
                        alpha,
                        map_diagonal,
                        blend_threshold,
                    )
                } else {
                    map_clusters::distance_shaded_cluster_color(
                        point,
                        centroids_arc
                            .as_ref()
                            .expect("centroids set for cluster overlay"),
                        &bounds,
                        &palette,
                        alpha,
                        map_diagonal,
                    )
                }
            } else {
                palette.accent_mint
            }
        };
        let display_points = filtered_points.clone();
        let painter = ui.painter_at(rect);
        let hovered = map_interactions::find_hover_point(
            &display_points,
            rect,
            center,
            scale,
            self.controller.ui.map.pan,
            pointer,
        );
        self.controller.ui.map.hovered_sample_id =
            hovered.as_ref().map(|(point, _)| point.sample_id.clone());
        if self.controller.ui.map.hovered_sample_id.is_none() {
            self.controller.ui.map.paint_hover_active_id = None;
        }
        if response.dragged_by(egui::PointerButton::Primary) {
            self.paint_map_hover(ui, hovered.as_ref());
        }

        if let Some((point, pos)) = hovered.as_ref() {
            let stroke_color = point_color(point, 200);
            painter.circle_stroke(*pos, 4.0, egui::Stroke::new(1.5, stroke_color));
            egui::Tooltip::always_open(
                ui.ctx().clone(),
                ui.layer_id(),
                egui::Id::new("map_hover_tooltip"),
                egui::PopupAnchor::Pointer,
            )
            .show(|ui| {
                ui.label(sample_label_from_id(&point.sample_id));
                if self.controller.ui.map.cluster_overlay {
                    if let Some(cluster_id) = point.cluster_id {
                        ui.label(format!("Cluster: {cluster_id}"));
                    } else {
                        ui.label("Cluster: (missing)");
                    }
                }
                ui.label("Click to audition");
            });
        }

        if response.clicked() {
            if let Some((point, _)) = hovered.as_ref() {
                self.controller.ui.map.selected_sample_id = Some(point.sample_id.clone());
                if let Err(err) = self.controller.focus_sample_from_map(&point.sample_id) {
                    self.controller.set_status(
                        format!("Map focus failed: {err}"),
                        style::StatusTone::Error,
                    );
                }
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
        });

        let mut draw_calls = 0usize;
        let mut points_rendered = 0usize;
        let focused_point = self
            .controller
            .ui
            .map
            .selected_sample_id
            .as_ref()
            .and_then(|id| filtered_points.iter().find(|point| point.sample_id == *id));
        if filtered_points.len() > 8000 || self.controller.ui.map.zoom < 0.6 {
            if self.controller.ui.map.cluster_overlay {
                draw_calls = map_render::render_heatmap_with_color(
                    &painter,
                    rect,
                    &display_points,
                    center,
                    scale,
                    self.controller.ui.map.pan,
                    MAP_HEATMAP_BINS,
                    |point| point_color(point, 255),
                );
            } else {
                draw_calls = map_render::render_heatmap(
                    &painter,
                    rect,
                    &display_points,
                    center,
                    scale,
                    self.controller.ui.map.pan,
                    MAP_HEATMAP_BINS,
                );
            }
            points_rendered = display_points.len();
            self.controller.ui.map.last_render_mode =
                crate::egui_app::state::MapRenderMode::Heatmap;
        } else {
            for point in display_points {
                let pos = map_render::map_to_screen(
                    point.x,
                    point.y,
                    rect,
                    center,
                    scale,
                    self.controller.ui.map.pan,
                );
                if rect.contains(pos) {
                    points_rendered += 1;
                    let is_focused = self
                        .controller
                        .ui
                        .map
                        .selected_sample_id
                        .as_deref()
                        == Some(point.sample_id.as_str());
                    let radius = if is_focused { 3.5 } else { 2.0 };
                    let color = point_color(&point, 200);
                    painter.circle_filled(pos, radius, color);
                    draw_calls += 1;
                }
            }
            self.controller.ui.map.last_render_mode = crate::egui_app::state::MapRenderMode::Points;
        }
        if let Some(point) = focused_point {
            let pos = map_render::map_to_screen(
                point.x,
                point.y,
                rect,
                center,
                scale,
                self.controller.ui.map.pan,
            );
            if rect.contains(pos) {
                painter.circle_stroke(pos, 6.0, style::focused_row_stroke());
            }
        }
        self.controller.ui.map.last_render_ms = render_started.elapsed().as_secs_f32() * 1000.0;
        self.controller.ui.map.last_draw_calls = draw_calls;
        self.controller.ui.map.last_points_rendered = points_rendered;
    }

    fn paint_map_hover(
        &mut self,
        ui: &egui::Ui,
        hovered: Option<&(crate::egui_app::state::MapPoint, egui::Pos2)>,
    ) {
        let Some((point, _)) = hovered else {
            return;
        };
        let same_sample = self
            .controller
            .ui
            .map
            .paint_hover_active_id
            .as_deref()
            == Some(point.sample_id.as_str());
        if same_sample {
            return;
        }
        self.controller.ui.map.paint_hover_active_id = Some(point.sample_id.clone());
        self.controller.ui.map.selected_sample_id = Some(point.sample_id.clone());
        if let Err(err) = self.controller.focus_sample_from_map(&point.sample_id) {
            self.controller.set_status(
                format!("Map focus failed: {err}"),
                style::StatusTone::Error,
            );
        }
        if let Err(err) = self.controller.preview_sample_by_id(&point.sample_id) {
            self.controller
                .set_status(format!("Preview failed: {err}"), style::StatusTone::Error);
        } else if let Err(err) = self.controller.play_audio(false, None) {
            self.controller
                .set_status(format!("Playback failed: {err}"), style::StatusTone::Error);
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

fn sample_label_from_id(sample_id: &str) -> String {
    if let Some((_, rel)) = sample_id.split_once("::") {
        let path = std::path::Path::new(rel);
        return view_model::sample_display_label(path);
    }
    sample_id.to_string()
}
