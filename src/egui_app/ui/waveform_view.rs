use super::style;
use super::*;
use crate::egui_app::state::DragSource;
use crate::egui_app::view_model;
use eframe::egui::{self, StrokeKind, Ui};

mod base_render;
mod controls;
mod destructive_prompt;
mod hover_overlay;
mod interactions;
mod overlays;
mod selection_drag;
mod selection_geometry;
mod selection_menu;
mod selection_overlay;
mod slice_overlay;

impl EguiApp {
    pub(super) fn render_waveform(&mut self, ui: &mut Ui) {
        let palette = style::palette();
        let highlight = palette.accent_copper;
        let cursor_color = palette.accent_mint;
        let start_marker_color = palette.accent_ice;
        let is_loading = self.controller.ui.waveform.loading.is_some();
        controls::render_waveform_controls(self, ui, &palette);
        let frame = style::section_frame();
        let frame_response = frame.show(ui, |ui| {
            let view = self.controller.ui.waveform.view;
            let view_width = view.width();
            let scrollbar_height = if view_width < 1.0 {
                WAVEFORM_SCROLLBAR_HEIGHT
            } else {
                0.0
            };
            let desired = egui::vec2(ui.available_width(), 260.0 + scrollbar_height);
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
            self.controller
                .update_waveform_size(target_width, target_height);
            let pointer_pos = response.hover_pos();
            let to_screen_x = |position: f32, rect: egui::Rect| {
                let normalized = ((position - view.start) / view_width).clamp(0.0, 1.0);
                rect.left() + rect.width() * normalized
            };
            if !base_render::render_waveform_base(self, ui, waveform_rect, &palette, is_loading) {
                return;
            }

            hover_overlay::render_hover_overlay(
                self,
                ui,
                waveform_rect,
                pointer_pos,
                view,
                view_width,
                cursor_color,
                &to_screen_x,
            );

            let slice_dragging = slice_overlay::render_slice_overlays(
                self,
                ui,
                waveform_rect,
                &palette,
                view,
                view_width,
                pointer_pos,
            );
            let edge_dragging = selection_overlay::render_selection_overlay(
                self,
                ui,
                waveform_rect,
                &palette,
                view,
                view_width,
                highlight,
                pointer_pos,
            );
            overlays::render_overlays(
                self,
                ui,
                waveform_rect,
                view,
                view_width,
                highlight,
                start_marker_color,
                &to_screen_x,
            );
            render_waveform_drag_handle(self, ui, waveform_rect, &palette);

            interactions::handle_waveform_interactions(
                self,
                ui,
                waveform_rect,
                &response,
                view,
                view_width,
            );
            if !edge_dragging && !slice_dragging {
                interactions::handle_waveform_pointer_interactions(
                    self,
                    ui,
                    waveform_rect,
                    &response,
                    view,
                    view_width,
                );
            }

            if scrollbar_height > 0.0 {
                let scroll_rect = egui::Rect::from_min_size(
                    egui::pos2(rect.left(), waveform_rect.bottom()),
                    egui::vec2(rect.width(), scrollbar_height),
                );
                interactions::render_waveform_scrollbar(
                    self,
                    ui,
                    scroll_rect,
                    view,
                    view_width,
                );
            }
        });
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
            app.controller.refresh_drag_position(pos, shift_down);
        }
    } else if response.drag_stopped() {
        app.controller.finish_active_drag();
    }
}
