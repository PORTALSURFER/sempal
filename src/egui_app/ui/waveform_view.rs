use super::style;
use super::*;
use crate::egui_app::state::DragSource;
use crate::egui_app::view_model;
use eframe::egui::{self, StrokeKind, Ui};
use std::time::Duration;

mod base_render;
mod beat_grid;
mod controls;
mod destructive_prompt;
mod edit_selection_overlay;
mod hover_overlay;
mod interactions;
mod overlays;
mod selection_drag;
mod selection_geometry;
mod selection_menu;
mod selection_overlay;
mod slice_overlay;

struct WaveformFrameContext {
    palette: style::Palette,
    highlight: egui::Color32,
    cursor_color: egui::Color32,
    start_marker_color: egui::Color32,
    is_loading: bool,
    flash_alpha: Option<u8>,
    tooltip_mode: crate::sample_sources::config::TooltipMode,
}

struct WaveformLayout {
    rect: egui::Rect,
    waveform_rect: egui::Rect,
    response: egui::Response,
    display_view: crate::egui_app::state::WaveformView,
    view_width: f64,
    scrollbar_height: f32,
    pointer_pos: Option<egui::Pos2>,
}

#[derive(Default)]
struct WaveformDragState {
    edge_dragging: bool,
    slice_dragging: bool,
    consumed_click: bool,
}

impl EguiApp {
    pub(super) fn render_waveform(&mut self, ui: &mut Ui) {
        let context = prepare_waveform_frame_context(self);
        controls::render_waveform_controls(self, ui, &context.palette);
        let frame = style::section_frame();
        let frame_response = frame.show(ui, |ui| render_waveform_frame(self, ui, &context));
        let drag_state = frame_response.inner;
        if frame_response.response.hovered() && !drag_state.edge_dragging && !drag_state.slice_dragging
        {
            helpers::show_hover_hint(
                ui,
                context.tooltip_mode,
                "Left-click: Seek | Primary Drag: Select | Secondary Drag: Edit Selection\nMiddle Drag: Pan | Ctrl+Shift+Alt+Primary Drag: Circular Slide",
            );
        }
        style::paint_section_border(ui, frame_response.response.rect, false);
        if let Some(prompt) = self.controller.ui.waveform.pending_destructive.clone() {
            self.render_destructive_edit_prompt(ui.ctx(), prompt);
        }
        if matches!(
            self.controller.ui.focus.context,
            crate::egui_app::state::FocusContext::Waveform
        ) {
            ui.painter().rect_stroke(
                frame_response.response.rect,
                2.0,
                style::focused_row_stroke(),
                StrokeKind::Outside,
            );
        }
    }
}

const WAVEFORM_DRAG_HANDLE_SIZE: f32 = 16.0;
const WAVEFORM_DRAG_HANDLE_MARGIN: f32 = 8.0;
const WAVEFORM_SCROLLBAR_HEIGHT: f32 = 6.0;
const WAVEFORM_FIXED_HEIGHT: f32 = 260.0;
const WAVEFORM_RESPONSIVE_FRACTION: f32 = 0.4;
const WAVEFORM_MIN_HEIGHT: f32 = 160.0;
const WAVEFORM_MAX_HEIGHT: f32 = 320.0;

fn prepare_waveform_frame_context(app: &mut EguiApp) -> WaveformFrameContext {
    let palette = style::palette();
    let highlight = palette.accent_copper;
    let cursor_color = palette.accent_mint;
    let start_marker_color = palette.accent_ice;
    let is_loading = app.controller.ui.waveform.loading.is_some();
    let flash_alpha = helpers::flash_alpha(
        &mut app.controller.ui.waveform.copy_flash_at,
        Duration::from_millis(260),
        70,
    );
    let tooltip_mode = app.controller.ui.controls.tooltip_mode;
    WaveformFrameContext {
        palette,
        highlight,
        cursor_color,
        start_marker_color,
        is_loading,
        flash_alpha,
        tooltip_mode,
    }
}

fn render_waveform_frame(
    app: &mut EguiApp,
    ui: &mut Ui,
    context: &WaveformFrameContext,
) -> WaveformDragState {
    let layout = allocate_waveform_layout(app, ui);
    layout.response.context_menu(|ui| {
        selection_menu::render_selection_context_menu(app, ui);
    });
    let Some(drag_state) = render_waveform_layers(app, ui, context, &layout) else {
        return WaveformDragState::default();
    };
    handle_waveform_interactions(app, ui, &layout, &drag_state);
    drag_state
}

fn allocate_waveform_layout(app: &mut EguiApp, ui: &mut Ui) -> WaveformLayout {
    let view = app.controller.ui.waveform.view;
    // Always use the actual f64 view for precision
    // Don't use the image's snapped view as it causes desync
    let display_view = view;
    let view_width = display_view.width();
    let scrollbar_height = if view_width < 1.0 {
        WAVEFORM_SCROLLBAR_HEIGHT
    } else {
        0.0
    };
    let available_height = ui.available_height().max(0.0);
    let fixed_total_height = WAVEFORM_FIXED_HEIGHT + scrollbar_height;
    let responsive_height = (available_height * WAVEFORM_RESPONSIVE_FRACTION)
        .clamp(WAVEFORM_MIN_HEIGHT, WAVEFORM_MAX_HEIGHT);
    let total_height = if available_height >= fixed_total_height {
        fixed_total_height
    } else {
        (responsive_height + scrollbar_height).min(available_height)
    };
    let desired = egui::vec2(ui.available_width(), total_height);
    let (rect, _) = ui.allocate_exact_size(desired, egui::Sense::hover());
    let waveform_rect = egui::Rect::from_min_size(
        rect.min,
        egui::vec2(rect.width(), (rect.height() - scrollbar_height).max(1.0)),
    );
    let response = ui.interact(
        waveform_rect,
        ui.id().with("waveform_area"),
        egui::Sense::click_and_drag(),
    );
    let target_width = rect.width().round().max(1.0) as u32;
    let target_height = waveform_rect.height().round().max(1.0) as u32;
    app.controller
        .update_waveform_size(target_width, target_height);
    let pointer_pos = response.hover_pos();
    WaveformLayout {
        rect,
        waveform_rect,
        response,
        display_view,
        view_width,
        scrollbar_height,
        pointer_pos,
    }
}

fn render_waveform_layers(
    app: &mut EguiApp,
    ui: &mut Ui,
    context: &WaveformFrameContext,
    layout: &WaveformLayout,
) -> Option<WaveformDragState> {
    let to_screen_x = |position: f32, rect: egui::Rect| {
        let normalized = ((position as f64 - layout.display_view.start) / layout.view_width)
            .clamp(0.0, 1.0);
        rect.left() + rect.width() * normalized as f32
    };
    if !base_render::render_waveform_base(
        app,
        ui,
        layout.waveform_rect,
        &context.palette,
        context.is_loading,
    ) {
        return None;
    }
    beat_grid::render_waveform_beat_grid(
        app,
        ui,
        layout.waveform_rect,
        &context.palette,
        layout.display_view,
        layout.view_width,
    );

    hover_overlay::render_hover_overlay(
        app,
        ui,
        layout.waveform_rect,
        layout.pointer_pos,
        layout.display_view,
        layout.view_width,
        context.cursor_color,
        &to_screen_x,
    );

    let slice_result = slice_overlay::render_slice_overlays(
        app,
        ui,
        layout.waveform_rect,
        &context.palette,
        layout.display_view,
        layout.view_width,
        layout.pointer_pos,
    );
    edit_selection_overlay::render_edit_selection_overlay(
        app,
        ui,
        layout.waveform_rect,
        layout.display_view,
        layout.view_width,
    );
    let edge_dragging = selection_overlay::render_selection_overlay(
        app,
        ui,
        layout.waveform_rect,
        &context.palette,
        layout.display_view,
        layout.view_width,
        context.highlight,
        layout.pointer_pos,
    );
    overlays::render_overlays(
        app,
        ui,
        layout.waveform_rect,
        layout.display_view,
        layout.view_width,
        context.highlight,
        context.start_marker_color,
        &to_screen_x,
    );
    if let Some(alpha) = context.flash_alpha {
        let flash_color = style::with_alpha(style::semantic_palette().drag_highlight, alpha);
        ui.painter()
            .rect_filled(layout.waveform_rect, 0.0, flash_color);
    }
    render_waveform_drag_handle(app, ui, layout.waveform_rect, &context.palette);

    Some(WaveformDragState {
        edge_dragging,
        slice_dragging: slice_result.dragging,
        consumed_click: slice_result.consumed_click,
    })
}

fn handle_waveform_interactions(
    app: &mut EguiApp,
    ui: &mut Ui,
    layout: &WaveformLayout,
    drag_state: &WaveformDragState,
) {
    interactions::handle_waveform_interactions(
        app,
        ui,
        layout.waveform_rect,
        &layout.response,
        layout.display_view,
        layout.view_width,
    );
    if !drag_state.edge_dragging && !drag_state.slice_dragging && !drag_state.consumed_click {
        interactions::handle_waveform_pointer_interactions(
            app,
            ui,
            layout.waveform_rect,
            &layout.response,
            layout.display_view,
            layout.view_width,
        );
    }

    if layout.scrollbar_height > 0.0 {
        let scroll_rect = egui::Rect::from_min_size(
            egui::pos2(layout.rect.left(), layout.waveform_rect.bottom()),
            egui::vec2(layout.rect.width(), layout.scrollbar_height),
        );
        interactions::render_waveform_scrollbar(
            app,
            ui,
            scroll_rect,
            layout.display_view,
            layout.view_width,
        );
    }
}

fn render_waveform_drag_handle(
    app: &mut EguiApp,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    palette: &style::Palette,
) {
    let handle_rect = waveform_drag_handle_rect(rect);
    let response = ui.interact(
        handle_rect,
        ui.id().with("waveform_drag_handle"),
        egui::Sense::click_and_drag(),
    );
    paint_waveform_drag_handle(ui, handle_rect, palette, &response);
    if response.hovered() {
        helpers::show_hover_hint(
            ui,
            app.controller.ui.controls.tooltip_mode,
            "Primary Drag: Export sample",
        );
    }
    handle_waveform_drag_handle_interactions(app, ui, &response);
}

fn waveform_drag_handle_rect(rect: egui::Rect) -> egui::Rect {
    let size = egui::vec2(WAVEFORM_DRAG_HANDLE_SIZE, WAVEFORM_DRAG_HANDLE_SIZE);
    let min = egui::pos2(
        rect.right() - size.x - WAVEFORM_DRAG_HANDLE_MARGIN,
        rect.bottom() - size.y - WAVEFORM_DRAG_HANDLE_MARGIN,
    );
    egui::Rect::from_min_size(min, size)
}

fn paint_waveform_drag_handle(
    ui: &egui::Ui,
    rect: egui::Rect,
    palette: &style::Palette,
    response: &egui::Response,
) {
    let active = response.hovered() || response.dragged();
    let fill = if active {
        style::with_alpha(palette.accent_copper, 140)
    } else {
        style::with_alpha(palette.bg_secondary, 170)
    };
    let stroke = if active {
        egui::Stroke::new(1.5, palette.accent_copper)
    } else {
        egui::Stroke::new(1.0, palette.grid_soft)
    };
    ui.painter().rect_filled(rect, 2.0, fill);
    ui.painter()
        .rect_stroke(rect, 2.0, stroke, StrokeKind::Inside);
    if response.dragged() {
        ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::Grabbing);
    } else if response.hovered() {
        ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::Grab);
    }
}

fn handle_waveform_drag_handle_interactions(
    app: &mut EguiApp,
    ui: &mut egui::Ui,
    response: &egui::Response,
) {
    if response.drag_started() {
        let Some(pos) = response.interact_pointer_pos() else {
            return;
        };
        let Some(source) = app.controller.current_source() else {
            app.controller.set_status(
                "Select a source before dragging",
                style::StatusTone::Warning,
            );
            return;
        };
        let Some(path) = app.controller.ui.loaded_wav.clone() else {
            app.controller
                .set_status("Load a sample before dragging", style::StatusTone::Warning);
            return;
        };
        let label = view_model::sample_display_label(&path);
        app.controller
            .start_sample_drag(source.id.clone(), path, label, pos);
        app.controller.ui.drag.origin_source = Some(DragSource::Waveform);
    } else if response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            let shift_down = ui.input(|i| i.modifiers.shift);
            let alt_down = ui.input(|i| i.modifiers.alt);
            app.controller
                .refresh_drag_position(pos, shift_down, alt_down);
        }
    } else if response.drag_stopped() {
        app.controller.finish_active_drag();
    }
}
