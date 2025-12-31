use super::selection_geometry::{
    paint_selection_edge_bracket, selection_edge_handle_rect, selection_handle_height,
    selection_handle_rect,
};
use super::style;
use super::*;
use super::super::{SliceDragKind, SliceDragState};
use crate::egui_app::state::WaveformView;
use crate::selection::{SelectionEdge, SelectionRange};
use eframe::egui::{self, Color32, CursorIcon};

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

pub(super) fn render_slice_overlays(
    app: &mut EguiApp,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    palette: &style::Palette,
    view: WaveformView,
    view_width: f32,
    pointer_pos: Option<egui::Pos2>,
) -> bool {
    if app.controller.ui.waveform.slices.is_empty() {
        app.slice_drag = None;
        return false;
    }
    let slice_color = palette.accent_ice;
    let env = SliceOverlayEnv {
        rect,
        view,
        view_width,
        pointer_pos,
        palette,
        slice_color,
    };
    let mut dragging = app.slice_drag.is_some();
    let slices: Vec<SliceItem> = app
        .controller
        .ui
        .waveform
        .slices
        .iter()
        .copied()
        .enumerate()
        .map(|(index, range)| SliceItem { range, index })
        .collect();
    for item in slices {
        dragging |= render_slice_overlay(app, ui, &env, item);
    }

    sync_slice_drag_release(app, ui.ctx());
    dragging
}

fn render_slice_overlay(
    app: &mut EguiApp,
    ui: &mut egui::Ui,
    env: &SliceOverlayEnv<'_>,
    item: SliceItem,
) -> bool {
    let Some(slice_rect) = slice_rect(env, item.range) else {
        return false;
    };
    let handle_rect = selection_handle_rect(slice_rect);
    let handle_response = ui.interact(
        handle_rect,
        ui.id().with(("slice_handle", item.index)),
        egui::Sense::click_and_drag(),
    );
    paint_slice(
        ui,
        slice_rect,
        handle_rect,
        env.slice_color,
        handle_response.hovered(),
    );
    let mut dragging = render_slice_handle(app, ui, env, item, &handle_response);
    dragging |= render_slice_edges(app, ui, env, slice_rect, item.index);
    draw_slice_bar(ui, slice_rect, env);
    dragging
}

fn render_slice_handle(
    app: &mut EguiApp,
    ui: &mut egui::Ui,
    env: &SliceOverlayEnv<'_>,
    item: SliceItem,
    handle_response: &egui::Response,
) -> bool {
    handle_slice_move_drag(app, env, item, handle_response);
    if handle_response.dragged() {
        ui.output_mut(|o| o.cursor_icon = CursorIcon::Grabbing);
        return true;
    }
    if handle_response.hovered() {
        ui.output_mut(|o| o.cursor_icon = CursorIcon::Grab);
    }
    false
}

fn render_slice_edges(
    app: &mut EguiApp,
    ui: &mut egui::Ui,
    env: &SliceOverlayEnv<'_>,
    slice_rect: egui::Rect,
    index: usize,
) -> bool {
    let start_edge_rect = selection_edge_handle_rect(slice_rect, SelectionEdge::Start);
    let end_edge_rect = selection_edge_handle_rect(slice_rect, SelectionEdge::End);
    let mut dragging = false;
    for (edge, edge_rect, edge_id) in [
        (SelectionEdge::Start, start_edge_rect, "slice_edge_start"),
        (SelectionEdge::End, end_edge_rect, "slice_edge_end"),
    ] {
        let spec = SliceEdgeSpec {
            edge,
            edge_rect,
            edge_id,
            slice_rect,
            index,
        };
        dragging |= render_slice_edge(app, ui, env, spec);
    }
    dragging
}

fn render_slice_edge(app: &mut EguiApp, ui: &mut egui::Ui, env: &SliceOverlayEnv<'_>, spec: SliceEdgeSpec) -> bool {
    let edge_response = ui.interact(
        spec.edge_rect,
        ui.id().with((spec.edge_id, spec.index)),
        egui::Sense::click_and_drag(),
    );
    handle_slice_edge_drag(app, env, &spec, &edge_response);
    apply_edge_hover(ui, env, spec.edge_rect, spec.edge, &edge_response);
    edge_response.dragged()
}

fn slice_rect(env: &SliceOverlayEnv<'_>, slice: SelectionRange) -> Option<egui::Rect> {
    let start_norm = ((slice.start() - env.view.start) / env.view_width).clamp(0.0, 1.0);
    let end_norm = ((slice.end() - env.view.start) / env.view_width).clamp(0.0, 1.0);
    let width = env.rect.width() * (end_norm - start_norm).max(0.0);
    if width <= 0.0 {
        return None;
    }
    let x = env.rect.left() + env.rect.width() * start_norm;
    Some(egui::Rect::from_min_size(
        egui::pos2(x, env.rect.top()),
        egui::vec2(width, env.rect.height()),
    ))
}

fn paint_slice(
    ui: &egui::Ui,
    slice_rect: egui::Rect,
    handle_rect: egui::Rect,
    color: Color32,
    hovered: bool,
) {
    let fill_alpha = if hovered { 80 } else { 60 };
    let handle_alpha = if hovered { 215 } else { 180 };
    let painter = ui.painter();
    painter.rect_filled(slice_rect, 0.0, style::with_alpha(color, fill_alpha));
    painter.rect_filled(handle_rect, 0.0, style::with_alpha(color, handle_alpha));
}

fn draw_slice_bar(ui: &egui::Ui, slice_rect: egui::Rect, env: &SliceOverlayEnv<'_>) {
    let bar_height = selection_handle_height(slice_rect);
    let bar_rect = egui::Rect::from_min_size(
        egui::pos2(slice_rect.left(), slice_rect.bottom() - bar_height),
        egui::vec2(slice_rect.width(), bar_height),
    );
    let accent = style::with_alpha(env.slice_color, 90);
    ui.painter().rect_filled(bar_rect, 0.0, accent);
    ui.painter().rect_stroke(
        bar_rect,
        0.0,
        egui::Stroke::new(1.0, style::with_alpha(env.palette.bg_secondary, 180)),
        egui::StrokeKind::Inside,
    );
}

fn handle_slice_move_drag(
    app: &mut EguiApp,
    env: &SliceOverlayEnv<'_>,
    item: SliceItem,
    handle_response: &egui::Response,
) {
    if handle_response.drag_started() {
        start_slice_move_drag(app, env, item, handle_response);
        return;
    } else if handle_response.dragged() {
        update_slice_move_drag(app, env, item.index, handle_response);
        return;
    } else if handle_response.drag_stopped() {
        finish_slice_drag(app, item.index);
    }
}

fn start_slice_move_drag(
    app: &mut EguiApp,
    env: &SliceOverlayEnv<'_>,
    item: SliceItem,
    handle_response: &egui::Response,
) {
    let Some(pos) = handle_response.interact_pointer_pos() else {
        return;
    };
    let anchor = to_wave_pos(env, pos);
    app.slice_drag = Some(SliceDragState {
        index: item.index,
        kind: SliceDragKind::Move {
            anchor,
            range: item.range,
        },
    });
}

fn update_slice_move_drag(
    app: &mut EguiApp,
    env: &SliceOverlayEnv<'_>,
    index: usize,
    handle_response: &egui::Response,
) {
    let Some(pos) = handle_response.interact_pointer_pos() else {
        return;
    };
    let cursor = to_wave_pos(env, pos);
    if let Some(SliceDragState {
        index: active_index,
        kind: SliceDragKind::Move { anchor, range },
    }) = app.slice_drag
        && active_index == index
    {
        let delta = cursor - anchor;
        let shifted = range.shift(delta);
        if let Some(slot) = app.controller.ui.waveform.slices.get_mut(active_index) {
            *slot = shifted;
        }
    }
}

fn handle_slice_edge_drag(
    app: &mut EguiApp,
    env: &SliceOverlayEnv<'_>,
    spec: &SliceEdgeSpec,
    edge_response: &egui::Response,
) {
    let pointer_down = edge_response.is_pointer_button_down_on();
    if edge_response.drag_started() || (pointer_down && app.slice_drag.is_none()) {
        start_slice_edge_drag(app, spec, edge_response);
        return;
    }
    if (pointer_down || edge_response.dragged())
        && let Some(pos) = edge_response.interact_pointer_pos()
    {
        update_slice_edge_drag(app, env, spec.index, pos);
    }
    if edge_response.drag_stopped() {
        finish_slice_drag(app, spec.index);
    }
}

fn start_slice_edge_drag(
    app: &mut EguiApp,
    spec: &SliceEdgeSpec,
    edge_response: &egui::Response,
) {
    let offset = edge_response
        .interact_pointer_pos()
        .map(|pos| pos.x - edge_position_px(spec.edge, spec.slice_rect))
        .unwrap_or(0.0);
    app.slice_drag = Some(SliceDragState {
        index: spec.index,
        kind: SliceDragKind::Edge {
            edge: spec.edge,
            offset,
        },
    });
}

fn update_slice_edge_drag(
    app: &mut EguiApp,
    env: &SliceOverlayEnv<'_>,
    index: usize,
    pos: egui::Pos2,
) {
    if let Some(SliceDragState {
        index: active_index,
        kind: SliceDragKind::Edge { edge, offset },
    }) = app.slice_drag
        && active_index == index
    {
        let view_fraction =
            ((pos.x - offset - env.rect.left()) / env.rect.width()).clamp(0.0, 1.0);
        let absolute = env.view.start + env.view_width.max(f32::EPSILON) * view_fraction;
        let clamped = absolute.clamp(0.0, 1.0);
        if let Some(slot) = app.controller.ui.waveform.slices.get_mut(active_index) {
            *slot = update_slice_edge(*slot, edge, clamped);
        }
    }
}

fn update_slice_edge(range: SelectionRange, edge: SelectionEdge, position: f32) -> SelectionRange {
    let min_width = crate::egui_app::controller::MIN_SELECTION_WIDTH;
    match edge {
        SelectionEdge::Start => {
            let max_start = (range.end() - min_width).max(0.0);
            SelectionRange::new(position.min(max_start), range.end())
        }
        SelectionEdge::End => {
            let min_end = (range.start() + min_width).min(1.0);
            SelectionRange::new(range.start(), position.max(min_end))
        }
    }
}

fn edge_position_px(edge: SelectionEdge, slice_rect: egui::Rect) -> f32 {
    match edge {
        SelectionEdge::Start => slice_rect.left(),
        SelectionEdge::End => slice_rect.right(),
    }
}

fn apply_edge_hover(
    ui: &mut egui::Ui,
    env: &SliceOverlayEnv<'_>,
    edge_rect: egui::Rect,
    edge: SelectionEdge,
    edge_response: &egui::Response,
) {
    let edge_hovered = env.pointer_pos.is_some_and(|p| edge_rect.contains(p))
        || edge_response.hovered()
        || edge_response.is_pointer_button_down_on()
        || edge_response.dragged();
    if edge_hovered {
        paint_selection_edge_bracket(ui.painter(), edge_rect, edge, env.slice_color);
        ui.output_mut(|o| o.cursor_icon = CursorIcon::ResizeHorizontal);
    }
}

fn sync_slice_drag_release(app: &mut EguiApp, ctx: &egui::Context) {
    if !ctx.input(|i| i.pointer.primary_down()) {
        app.slice_drag = None;
    }
}

fn finish_slice_drag(app: &mut EguiApp, index: usize) {
    if let Some(SliceDragState { index: active_index, .. }) = app.slice_drag
        && active_index == index
    {
        app.slice_drag = None;
    }
}

fn to_wave_pos(
    env: &SliceOverlayEnv<'_>,
    pos: egui::Pos2,
) -> f32 {
    let normalized = ((pos.x - env.rect.left()) / env.rect.width()).clamp(0.0, 1.0);
    normalized.mul_add(env.view_width, env.view.start).clamp(0.0, 1.0)
}
