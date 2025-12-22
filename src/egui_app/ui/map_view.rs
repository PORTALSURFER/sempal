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
        ui.horizontal(|ui| {
            refresh |= ui
                .checkbox(&mut self.controller.ui.map.cluster_overlay, "Clusters")
                .changed();
            if self.controller.ui.map.cluster_overlay {
                ui.checkbox(&mut self.controller.ui.map.cluster_hide_noise, "Hide noise");
                ui.checkbox(&mut self.controller.ui.map.similarity_blend, "Similarity blend");
                if self.controller.ui.map.similarity_blend {
                    let response = ui.add(
                        egui::Slider::new(
                            &mut self.controller.ui.map.similarity_blend_threshold,
                            0.02..=0.5,
                        )
                        .clamp_to_range(true)
                        .text("Blend range"),
                    );
                    if response.changed() {
                        self.controller.ui.map.similarity_blend_threshold = self
                            .controller
                            .ui
                            .map
                            .similarity_blend_threshold
                            .clamp(0.02, 0.5);
                    }
                }
                ui.label("Filter");
                let response =
                    ui.text_edit_singleline(&mut self.controller.ui.map.cluster_filter_input);
                if response.changed() {
                    self.controller.ui.map.cluster_filter = self
                        .controller
                        .ui
                        .map
                        .cluster_filter_input
                        .trim()
                        .parse::<i32>()
                        .ok();
                }
            }
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
                    ui.label(format!("Noise: {:.1}%", stats.noise_ratio * 100.0));
                    ui.label(format!("Size min/max: {}/{}", stats.min_cluster_size, stats.max_cluster_size));
                });
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
        let cluster_method_str = "embedding";
        let cluster_umap_version = "";

        let source_id = self
            .controller
            .current_source()
            .map(|source| source.id);
        if self.controller.ui.map.bounds.is_none() {
            match self
                .controller
                .umap_bounds(model_id, &umap_version, source_id.as_ref())
            {
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
            if map_empty::render_empty_state(ui, rect, &palette) {
                self.controller
                    .build_umap_layout(model_id, &umap_version);
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
                        format!("UMAP query failed: {err}"),
                        style::StatusTone::Error,
                    );
                }
            }
        }

        let points = self.controller.ui.map.cached_points.clone();
        let filtered_points = map_clusters::filter_points(
            &points,
            self.controller.ui.map.cluster_overlay,
            self.controller.ui.map.cluster_hide_noise,
            self.controller.ui.map.cluster_filter,
        );
        let cluster_overlay = self.controller.ui.map.cluster_overlay;
        let similarity_blend = self.controller.ui.map.similarity_blend;
        let blend_threshold = self.controller.ui.map.similarity_blend_threshold;
        let blend_enabled = cluster_overlay && similarity_blend;
        let centroids = if blend_enabled {
            Some(map_clusters::cluster_centroids(&points))
        } else {
            None
        };
        let map_diagonal =
            ((bounds.max_x - bounds.min_x).powi(2) + (bounds.max_y - bounds.min_y).powi(2)).sqrt();
        let point_color = |point: &crate::egui_app::state::MapPoint, alpha: u8| {
            if cluster_overlay {
                if let Some(centroids) = centroids.as_ref() {
                    map_clusters::blended_cluster_color(
                        point,
                        centroids,
                        &palette,
                        alpha,
                        map_diagonal,
                        blend_threshold,
                    )
                } else {
                    point
                        .cluster_id
                        .map(|id| map_clusters::cluster_color(id, &palette, alpha))
                        .unwrap_or(palette.accent_mint)
                }
            } else {
                palette.accent_mint
            }
        };
        let painter = ui.painter_at(rect);
        let hovered = map_interactions::find_hover_point(
            &filtered_points,
            rect,
            center,
            scale,
            self.controller.ui.map.pan,
            pointer,
        );
        self.controller.ui.map.hovered_sample_id = hovered.as_ref().map(|(point, _)| point.sample_id.clone());

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
                        if cluster_id < 0 {
                            ui.label("Cluster: noise");
                        } else {
                            ui.label(format!("Cluster: {cluster_id}"));
                        }
                    }
                }
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
        });

        let mut draw_calls = 0usize;
        let mut points_rendered = 0usize;
        if filtered_points.len() > 8000 || self.controller.ui.map.zoom < 0.6 {
            if self.controller.ui.map.cluster_overlay {
                draw_calls = map_render::render_heatmap_with_color(
                    &painter,
                    rect,
                    &filtered_points,
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
                    &filtered_points,
                    center,
                    scale,
                    self.controller.ui.map.pan,
                    MAP_HEATMAP_BINS,
                );
            }
            points_rendered = filtered_points.len();
            self.controller.ui.map.last_render_mode = crate::egui_app::state::MapRenderMode::Heatmap;
        } else {
            for point in filtered_points {
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
                    let radius = if self.controller.ui.map.selected_sample_id.as_deref()
                        == Some(point.sample_id.as_str())
                    {
                        3.5
                    } else {
                        2.0
                    };
                    let color = point_color(&point, 200);
                    painter.circle_filled(pos, radius, color);
                    draw_calls += 1;
                }
            }
            self.controller.ui.map.last_render_mode = crate::egui_app::state::MapRenderMode::Points;
        }
        self.controller.ui.map.last_render_ms = render_started.elapsed().as_secs_f32() * 1000.0;
        self.controller.ui.map.last_draw_calls = draw_calls;
        self.controller.ui.map.last_points_rendered = points_rendered;
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
