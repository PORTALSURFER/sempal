use super::style;
use super::*;
use crate::egui_app::state::{DragPayload, DragSource, FocusContext};
use eframe::egui::{self, Align2, RichText, StrokeKind, TextStyle, Ui};

mod drag_drop;
mod drop_targets;
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
            if ui.rect_contains_pointer(sources_rect) {
                self.controller.focus_sources_context();
            }
            ui.add_space(8.0);
            let remaining = ui.available_height();
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
            let drop_targets_active = matches!(
                drag_payload,
                Some(
                    DragPayload::Sample { .. }
                        | DragPayload::Samples { .. }
                        | DragPayload::Folder { .. }
                        | DragPayload::DropTargetReorder { .. }
                )
            );
            if drag_payload.is_some() && !folder_drop_active {
                self.controller
                    .ui
                    .drag
                    .clear_targets_from(DragSource::Folders);
            }
            if drag_payload.is_some() && !source_drop_active {
                self.controller
                    .ui
                    .drag
                    .clear_targets_from(DragSource::Sources);
            }
            if drag_payload.is_some() && !drop_targets_active {
                self.controller
                    .ui
                    .drag
                    .clear_targets_from(DragSource::DropTargets);
            }
            let handle_height = 10.0;
            let min_folder_height = 60.0;
            let min_drop_targets_height = 40.0;
            let default_drop_targets_height = (remaining * 0.25).clamp(60.0, 160.0);
            let mut drop_targets_height = self
                .controller
                .ui
                .sources
                .drop_targets
                .height_override
                .unwrap_or(default_drop_targets_height);
            let mut height_override = self.controller.ui.sources.drop_targets.height_override;
            let mut resize_origin = self.controller.ui.sources.drop_targets.resize_origin_height;
            let max_drop_targets_height =
                (remaining - min_folder_height - handle_height).max(min_drop_targets_height);
            let clamp_heights = |mut drop_height: f32| {
                drop_height = drop_height.clamp(min_drop_targets_height, max_drop_targets_height);
                let mut folder_height = remaining - drop_height - handle_height;
                if folder_height < min_folder_height {
                    folder_height = min_folder_height;
                    drop_height = (remaining - folder_height - handle_height)
                        .max(min_drop_targets_height);
                }
                (drop_height, folder_height)
            };
            let (mut drop_targets_height, mut folder_height) =
                clamp_heights(drop_targets_height);
            if let Some(current) = height_override {
                if (current - drop_targets_height).abs() > f32::EPSILON {
                    height_override = Some(drop_targets_height);
                }
            }
            let pointer_pos = ui
                .input(|i| i.pointer.hover_pos().or_else(|| i.pointer.interact_pos()))
                .or(self.controller.ui.drag.position);
            let available_rect = ui.available_rect_before_wrap();
            let build_layout = |folder_height: f32, drop_targets_height: f32| {
                let total_height = folder_height + handle_height + drop_targets_height;
                let layout_rect = egui::Rect::from_min_size(
                    available_rect.min,
                    egui::vec2(available_rect.width(), total_height),
                );
                let folder_rect = egui::Rect::from_min_size(
                    layout_rect.min,
                    egui::vec2(layout_rect.width(), folder_height),
                );
                let handle_rect = egui::Rect::from_min_size(
                    egui::pos2(layout_rect.left(), folder_rect.bottom()),
                    egui::vec2(layout_rect.width(), handle_height),
                );
                let drop_targets_rect = egui::Rect::from_min_size(
                    egui::pos2(layout_rect.left(), handle_rect.bottom()),
                    egui::vec2(layout_rect.width(), drop_targets_height),
                );
                (layout_rect, folder_rect, handle_rect, drop_targets_rect)
            };
            let (layout_rect, _folder_rect, handle_rect, _drop_targets_rect) =
                build_layout(folder_height, drop_targets_height);
            ui.allocate_rect(layout_rect, egui::Sense::hover());
            let handle_response =
                ui.interact(handle_rect, ui.id().with("drop_targets_handle"), egui::Sense::drag());
            if handle_response.hovered() || handle_response.dragged() {
                ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::ResizeVertical);
            }
            if handle_response.drag_started() {
                resize_origin = Some(drop_targets_height);
            }
            if handle_response.dragged() {
                let origin = resize_origin.unwrap_or(drop_targets_height);
                drop_targets_height = (origin - handle_response.drag_delta().y)
                    .clamp(min_drop_targets_height, max_drop_targets_height);
                height_override = Some(drop_targets_height);
            }
            if handle_response.drag_stopped() {
                resize_origin = None;
            }
            let handle_stroke = style::inner_border();
            ui.painter()
                .line_segment([handle_rect.center_top(), handle_rect.center_bottom()], handle_stroke);
            if height_override.is_some() {
                let (clamped_drop_height, clamped_folder_height) =
                    clamp_heights(drop_targets_height);
                drop_targets_height = clamped_drop_height;
                folder_height = clamped_folder_height;
                height_override = Some(drop_targets_height);
            }
            let (_layout_rect, folder_rect, handle_rect, drop_targets_rect) =
                build_layout(folder_height, drop_targets_height);
            ui.allocate_ui_at_rect(folder_rect, |ui| {
                self.render_folder_browser(ui, folder_height, folder_drop_active, pointer_pos);
                if ui.rect_contains_pointer(folder_rect) {
                    self.controller.focus_folder_context();
                }
            });
            let handle_stroke = style::inner_border();
            ui.painter().line_segment(
                [handle_rect.center_top(), handle_rect.center_bottom()],
                handle_stroke,
            );
            ui.allocate_ui_at_rect(drop_targets_rect, |ui| {
                self.render_drop_targets(ui, drop_targets_height);
            });
            self.controller.ui.sources.drop_targets.height_override = height_override;
            self.controller.ui.sources.drop_targets.resize_origin_height = resize_origin;

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
