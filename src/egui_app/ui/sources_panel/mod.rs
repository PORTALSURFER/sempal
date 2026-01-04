use super::style;
use super::*;
use crate::egui_app::state::{DragPayload, DragSource, FocusContext};
use eframe::egui::{Align2, RichText, StrokeKind, TextStyle, Ui};

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
            let sources_rect = self.render_sources_list(ui, source_list_height);
            ui.add_space(8.0);
            let remaining = ui.available_height();
            let folder_height = remaining.max(120.0);
            let drag_payload = self.controller.ui.drag.payload.clone();
            let folder_drop_active = matches!(
                drag_payload,
                Some(
                    DragPayload::Sample { .. }
                        | DragPayload::Samples { .. }
                        | DragPayload::Folder { .. }
                        | DragPayload::Selection { .. }
                )
            );
            let source_drop_active = matches!(
                drag_payload,
                Some(DragPayload::Sample { .. } | DragPayload::Samples { .. })
            );
            if drag_payload.is_some() && !folder_drop_active {
                self.controller
                    .ui
                    .drag
                    .clear_targets_from(DragSource::Folders);
            }
            if drag_payload.is_some() && !source_drop_active {
                self.controller.ui.drag.clear_targets_from(DragSource::Sources);
            }
            let pointer_pos = ui
                .input(|i| i.pointer.hover_pos().or_else(|| i.pointer.interact_pos()))
                .or(self.controller.ui.drag.position);
            self.render_folder_browser(ui, folder_height, folder_drop_active, pointer_pos);

            let focus = self.controller.ui.focus.context;
            let stroke = style::focused_row_stroke();
            if matches!(focus, FocusContext::SourcesList) {
                ui.painter()
                    .rect_stroke(sources_rect, 0.0, stroke, StrokeKind::Outside);
            }
        });
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
}
