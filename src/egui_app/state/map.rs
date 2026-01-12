use eframe::egui::{Pos2, Vec2};
use std::collections::HashMap;
use std::sync::Arc;

/// UI state for the map view and its caches.
#[derive(Clone, Debug)]
pub struct MapUiState {
    /// Whether the map panel is open.
    pub open: bool,
    /// Current pan offset in screen space.
    pub pan: Vec2,
    /// Current zoom level.
    pub zoom: f32,
    /// Last drag position for panning.
    pub last_drag_pos: Option<Pos2>,
    /// Cached data bounds for the current layout.
    pub bounds: Option<MapBounds>,
    /// Bounds used for the most recent query.
    pub last_query: Option<MapQueryBounds>,
    /// Cached points for the current source/layout.
    pub cached_points: Vec<MapPoint>,
    /// Revision counter for cached points.
    pub cached_points_revision: u64,
    /// Cached filter key for filtered points.
    pub cached_filtered_key: Option<MapFilterKey>,
    /// Filtered points cache.
    pub cached_filtered_points: Vec<MapPoint>,
    /// Cache key for cluster centroid lookups.
    pub cached_cluster_centroids_key: Option<String>,
    /// Cached cluster centroids.
    pub cached_cluster_centroids: Option<Arc<HashMap<i32, MapClusterCentroid>>>,
    /// Key indicating a pending auto cluster build.
    pub auto_cluster_build_requested_key: Option<String>,
    /// Sample id currently hovered in the map.
    pub hovered_sample_id: Option<String>,
    /// Anchor sample id for similarity highlighting.
    pub similarity_anchor_sample_id: Option<String>,
    /// Anchor point for similarity highlighting.
    pub similarity_anchor_point: Option<(f32, f32)>,
    /// Sample id currently selected in the map.
    pub selected_sample_id: Option<String>,
    /// Active hover target for paint operations.
    pub paint_hover_active_id: Option<String>,
    /// Selected UMAP version string.
    pub umap_version: String,
    /// Whether to overlay cluster information.
    pub cluster_overlay: bool,
    /// Whether to hide noise clusters.
    pub cluster_hide_noise: bool,
    /// Raw text input for cluster filter.
    pub cluster_filter_input: String,
    /// Parsed cluster filter value.
    pub cluster_filter: Option<i32>,
    /// Whether similarity blending is enabled.
    pub similarity_blend: bool,
    /// Threshold for similarity blending.
    pub similarity_blend_threshold: f32,
    /// Whether to focus the selected point.
    pub focus_selected_requested: bool,
    /// Last render duration in milliseconds.
    pub last_render_ms: f32,
    /// Last render draw call count.
    pub last_draw_calls: usize,
    /// Number of points rendered last frame.
    pub last_points_rendered: usize,
    /// Last render mode used.
    pub last_render_mode: MapRenderMode,
    /// Whether the map data is out of date.
    pub outdated: bool,
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
            outdated: false,
        }
    }
}

/// Bounds covering all points in a layout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapBounds {
    /// Minimum X coordinate.
    pub min_x: f32,
    /// Maximum X coordinate.
    pub max_x: f32,
    /// Minimum Y coordinate.
    pub min_y: f32,
    /// Maximum Y coordinate.
    pub max_y: f32,
}

/// Bounds used to query visible points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapQueryBounds {
    /// Minimum X coordinate for query.
    pub min_x: f32,
    /// Maximum X coordinate for query.
    pub max_x: f32,
    /// Minimum Y coordinate for query.
    pub min_y: f32,
    /// Maximum Y coordinate for query.
    pub max_y: f32,
}

/// Single point in a UMAP layout.
#[derive(Clone, Debug, PartialEq)]
pub struct MapPoint {
    /// Sample id for this point.
    pub sample_id: String,
    /// X coordinate in layout space.
    pub x: f32,
    /// Y coordinate in layout space.
    pub y: f32,
    /// Optional cluster id.
    pub cluster_id: Option<i32>,
}

/// Cluster centroid summary.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapClusterCentroid {
    /// X coordinate of the centroid.
    pub x: f32,
    /// Y coordinate of the centroid.
    pub y: f32,
    /// Number of points in the cluster.
    pub count: usize,
}

/// Cache key for filtered map points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapFilterKey {
    /// Revision of the points cache.
    pub points_revision: u64,
    /// Whether cluster overlay is enabled.
    pub overlay: bool,
    /// Optional cluster filter.
    pub filter: Option<i32>,
}

/// Render mode for the map view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapRenderMode {
    /// Render a density heatmap.
    Heatmap,
    /// Render individual points.
    Points,
}
