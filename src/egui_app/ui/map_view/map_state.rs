use super::EguiApp;
use super::map_clusters;
use super::map_math;
use super::style;
use crate::egui_app::state::{MapBounds, MapFilterKey, MapQueryBounds};
use crate::egui_app::view_model;
use crate::sample_sources::SourceId;
use eframe::egui;
use std::sync::Arc;

pub(super) fn render_map_controls(app: &mut EguiApp, ui: &mut egui::Ui) -> bool {
    let refresh = false;
    app.controller.ui.map.cluster_overlay = true;
    app.controller.ui.map.similarity_blend = true;
    app.controller.ui.map.similarity_blend_threshold = 0.2;
    app.controller.ui.map.cluster_filter_input.clear();
    app.controller.ui.map.cluster_filter = None;
    ui.horizontal(|ui| {
        let mode = match app.controller.ui.map.last_render_mode {
            crate::egui_app::state::MapRenderMode::Heatmap => "heatmap",
            crate::egui_app::state::MapRenderMode::Points => "points",
        };
        ui.label(format!(
            "Frame {:.2} ms | draw {} | points {} | {}",
            app.controller.ui.map.last_render_ms,
            app.controller.ui.map.last_draw_calls,
            app.controller.ui.map.last_points_rendered,
            mode
        ));
    });
    if app.controller.ui.map.cluster_overlay {
        if let Some(stats) =
            map_clusters::compute_cluster_stats(&app.controller.ui.map.cached_points)
        {
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

pub(super) fn ensure_bounds(
    app: &mut EguiApp,
    model_id: &str,
    umap_version: &str,
    source_id: Option<&SourceId>,
) -> Option<MapBounds> {
    if app.controller.ui.map.bounds.is_none() {
        match app
            .controller
            .umap_bounds(model_id, umap_version, source_id)
        {
            Ok(bounds) => {
                app.controller.ui.map.bounds = bounds.map(|b| crate::egui_app::state::MapBounds {
                    min_x: b.min_x,
                    max_x: b.max_x,
                    min_y: b.min_y,
                    max_y: b.max_y,
                });
            }
            Err(err) => {
                app.controller.set_status(
                    format!("t-SNE bounds failed: {err}"),
                    style::StatusTone::Error,
                );
            }
        }
    }
    app.controller.ui.map.bounds
}

pub(super) fn sync_selected_sample(app: &mut EguiApp) {
    let focused_sample_id = app.controller.selected_sample_id();
    if app.controller.ui.map.selected_sample_id != focused_sample_id {
        app.controller.ui.map.selected_sample_id = focused_sample_id;
    }
}

pub(super) fn update_points_cache(
    app: &mut EguiApp,
    model_id: &str,
    umap_version: &str,
    cluster_method_str: &str,
    cluster_umap_version: &str,
    source_id: Option<&SourceId>,
    world_bounds: MapQueryBounds,
    point_limit: usize,
) {
    if map_math::should_requery(&app.controller.ui.map.last_query, &world_bounds) {
        match app.controller.umap_points_in_bounds(
            model_id,
            umap_version,
            cluster_method_str,
            cluster_umap_version,
            source_id,
            world_bounds,
            point_limit,
        ) {
            Ok(points) => {
                app.controller.ui.map.cached_points = points
                    .into_iter()
                    .map(|p| crate::egui_app::state::MapPoint {
                        sample_id: p.sample_id,
                        x: p.x,
                        y: p.y,
                        cluster_id: p.cluster_id,
                    })
                    .collect();
                app.controller.ui.map.cached_points_revision =
                    app.controller.ui.map.cached_points_revision.wrapping_add(1);
                app.controller.ui.map.cached_filtered_key = None;
                app.controller.ui.map.cached_filtered_points.clear();
                app.controller.ui.map.last_query = Some(world_bounds);
            }
            Err(err) => {
                app.controller.set_status(
                    format!("t-SNE query failed: {err}"),
                    style::StatusTone::Error,
                );
            }
        }
    }
}

pub(super) fn update_filtered_points(app: &mut EguiApp) {
    let filter_key = MapFilterKey {
        points_revision: app.controller.ui.map.cached_points_revision,
        overlay: app.controller.ui.map.cluster_overlay,
        filter: app.controller.ui.map.cluster_filter,
    };
    if app.controller.ui.map.cached_filtered_key != Some(filter_key) {
        let points = &app.controller.ui.map.cached_points;
        app.controller.ui.map.cached_filtered_points = map_clusters::filter_points(
            points,
            app.controller.ui.map.cluster_overlay,
            app.controller.ui.map.cluster_filter,
        );
        app.controller.ui.map.cached_filtered_key = Some(filter_key);
    }
}

pub(super) fn prepare_cluster_centroids(
    app: &mut EguiApp,
    model_id: &str,
    umap_version: &str,
    cluster_method_str: &str,
    cluster_umap_version: &str,
    source_id: Option<&SourceId>,
) -> Option<Arc<std::collections::HashMap<i32, crate::egui_app::state::MapClusterCentroid>>> {
    let cluster_overlay = app.controller.ui.map.cluster_overlay;
    if !cluster_overlay {
        return None;
    }
    let source_key = source_id.map(|id| id.as_str().to_string());
    let centroids_key = format!(
        "{}|{}|{}|{}",
        umap_version,
        source_key.as_deref().unwrap_or(""),
        cluster_method_str,
        cluster_umap_version
    );
    if app
        .controller
        .ui
        .map
        .cached_cluster_centroids_key
        .as_deref()
        != Some(&centroids_key)
    {
        app.controller.ui.map.cached_cluster_centroids_key = Some(centroids_key);
        app.controller.ui.map.cached_cluster_centroids = None;
        app.controller.ui.map.auto_cluster_build_requested_key = None;
    }
    if app.controller.ui.map.cached_cluster_centroids.is_none() {
        match app.controller.umap_cluster_centroids(
            model_id,
            umap_version,
            cluster_method_str,
            cluster_umap_version,
            source_id,
        ) {
            Ok(centroids) => {
                app.controller.ui.map.cached_cluster_centroids = Some(Arc::new(centroids));
            }
            Err(err) => {
                app.controller.set_status(
                    format!("Cluster centroids query failed: {err}"),
                    style::StatusTone::Error,
                );
            }
        }
    }
    let (has_any_points, has_missing_cluster_ids) = {
        let points = &app.controller.ui.map.cached_points;
        (
            !points.is_empty(),
            points.iter().any(|point| point.cluster_id.is_none()),
        )
    };
    let centroids_empty = app
        .controller
        .ui
        .map
        .cached_cluster_centroids
        .as_ref()
        .is_some_and(|centroids| centroids.is_empty());
    if has_any_points
        && (has_missing_cluster_ids || centroids_empty)
        && app
            .controller
            .ui
            .map
            .auto_cluster_build_requested_key
            .is_none()
    {
        app.controller.ui.map.auto_cluster_build_requested_key =
            app.controller.ui.map.cached_cluster_centroids_key.clone();
        let umap_version = umap_version.to_string();
        app.controller.build_umap_clusters(
            crate::analysis::embedding::EMBEDDING_MODEL_ID,
            &umap_version,
        );
    }
    app.controller
        .ui
        .map
        .cached_cluster_centroids
        .clone()
        .filter(|centroids| !centroids.is_empty())
        .or_else(|| {
            let points = &app.controller.ui.map.cached_points;
            Some(Arc::new(map_clusters::cluster_centroids(points)))
        })
}

pub(super) fn map_scale(rect: egui::Rect, bounds: MapBounds, zoom: f32) -> f32 {
    let world_w = (bounds.max_x - bounds.min_x).max(1e-3);
    let world_h = (bounds.max_y - bounds.min_y).max(1e-3);
    let scale_x = rect.width() / world_w;
    let scale_y = rect.height() / world_h;
    let base = scale_x.min(scale_y) * 0.9;
    base * zoom
}

pub(super) fn sample_label_from_id(sample_id: &str) -> String {
    if let Some((_, rel)) = sample_id.split_once("::") {
        let path = std::path::Path::new(rel);
        return view_model::sample_display_label(path);
    }
    sample_id.to_string()
}
