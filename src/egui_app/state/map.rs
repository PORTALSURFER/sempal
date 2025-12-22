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
}
