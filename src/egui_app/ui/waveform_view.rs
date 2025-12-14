use super::style;
use super::*;
use eframe::egui::{
    self, RichText, StrokeKind, Ui,
};

mod base_render;
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
        let mut view_mode = self.controller.ui.waveform.channel_view;
        ui.horizontal(|ui| {
            let mono = ui.selectable_value(
                &mut view_mode,
                crate::waveform::WaveformChannelView::Mono,
                "Mono envelope",
            );
            mono.on_hover_text("Show peak envelope across all channels");
            let split = ui.selectable_value(
                &mut view_mode,
                crate::waveform::WaveformChannelView::SplitStereo,
                "Split L/R",
            );
            split.on_hover_text("Render the first two channels separately");
            ui.add_space(10.0);
            let loop_enabled = self.controller.ui.waveform.loop_enabled;
            let loop_label = if loop_enabled {
                RichText::new("Loop: On").color(palette.accent_mint)
            } else {
                RichText::new("Loop: Off").color(palette.text_muted)
            };
            if ui
                .add(egui::Button::new(loop_label))
                .on_hover_text("Toggle loop playback for the current selection (or whole sample)")
                .clicked()
            {
                self.controller.toggle_loop();
            }
        });
        if view_mode != self.controller.ui.waveform.channel_view {
            self.controller.set_waveform_channel_view(view_mode);
        }
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
