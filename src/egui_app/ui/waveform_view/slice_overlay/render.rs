use crate::selection::{SelectionEdge, SelectionRange};
use eframe::egui::{self, Color32, CursorIcon};

use super::geometry::{edge_position_px, slice_rect, to_wave_pos, update_slice_edge};
use super::super::selection_geometry::{
    paint_selection_edge_bracket, selection_edge_handle_rect, selection_handle_height,
    selection_handle_rect,
};
use super::super::super::style;
use super::super::super::{EguiApp, SliceDragKind, SliceDragState};
use super::{SliceEdgeSpec, SliceItem, SliceOverlayEnv, SliceOverlayResult};

pub(super) fn render_slice_overlays(
    app: &mut EguiApp,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    palette: &style::Palette,
    view: crate::egui_app::state::WaveformView,
    view_width: f32,
    pointer_pos: Option<egui::Pos2>,
) -> SliceOverlayResult {
    let has_slices = !app.controller.ui.waveform.slices.is_empty();
    if !has_slices && app.slice_paint.is_none() {
        app.slice_drag = None;
        return SliceOverlayResult::default();
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
    let mut result = SliceOverlayResult {
        dragging: app.slice_drag.is_some(),
        consumed_click: false,
    };
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
        let item_result = render_slice_overlay(app, ui, &env, item);
        result.dragging |= item_result.dragging;
        result.consumed_click |= item_result.consumed_click;
    }
    if let Some(state) = app.slice_paint {
        render_slice_paint_preview(ui, &env, state.range);
    }

    sync_slice_drag_release(app, ui.ctx());
    result
}

fn render_slice_overlay(
    app: &mut EguiApp,
    ui: &mut egui::Ui,
    env: &SliceOverlayEnv<'_>,
    item: SliceItem,
) -> SliceOverlayResult {
    let Some(slice_rect) = slice_rect(env, item.range) else {
        return SliceOverlayResult::default();
    };
    let ctrl_down = ui.input(|i| i.modifiers.ctrl);
    let body_response = ui.interact(
        slice_rect,
        ui.id().with(("slice_body", item.index)),
        egui::Sense::click(),
    );
    let handle_rect = selection_handle_rect(slice_rect);
    let handle_response = ui.interact(
        handle_rect,
        ui.id().with(("slice_handle", item.index)),
        egui::Sense::click_and_drag(),
    );
    let hovered = body_response.hovered() || handle_response.hovered();
    let selected = app
        .controller
        .ui
        .waveform
        .selected_slices
        .contains(&item.index);
    paint_slice(
        ui,
        slice_rect,
        handle_rect,
        env.slice_color,
        env.palette.accent_mint,
        hovered,
        selected,
    );
    let mut result = SliceOverlayResult::default();
    result.dragging |= render_slice_handle(app, ui, env, item, &handle_response);
    result.dragging |= render_slice_edges(app, ui, env, slice_rect, item.index);
    if body_response.clicked() || handle_response.clicked() {
        if app.controller.ui.waveform.image.is_some() {
            app.controller.focus_waveform_context();
        }
        if ctrl_down && app.controller.ui.waveform.slice_mode_enabled {
            app.controller.toggle_slice_selection(item.index);
        } else {
            play_slice_range(app, item.range);
        }
        result.consumed_click = true;
    }
    draw_slice_bar(ui, slice_rect, env);
    result
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

fn render_slice_edge(
    app: &mut EguiApp,
    ui: &mut egui::Ui,
    env: &SliceOverlayEnv<'_>,
    spec: SliceEdgeSpec,
) -> bool {
    let edge_response = ui.interact(
        spec.edge_rect,
        ui.id().with((spec.edge_id, spec.index)),
        egui::Sense::click_and_drag(),
    );
    handle_slice_edge_drag(app, env, &spec, &edge_response);
    apply_edge_hover(ui, env, spec.edge_rect, spec.edge, &edge_response);
    edge_response.dragged()
}

fn paint_slice(
    ui: &egui::Ui,
    slice_rect: egui::Rect,
    handle_rect: egui::Rect,
    color: Color32,
    selected_color: Color32,
    hovered: bool,
    selected: bool,
) {
    let fill_color = if selected {
        style::with_alpha(selected_color, if hovered { 150 } else { 110 })
    } else {
        style::with_alpha(color, if hovered { 100 } else { 60 })
    };
    let handle_color = if selected {
        style::with_alpha(selected_color, if hovered { 255 } else { 235 })
    } else {
        style::with_alpha(color, if hovered { 235 } else { 180 })
    };
    let outline_color = if selected {
        style::with_alpha(selected_color, if hovered { 255 } else { 240 })
    } else if hovered {
        style::with_alpha(color, 220)
    } else {
        style::with_alpha(color, 0)
    };
    let painter = ui.painter();
    painter.rect_filled(slice_rect, 0.0, fill_color);
    painter.rect_filled(handle_rect, 0.0, handle_color);
    painter.rect_stroke(
        slice_rect,
        0.0,
        egui::Stroke::new(1.4, outline_color),
        egui::StrokeKind::Inside,
    );
}

fn render_slice_paint_preview(ui: &egui::Ui, env: &SliceOverlayEnv<'_>, range: SelectionRange) {
    let Some(slice_rect) = slice_rect(env, range) else {
        return;
    };
    let handle_rect = selection_handle_rect(slice_rect);
    paint_slice(
        ui,
        slice_rect,
        handle_rect,
        env.slice_color,
        env.palette.accent_mint,
        true,
        false,
    );
    draw_slice_bar(ui, slice_rect, env);
}

fn play_slice_range(app: &mut EguiApp, range: SelectionRange) {
    app.controller.set_selection_range(range);
    if let Err(err) = app.controller.play_audio(false, Some(range.start())) {
        app.controller.set_status(err, style::StatusTone::Error);
    }
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
        if let Some(new_index) = app.controller.update_slice_range(active_index, shifted) {
            if let Some(state) = app.slice_drag.as_mut() {
                state.index = new_index;
            }
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
        if let Some(slice) = app.controller.ui.waveform.slices.get(active_index).copied() {
            let updated = update_slice_edge(slice, edge, clamped);
            if let Some(new_index) = app.controller.update_slice_range(active_index, updated) {
                if let Some(state) = app.slice_drag.as_mut() {
                    state.index = new_index;
                }
            }
        }
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
