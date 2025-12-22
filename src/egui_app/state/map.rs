use eframe::egui::{Pos2, Vec2};

#[derive(Clone, Debug)]
pub struct MapUiState {
    pub open: bool,
    pub pan: Vec2,
    pub zoom: f32,
    pub last_drag_pos: Option<Pos2>,
    pub bounds: Option<MapBounds>,
    pub last_query: Option<MapQueryBounds>,
    pub cached_points: Vec<MapPoint>,
    pub hovered_sample_id: Option<String>,
    pub selected_sample_id: Option<String>,
    pub umap_version: String,
    pub cluster_overlay: bool,
    pub cluster_hide_noise: bool,
    pub cluster_method: MapClusterMethod,
    pub cluster_filter_input: String,
    pub cluster_filter: Option<i32>,
    pub similarity_blend: bool,
    pub similarity_blend_threshold: f32,
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
            hovered_sample_id: None,
            selected_sample_id: None,
            umap_version: "v1".to_string(),
            cluster_overlay: false,
            cluster_hide_noise: false,
            cluster_method: MapClusterMethod::Umap,
            cluster_filter_input: String::new(),
            cluster_filter: None,
            similarity_blend: false,
            similarity_blend_threshold: 0.12,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapRenderMode {
    Heatmap,
    Points,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapClusterMethod {
    Embedding,
    Umap,
}

impl MapClusterMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            MapClusterMethod::Embedding => "embedding",
            MapClusterMethod::Umap => "umap",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MapClusterMethod::Embedding => "Embedding",
            MapClusterMethod::Umap => "UMAP",
        }
    }
}
