mod geometry;
mod render;

use crate::egui_app::state::WaveformView;
use crate::selection::{SelectionEdge, SelectionRange};
use eframe::egui::{self, Color32};

use crate::egui_app::ui::style;

struct SliceOverlayEnv<'a> {
    rect: egui::Rect,
    view: WaveformView,
    view_width: f32,
    pointer_pos: Option<egui::Pos2>,
    palette: &'a style::Palette,
    slice_color: Color32,
}

#[derive(Clone, Copy)]
struct SliceItem {
    range: SelectionRange,
    index: usize,
}

struct SliceEdgeSpec {
    edge: SelectionEdge,
    edge_rect: egui::Rect,
    edge_id: &'static str,
    slice_rect: egui::Rect,
    index: usize,
}

#[derive(Clone, Copy, Default)]
pub(super) struct SliceOverlayResult {
    pub dragging: bool,
    pub consumed_click: bool,
}

pub(super) fn render_slice_overlays(
    app: &mut EguiApp,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    palette: &style::Palette,
    view: WaveformView,
    view_width: f32,
    pointer_pos: Option<egui::Pos2>,
) -> SliceOverlayResult {
    render::render_slice_overlays(
        app,
        ui,
        rect,
        palette,
        view,
        view_width,
        pointer_pos,
    )
}
