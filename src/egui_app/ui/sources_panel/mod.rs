use super::style;
use super::*;
use crate::egui_app::state::{DragPayload, DragSource, FocusContext};
use eframe::egui::{self, Align, Align2, Layout, RichText, StrokeKind, TextStyle, Ui};

mod drag_drop;
mod folder_actions;
mod folder_browser;
mod sources_list;
mod utils;

impl EguiApp {
    pub(super) fn render_sources_panel(&mut self, ui: &mut Ui) {
        let panel_rect = ui.max_rect();
        self.sources_panel_rect = Some(panel_rect);
        let drop_hovered = self.update_sources_panel_drop_state(ui.ctx(), panel_rect);
        if drop_hovered {
            let highlight = style::with_alpha(style::semantic_palette().drag_highlight, 32);
            ui.painter().rect_filled(panel_rect, 0.0, highlight);
        }
        let palette = style::palette();
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Sources").color(palette.text_primary));
                if ui
                    .button(RichText::new("+").color(palette.text_primary))
                    .clicked()
                {
                    self.controller.add_source_via_dialog();
                }
            });
            ui.add_space(6.0);
            let source_list_height = (ui.available_height() * 0.25).max(140.0);
            self.render_sources_list(ui, source_list_height);
            ui.add_space(8.0);
            let remaining = ui.available_height();
            let folder_height = (remaining * 0.7).max(120.0).min(remaining);
            let selected_height = (remaining - folder_height).max(0.0);
            let drag_payload = self.controller.ui.drag.payload.clone();
            let folder_drop_active = matches!(
                drag_payload,
                Some(DragPayload::Sample { .. } | DragPayload::Selection { .. })
            );
            if drag_payload.is_some() && !folder_drop_active {
                self.controller
                    .ui
                    .drag
                    .clear_targets_from(DragSource::Folders);
            }
            let pointer_pos = ui
                .input(|i| i.pointer.hover_pos().or_else(|| i.pointer.interact_pos()))
                .or(self.controller.ui.drag.position);
            self.render_folder_browser(ui, folder_height, folder_drop_active, pointer_pos);
            ui.add_space(8.0);
            self.render_selected_folders(ui, selected_height);
        });
        if matches!(self.controller.ui.focus.context, FocusContext::SourcesList) {
            ui.painter().rect_stroke(
                panel_rect,
                0.0,
                style::focused_row_stroke(),
                StrokeKind::Outside,
            );
        }
        if drop_hovered {
            let painter = ui.painter();
            painter.rect_stroke(
                panel_rect.shrink(0.5),
                0.0,
                style::drag_target_stroke(),
                StrokeKind::Inside,
            );
            let font = TextStyle::Button.resolve(ui.style());
            painter.text(
                panel_rect.center(),
                Align2::CENTER_CENTER,
                "Drop folders to add",
                font,
                style::high_contrast_text(),
            );
        }
    }

    fn render_selected_folders(&mut self, ui: &mut Ui, max_height: f32) {
        let palette = style::palette();
        let selected_paths = self.controller.selected_folder_paths();
        let has_selection = !selected_paths.is_empty();
        let max_height = max_height.max(0.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Selected folders").color(palette.text_primary));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let button = egui::Button::new(
                    RichText::new("Clear selection").color(style::high_contrast_text()),
                )
                .small();
                let response = ui
                    .add_enabled(has_selection, button)
                    .on_hover_text("Show samples from all folders in this source");
                if response.clicked() {
                    self.controller.clear_folder_selection();
                }
            });
        });
        egui::ScrollArea::vertical()
            .id_salt("selected_folders_scroll")
            .max_height(max_height)
            .show(ui, |ui| {
                if selected_paths.is_empty() {
                    ui.label(RichText::new("No folders selected").color(palette.text_muted));
                    return;
                }
                ui.spacing_mut().item_spacing.y = 4.0;
                for path in selected_paths {
                    ui.label(
                        RichText::new(format!("• {}", path.display()))
                            .color(palette.text_primary),
                    );
                }
            });
    }
}
