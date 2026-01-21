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
            let _header_response = ui.horizontal(|ui| {
                ui.label(RichText::new("Sources").color(palette.text_primary));
                if ui
                    .button(RichText::new("+").color(palette.text_primary))
                    .clicked()
                {
                    self.controller.add_source_via_dialog();
                }
            });
            let sources_header_gap = 6.0;
            ui.add_space(sources_header_gap);
            let total_available = ui.available_height();
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
            let row_height = helpers::list_row_height(ui);
            let header_gap = ui.spacing().item_spacing.y;
            let folder_header_height = self
                .controller
                .ui
                .sources
                .folders
                .header_height
                .max(row_height);
            let drop_header_height = self
                .controller
                .ui
                .sources
                .drop_targets
                .header_height
                .max(row_height);
            let handle_height = 10.0;
            let component_budget = (total_available - handle_height * 2.0).max(0.0);
            let min_content_height = row_height * 2.0;
            let mut min_sources_total = min_content_height;
            let mut min_folder_total = folder_header_height + header_gap + min_content_height;
            let mut min_drop_total = drop_header_height + header_gap + min_content_height;
            if min_sources_total + min_folder_total + min_drop_total > component_budget {
                min_sources_total = 0.0;
                min_folder_total = folder_header_height + header_gap;
                min_drop_total = drop_header_height + header_gap;
            }
            let default_sources_total = component_budget * 0.2;
            let default_drop_total = component_budget * 0.2;
            let mut sources_height_override = self.controller.ui.sources.sources_height_override;
            let mut sources_resize_origin =
                self.controller.ui.sources.sources_resize_origin_height;
            let mut height_override = self.controller.ui.sources.drop_targets.height_override;
            let mut resize_origin = self.controller.ui.sources.drop_targets.resize_origin_height;
            enum ResizeMode {
                None,
                Sources,
                DropTargets,
            }
            let clamp_range = |value: f32, min: f32, max: f32| {
                if max < min {
                    max
                } else {
                    value.clamp(min, max)
                }
            };
            let sources_list_height = sources_height_override.unwrap_or(default_sources_total);
            let drop_total = height_override.unwrap_or(default_drop_total);
            let max_sources =
                (component_budget - min_folder_total - min_drop_total).max(0.0);
            let mut sources_list_height =
                clamp_range(sources_list_height, min_sources_total, max_sources);
            let max_drop =
                (component_budget - min_folder_total - sources_list_height).max(0.0);
            let mut drop_total = clamp_range(drop_total, min_drop_total, max_drop);
            let mut folder_total =
                (component_budget - sources_list_height - drop_total).max(0.0);
            if let Some(current) = sources_height_override {
                if (current - sources_list_height).abs() > f32::EPSILON {
                    sources_height_override = Some(sources_list_height);
                }
            }
            if let Some(current) = height_override {
                if (current - drop_total).abs() > f32::EPSILON {
                    height_override = Some(drop_total);
                }
            }
            let pointer_pos = ui
                .input(|i| i.pointer.hover_pos().or_else(|| i.pointer.interact_pos()))
                .or(self.controller.ui.drag.position);
            let available_rect = ui.available_rect_before_wrap();
            let layout_rect = egui::Rect::from_min_size(
                available_rect.min,
                egui::vec2(available_rect.width(), total_available),
            );
            let build_layout = |sources_list_height: f32, folder_total: f32, drop_total: f32| {
                let sources_list_rect = egui::Rect::from_min_size(
                    layout_rect.min,
                    egui::vec2(layout_rect.width(), sources_list_height),
                );
                let sources_handle_rect = egui::Rect::from_min_size(
                    egui::pos2(layout_rect.left(), sources_list_rect.bottom()),
                    egui::vec2(layout_rect.width(), handle_height),
                );
                let folder_rect = egui::Rect::from_min_size(
                    egui::pos2(layout_rect.left(), sources_handle_rect.bottom()),
                    egui::vec2(layout_rect.width(), folder_total),
                );
                let drop_handle_rect = egui::Rect::from_min_size(
                    egui::pos2(layout_rect.left(), folder_rect.bottom()),
                    egui::vec2(layout_rect.width(), handle_height),
                );
                let drop_rect = egui::Rect::from_min_size(
                    egui::pos2(layout_rect.left(), drop_handle_rect.bottom()),
                    egui::vec2(layout_rect.width(), drop_total),
                );
                (
                    layout_rect,
                    sources_list_rect,
                    sources_handle_rect,
                    folder_rect,
                    drop_handle_rect,
                    drop_rect,
                    sources_list_height,
                )
            };
            let (layout_rect, _sources_list_rect, sources_handle_rect, _folder_rect,
                drop_handle_rect, _drop_rect, _sources_list_height) =
                build_layout(sources_list_height, folder_total, drop_total);
            ui.allocate_rect(layout_rect, egui::Sense::hover());
            let sources_handle_response = ui.interact(
                sources_handle_rect,
                ui.id().with("sources_handle"),
                egui::Sense::drag(),
            );
            let drop_handle_response = ui.interact(
                drop_handle_rect,
                ui.id().with("drop_targets_handle"),
                egui::Sense::drag(),
            );
            let resize_mode = if sources_handle_response.dragged() {
                ResizeMode::Sources
            } else if drop_handle_response.dragged() {
                ResizeMode::DropTargets
            } else {
                ResizeMode::None
            };
            if sources_handle_response.hovered()
                || sources_handle_response.dragged()
                || drop_handle_response.hovered()
                || drop_handle_response.dragged()
            {
                ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::ResizeVertical);
            }
            if sources_handle_response.drag_started() {
                sources_resize_origin = Some(sources_list_height);
            }
            if sources_handle_response.dragged() {
                let pointer = sources_handle_response
                    .interact_pointer_pos()
                    .or(pointer_pos);
                let max_sources =
                    (component_budget - drop_total - min_folder_total).max(0.0);
                sources_list_height = if let Some(pointer) = pointer {
                    let desired = pointer.y - layout_rect.top();
                    clamp_range(desired, min_sources_total, max_sources)
                } else {
                    let origin = sources_resize_origin.unwrap_or(sources_list_height);
                    clamp_range(
                        origin + sources_handle_response.drag_delta().y,
                        min_sources_total,
                        max_sources,
                    )
                };
                let max_drop =
                    (component_budget - sources_list_height - min_folder_total).max(0.0);
                drop_total = clamp_range(drop_total, min_drop_total, max_drop);
                sources_height_override = Some(sources_list_height);
                if height_override.is_some() {
                    height_override = Some(drop_total);
                }
            }
            if sources_handle_response.drag_stopped() {
                sources_resize_origin = None;
            }
            if drop_handle_response.drag_started() {
                resize_origin = Some(drop_total);
            }
            if drop_handle_response.dragged() {
                let pointer = drop_handle_response
                    .interact_pointer_pos()
                    .or(pointer_pos);
                let max_drop =
                    (component_budget - sources_list_height - min_folder_total).max(0.0);
                drop_total = if let Some(pointer) = pointer {
                    let desired = layout_rect.bottom() - pointer.y;
                    clamp_range(desired, min_drop_total, max_drop)
                } else {
                    let origin = resize_origin.unwrap_or(drop_total);
                    clamp_range(
                        origin - drop_handle_response.drag_delta().y,
                        min_drop_total,
                        max_drop,
                    )
                };
                let max_sources =
                    (component_budget - drop_total - min_folder_total).max(0.0);
                sources_list_height =
                    clamp_range(sources_list_height, min_sources_total, max_sources);
                height_override = Some(drop_total);
                if sources_height_override.is_some() {
                    sources_height_override = Some(sources_list_height);
                }
            }
            if drop_handle_response.drag_stopped() {
                resize_origin = None;
            }
            if matches!(resize_mode, ResizeMode::None) {
                let max_sources =
                    (component_budget - min_folder_total - min_drop_total).max(0.0);
                sources_list_height =
                    clamp_range(sources_list_height, min_sources_total, max_sources);
                let max_drop =
                    (component_budget - min_folder_total - sources_list_height).max(0.0);
                drop_total = clamp_range(drop_total, min_drop_total, max_drop);
            }
            folder_total = (component_budget - sources_list_height - drop_total).max(0.0);
            if sources_handle_response.dragged() && height_override.is_none() {
                if (drop_total - default_drop_total).abs() > f32::EPSILON {
                    height_override = Some(drop_total);
                }
            }
            if drop_handle_response.dragged() && sources_height_override.is_none() {
                if (sources_list_height - default_sources_total).abs() > f32::EPSILON {
                    sources_height_override = Some(sources_list_height);
                }
            }
            let handle_stroke = style::inner_border();
            let (_layout_rect, sources_list_rect, sources_handle_rect, folder_rect,
                drop_handle_rect, drop_rect, sources_list_height) =
                build_layout(sources_list_height, folder_total, drop_total);
            let sources_rect = ui
                .scope_builder(
                    egui::UiBuilder::new().max_rect(sources_list_rect),
                    |ui| self.render_sources_list(ui, sources_list_height),
                )
                .inner;
            ui.painter().line_segment(
                [sources_handle_rect.center_top(), sources_handle_rect.center_bottom()],
                handle_stroke,
            );
            ui.scope_builder(
                egui::UiBuilder::new().max_rect(folder_rect),
                |ui| {
                    self.render_folder_browser(ui, folder_total, folder_drop_active, pointer_pos);
                },
            );
            ui.painter().line_segment(
                [drop_handle_rect.center_top(), drop_handle_rect.center_bottom()],
                handle_stroke,
            );
            ui.scope_builder(
                egui::UiBuilder::new().max_rect(drop_rect),
                |ui| self.render_drop_targets(ui, drop_total),
            );
            self.controller.ui.sources.sources_height_override = sources_height_override;
            self.controller.ui.sources.sources_resize_origin_height = sources_resize_origin;
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
        if panel_rect.contains(ui.input(|i| i.pointer.hover_pos()).unwrap_or(egui::Pos2::ZERO)) {
            helpers::show_hover_hint(
                ui,
                self.controller.ui.controls.tooltip_mode,
                "Drop folders: Add to library | Right-click: Source menu",
            );
        }
    }
}
