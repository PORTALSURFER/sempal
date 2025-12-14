use super::style;
use super::*;
use eframe::egui::{self, StrokeKind, Ui};

mod base_render;
mod controls;
mod destructive_prompt;
mod hover_overlay;
mod interactions;
mod overlays;
mod selection_geometry;
mod selection_menu;
mod selection_overlay;

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
            let desired = egui::vec2(ui.available_width(), 260.0);
            let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::click_and_drag());
            let target_width = rect.width().round().max(1.0) as u32;
            let target_height = rect.height().round().max(1.0) as u32;
            self.controller
                .update_waveform_size(target_width, target_height);
            let pointer_pos = response.hover_pos();
            let view = self.controller.ui.waveform.view;
            let view_width = view.width();
            let to_screen_x = |position: f32, rect: egui::Rect| {
                let normalized = ((position - view.start) / view_width).clamp(0.0, 1.0);
                rect.left() + rect.width() * normalized
            };
            if !base_render::render_waveform_base(self, ui, rect, &palette, is_loading) {
                return;
            }

            hover_overlay::render_hover_overlay(
                self,
                ui,
                rect,
                pointer_pos,
                view,
                view_width,
                cursor_color,
                &to_screen_x,
            );

            let edge_dragging = selection_overlay::render_selection_overlay(
                self,
                ui,
                rect,
                &palette,
                view,
                view_width,
                highlight,
                pointer_pos,
            );
            overlays::render_overlays(
                self,
                ui,
                rect,
                view,
                view_width,
                highlight,
                start_marker_color,
                &to_screen_x,
            );

            interactions::handle_waveform_interactions(self, ui, rect, &response, view, view_width);
            if !edge_dragging {
                interactions::handle_waveform_pointer_interactions(
                    self,
                    ui,
                    rect,
                    &response,
                    view,
                    view_width,
                );
            }

            let view = self.controller.ui.waveform.view;
            let view_width = view.width();
            if view_width < 1.0 {
                interactions::render_waveform_scrollbar(self, ui, rect, view, view_width);
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
