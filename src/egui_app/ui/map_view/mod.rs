mod map_input;
pub(super) mod map_render;
mod map_state;

use super::map_clusters;
use super::map_empty;
use super::map_math;
use super::style;
use super::*;
use eframe::egui;
use std::time::Instant;

const MAP_POINT_LIMIT: usize = 50_000;
const MAP_HEATMAP_BINS: usize = 64;
const MAP_ZOOM_MIN: f32 = 0.2;
const MAP_ZOOM_MAX: f32 = 20.0;
const MAP_ZOOM_SPEED: f32 = 0.0015;

impl EguiApp {
    pub(super) fn render_map_panel(&mut self, ui: &mut egui::Ui) {
        let refresh = map_state::render_map_controls(self, ui);
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
        map_state::sync_selected_sample(self);

        let Some(bounds) =
            map_state::ensure_bounds(self, model_id, &umap_version, source_id.as_ref())
        else {
            let prep_active = self.controller.similarity_prep_in_progress();
            if !prep_active {
                self.controller.prepare_similarity_for_selected_source();
            }
            let busy = prep_active || self.controller.ui.progress.visible;
            if map_empty::render_empty_state(ui, rect, &palette, busy) {
                self.controller.prepare_similarity_for_selected_source();
            }
            return;
        };

        map_input::handle_zoom(self, ui, &response);
        let pointer = response.interact_pointer_pos();
        map_input::handle_pan(self, &response, pointer);

        let scale = map_state::map_scale(rect, bounds, self.controller.ui.map.zoom);
        let center = egui::pos2(
            (bounds.min_x + bounds.max_x) * 0.5,
            (bounds.min_y + bounds.max_y) * 0.5,
        );
        map_input::handle_focus_request(self, model_id, &umap_version, bounds, center, scale);

        let world_bounds =
            map_math::world_bounds_from_view(rect, center, scale, self.controller.ui.map.pan);
        map_state::update_points_cache(
            self,
            model_id,
            &umap_version,
            cluster_method_str,
            cluster_umap_version,
            source_id.as_ref(),
            world_bounds,
            MAP_POINT_LIMIT,
        );
        map_state::update_filtered_points(self);

        let cluster_overlay = self.controller.ui.map.cluster_overlay;
        let similarity_blend = self.controller.ui.map.similarity_blend;
        let blend_threshold = self.controller.ui.map.similarity_blend_threshold;
        let centroids_arc = map_state::prepare_cluster_centroids(
            self,
            model_id,
            &umap_version,
            cluster_method_str,
            cluster_umap_version,
            source_id.as_ref(),
        );
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

        let painter = ui.painter_at(rect);
        let hovered = map_input::resolve_hover(
            self,
            rect,
            center,
            scale,
            self.controller.ui.map.pan,
            pointer,
        );
        if response.dragged_by(egui::PointerButton::Primary) {
            map_input::handle_paint_hover(self, ui, hovered.as_ref());
        }

        if let Some((point, pos)) = hovered.as_ref() {
            let stroke_color = point_color(point, 200);
            painter.circle_stroke(*pos, 4.0, egui::Stroke::new(1.5, stroke_color));
        }

        if response.clicked() {
            map_input::handle_click(self, hovered.as_ref());
        }

        map_input::handle_context_menu(self, ui, &response, hovered.as_ref());

        let focused_sample_id = self.controller.ui.map.selected_sample_id.as_deref();
        let (draw_calls, points_rendered, render_mode) = map_render::render_points(
            &painter,
            rect,
            &self.controller.ui.map.cached_filtered_points,
            center,
            scale,
            self.controller.ui.map.pan,
            self.controller.ui.map.zoom,
            focused_sample_id,
            cluster_overlay,
            MAP_HEATMAP_BINS,
            point_color,
        );
        self.controller.ui.map.last_render_mode = render_mode;

        let focused_pos = focused_sample_id.and_then(|id| {
            let display_points = &self.controller.ui.map.cached_filtered_points;
            display_points
                .iter()
                .find(|point| point.sample_id == id)
                .map(|point| {
                    map_render::map_to_screen(
                        point.x,
                        point.y,
                        rect,
                        center,
                        scale,
                        self.controller.ui.map.pan,
                    )
                })
        });
        if let Some(pos) = focused_pos {
            if rect.contains(pos) {
                painter.circle_stroke(pos, 6.0, style::focused_row_stroke());
            }
        }
        self.controller.ui.map.last_render_ms = render_started.elapsed().as_secs_f32() * 1000.0;
        self.controller.ui.map.last_draw_calls = draw_calls;
        self.controller.ui.map.last_points_rendered = points_rendered;
    }
}
