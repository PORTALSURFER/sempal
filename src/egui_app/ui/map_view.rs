use super::map_clusters;
use super::map_interactions;
use super::map_math;
use super::map_render;
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
                let refresh = self.render_map_controls(ui);
                if refresh {
                    self.controller.ui.map.last_query = None;
                }
                ui.separator();
                self.render_map_canvas(ui);
            });
    }

    fn render_map_controls(&mut self, ui: &mut egui::Ui) -> bool {
        let mut refresh = false;
        ui.horizontal(|ui| {
            refresh |= ui
                .checkbox(&mut self.controller.ui.map.cluster_overlay, "Clusters")
                .changed();
            if self.controller.ui.map.cluster_overlay {
                let mut method = self.controller.ui.map.cluster_method;
                egui::ComboBox::from_id_source("cluster_method")
                    .selected_text(method.label())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut method,
                            crate::egui_app::state::MapClusterMethod::Umap,
                            "UMAP",
                        );
                        ui.selectable_value(
                            &mut method,
                            crate::egui_app::state::MapClusterMethod::Embedding,
                            "Embedding",
                        );
                    });
                if method != self.controller.ui.map.cluster_method {
                    self.controller.ui.map.cluster_method = method;
                    refresh = true;
                }
                ui.checkbox(&mut self.controller.ui.map.cluster_hide_noise, "Hide noise");
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
        if self.controller.ui.map.cluster_overlay {
            if let Some(stats) =
                map_clusters::compute_cluster_stats(&self.controller.ui.map.cached_points)
            {
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "Clusters: {}",
                        stats.cluster_count
                    ));
                    ui.label(format!(
                        "Noise: {:.1}%",
                        stats.noise_ratio * 100.0
                    ));
                    ui.label(format!(
                        "Size min/max: {}/{}",
                        stats.min_cluster_size, stats.max_cluster_size
                    ));
                });
            }
        }
        refresh
    }

    fn render_map_canvas(&mut self, ui: &mut egui::Ui) {
        let palette = style::palette();
        let available = ui.available_size();
        let (rect, response) = ui.allocate_exact_size(available, egui::Sense::drag());
        let model_id = crate::analysis::embedding::EMBEDDING_MODEL_ID;
        let umap_version = self.controller.ui.map.umap_version.clone();
        let cluster_method = self.controller.ui.map.cluster_method;
        let cluster_method_str = cluster_method.as_str();
        let cluster_umap_version = if cluster_method == crate::egui_app::state::MapClusterMethod::Umap {
            umap_version.as_str()
        } else {
            ""
        };

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
        let world_bounds =
            map_math::world_bounds_from_view(rect, center, scale, self.controller.ui.map.pan);
        if map_math::should_requery(&self.controller.ui.map.last_query, &world_bounds) {
            match self.controller.umap_points_in_bounds(
                model_id,
                &umap_version,
                cluster_method_str,
                cluster_umap_version,
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
        let points = map_clusters::filter_points(
            &points,
            self.controller.ui.map.cluster_overlay,
            self.controller.ui.map.cluster_hide_noise,
            self.controller.ui.map.cluster_filter,
        );
        let painter = ui.painter_at(rect);
        let hovered = map_interactions::find_hover_point(
            &points,
            rect,
            center,
            scale,
            self.controller.ui.map.pan,
            pointer,
        );
        self.controller.ui.map.hovered_sample_id = hovered.as_ref().map(|(point, _)| point.sample_id.clone());

        if let Some((point, pos)) = hovered.as_ref() {
            let stroke_color = if self.controller.ui.map.cluster_overlay {
                point
                    .cluster_id
                    .map(|id| map_clusters::cluster_color(id, &palette, 200))
                    .unwrap_or(palette.accent_mint)
            } else {
                palette.accent_mint
            };
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
            map_render::render_heatmap(
                &painter,
                rect,
                &points,
                center,
                scale,
                self.controller.ui.map.pan,
                MAP_HEATMAP_BINS,
            );
        } else {
            for point in points {
                let pos = map_render::map_to_screen(
                    point.x,
                    point.y,
                    rect,
                    center,
                    scale,
                    self.controller.ui.map.pan,
                );
                if rect.contains(pos) {
                    let radius = if self.controller.ui.map.selected_sample_id.as_deref()
                        == Some(point.sample_id.as_str())
                    {
                        3.5
                    } else {
                        2.0
                    };
                    let color = if self.controller.ui.map.cluster_overlay {
                        point
                            .cluster_id
                            .map(|id| map_clusters::cluster_color(id, &palette, 200))
                            .unwrap_or(palette.accent_mint)
                    } else {
                        palette.accent_mint
                    };
                    painter.circle_filled(pos, radius, color);
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

fn sample_label_from_id(sample_id: &str) -> String {
    if let Some((_, rel)) = sample_id.split_once("::") {
        let path = std::path::Path::new(rel);
        return view_model::sample_display_label(path);
    }
    sample_id.to_string()
}
