use eframe::egui::{Pos2, Vec2};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct MapUiState {
    pub open: bool,
    pub pan: Vec2,
    pub zoom: f32,
    pub last_drag_pos: Option<Pos2>,
    pub bounds: Option<MapBounds>,
    pub last_query: Option<MapQueryBounds>,
    pub cached_points: Vec<MapPoint>,
    pub cached_points_revision: u64,
    pub cached_filtered_key: Option<MapFilterKey>,
    pub cached_filtered_points: Vec<MapPoint>,
    pub cached_cluster_centroids_key: Option<String>,
    pub cached_cluster_centroids: Option<Arc<HashMap<i32, MapClusterCentroid>>>,
    pub auto_cluster_build_requested_key: Option<String>,
    pub hovered_sample_id: Option<String>,
    pub similarity_anchor_sample_id: Option<String>,
    pub similarity_anchor_point: Option<(f32, f32)>,
    pub selected_sample_id: Option<String>,
    pub paint_hover_active_id: Option<String>,
    pub umap_version: String,
    pub cluster_overlay: bool,
    pub cluster_hide_noise: bool,
    pub cluster_filter_input: String,
    pub cluster_filter: Option<i32>,
    pub similarity_blend: bool,
    pub similarity_blend_threshold: f32,
    pub focus_selected_requested: bool,
    pub last_render_ms: f32,
    pub last_draw_calls: usize,
    pub last_points_rendered: usize,
    pub last_render_mode: MapRenderMode,
}

impl Default for MapUiState {
    fn default() -> Self {
        Self {
            open: false,
            pan: Vec2::ZERO,
            zoom: 1.0,
            last_drag_pos: None,
            bounds: None,
            last_query: None,
            cached_points: Vec::new(),
            cached_points_revision: 0,
            cached_filtered_key: None,
            cached_filtered_points: Vec::new(),
            cached_cluster_centroids_key: None,
            cached_cluster_centroids: None,
            auto_cluster_build_requested_key: None,
            hovered_sample_id: None,
            similarity_anchor_sample_id: None,
            similarity_anchor_point: None,
            selected_sample_id: None,
            paint_hover_active_id: None,
            umap_version: "v1".to_string(),
            cluster_overlay: true,
            cluster_hide_noise: true,
            cluster_filter_input: String::new(),
            cluster_filter: None,
            similarity_blend: true,
            similarity_blend_threshold: 0.2,
            focus_selected_requested: false,
            last_render_ms: 0.0,
            last_draw_calls: 0,
            last_points_rendered: 0,
            last_render_mode: MapRenderMode::Points,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapBounds {
    pub min_x: f32,
    pub max_x: f32,
    pub min_y: f32,
    pub max_y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapQueryBounds {
    pub min_x: f32,
    pub max_x: f32,
    pub min_y: f32,
    pub max_y: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MapPoint {
    pub sample_id: String,
    pub x: f32,
    pub y: f32,
    pub cluster_id: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapClusterCentroid {
    pub x: f32,
    pub y: f32,
    pub count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapFilterKey {
    pub points_revision: u64,
    pub overlay: bool,
    pub filter: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapRenderMode {
    Heatmap,
    Points,
}
