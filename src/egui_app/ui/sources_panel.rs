use super::helpers::{clamp_label_for_width, list_row_height, render_list_row};
use super::style;
use super::*;
use crate::egui_app::state::FocusContext;
use eframe::egui::{self, Align2, RichText, StrokeKind, TextStyle, Ui};

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
            let folder_height = ui.available_height().max(0.0);
            self.render_folder_browser(ui, folder_height);
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

    fn update_sources_panel_drop_state(&mut self, ctx: &egui::Context, rect: egui::Rect) -> bool {
        self.sources_panel_drop_hovered = ctx.input(|i| {
            let pointer_pos = i.pointer.hover_pos().or_else(|| i.pointer.interact_pos());
            let pointer_over = pointer_pos.map_or(true, |pos| rect.contains(pos));
            let hovered_has_path = i.raw.hovered_files.iter().any(|file| file.path.is_some());
            hovered_has_path && pointer_over
        });
        if self.sources_panel_drop_hovered {
            self.sources_panel_drop_armed = true;
        } else if ctx.input(|i| {
            i.pointer
                .hover_pos()
                .or_else(|| i.pointer.interact_pos())
                .is_some_and(|pos| !rect.contains(pos))
        }) {
            self.sources_panel_drop_armed = false;
        }
        self.sources_panel_drop_hovered
    }

    fn source_row_menu(
        &mut self,
        response: &egui::Response,
        index: usize,
        row: &crate::egui_app::state::SourceRowView,
    ) {
        response.context_menu(|ui| {
            let palette = style::palette();
            ui.label(RichText::new(row.name.clone()).color(palette.text_primary));
            let mut close_menu = false;
            if ui.button("Quick sync").clicked() {
                self.controller.select_source_by_index(index);
                self.controller.request_quick_sync();
                close_menu = true;
            }
            if ui
                .button("Hard sync (full rescan)")
                .on_hover_text("Prune missing rows and rebuild from disk")
                .clicked()
            {
                self.controller.select_source_by_index(index);
                self.controller.request_hard_sync();
                close_menu = true;
            }
            ui.separator();
            if ui.button("Open in file explorer").clicked() {
                self.controller.select_source_by_index(index);
                self.controller.open_source_folder(index);
                close_menu = true;
            }
            if ui.button("Remap source…").clicked() {
                self.controller.select_source_by_index(index);
                self.controller.remap_source_via_dialog(index);
                close_menu = true;
            }
            let remove_btn = egui::Button::new(
                RichText::new("Remove source")
                    .color(style::destructive_text())
                    .strong(),
            );
            if ui.add(remove_btn).clicked() {
                self.controller.remove_source(index);
                close_menu = true;
            }
            if close_menu {
                ui.close();
            }
        });
    }

    fn render_sources_list(&mut self, ui: &mut Ui, height: f32) {
        egui::ScrollArea::vertical()
            .id_salt("sources_scroll")
            .max_height(height)
            .show(ui, |ui| {
                let rows = self.controller.ui.sources.rows.clone();
                let selected = self.controller.ui.sources.selected;
                let row_height = list_row_height(ui);
                for (index, row) in rows.iter().enumerate() {
                    let is_selected = Some(index) == selected;
                    ui.push_id(&row.id, |ui| {
                        let row_width = ui.available_width();
                        let padding = ui.spacing().button_padding.x * 2.0;
                        let base_label = clamp_label_for_width(&row.name, row_width - padding);
                        let label = if row.missing {
                            format!("! {base_label}")
                        } else {
                            base_label
                        };
                        let text_color = if row.missing {
                            style::missing_text()
                        } else {
                            style::high_contrast_text()
                        };
                        let bg = is_selected.then_some(style::row_selected_fill());
                        let response = render_list_row(
                            ui,
                            &label,
                            row_width,
                            row_height,
                            bg,
                            text_color,
                            egui::Sense::click(),
                            None,
                            None,
                        )
                        .on_hover_text(&row.path);
                        if response.clicked() {
                            self.controller.select_source_by_index(index);
                        }
                        self.source_row_menu(&response, index, row);
                    });
                }
            });
    }

    fn render_folder_browser(&mut self, ui: &mut Ui, height: f32) {
        let palette = style::palette();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Folders").color(palette.text_primary));
        });
        let frame = style::section_frame();
        let focused = matches!(
            self.controller.ui.focus.context,
            FocusContext::SourceFolders
        );
        let scroll_to = self.controller.ui.sources.folders.scroll_to;
        let frame_response = frame.show(ui, |ui| {
            ui.set_min_height(height);
            ui.set_max_height(height);
            let rows = self.controller.ui.sources.folders.rows.clone();
            let row_height = list_row_height(ui);
            let scroll = egui::ScrollArea::vertical()
                .id_salt("folder_browser_scroll")
                .max_height(height);
            scroll.show(ui, |ui| {
                if rows.is_empty() {
                    let text = if self.controller.current_source().is_some() {
                        "No folders detected for this source"
                    } else {
                        "Add a source to browse folders"
                    };
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), row_height),
                        egui::Sense::hover(),
                    );
                    ui.painter().text(
                        rect.left_center(),
                        Align2::LEFT_CENTER,
                        text,
                        TextStyle::Body.resolve(ui.style()),
                        palette.text_muted,
                    );
                    return;
                }
                let focused_row = self.controller.ui.sources.folders.focused;
                for (index, row) in rows.iter().enumerate() {
                    let is_focused = Some(index) == focused_row;
                    let bg = if row.selected || is_focused {
                        Some(style::row_selected_fill())
                    } else {
                        None
                    };
                    let row_width = ui.available_width();
                    let label = folder_row_label(row, row_width, ui);
                    let response = render_list_row(
                        ui,
                        &label,
                        row_width,
                        row_height,
                        bg,
                        style::high_contrast_text(),
                        egui::Sense::click(),
                        None,
                        None,
                    );
                    if Some(index) == scroll_to {
                        ui.scroll_to_rect(response.rect, None);
                    }
                    if row.selected {
                        let marker_width = 4.0;
                        let marker_rect = egui::Rect::from_min_max(
                            response.rect.left_top(),
                            response.rect.left_top() + egui::vec2(marker_width, row_height),
                        );
                        ui.painter()
                            .rect_filled(marker_rect, 0.0, style::selection_marker_fill());
                    }
                    if response.clicked() {
                        let pointer = response.interact_pointer_pos();
                        let hit_expand = row.has_children
                            && pointer.map_or(false, |pos| {
                                let padding = ui.spacing().button_padding.x;
                                let indent = row.depth as f32 * 12.0;
                                pos.x <= response.rect.left() + padding + indent + 14.0
                            });
                        if hit_expand {
                            self.controller.toggle_folder_expanded(index);
                        } else {
                            let modifiers = ui.input(|i| i.modifiers);
                            if modifiers.shift {
                                self.controller.select_folder_range(index);
                            } else if modifiers.command || modifiers.ctrl {
                                self.controller.toggle_folder_row_selection(index);
                            } else {
                                self.controller.replace_folder_selection(index);
                            }
                        }
                    } else if response.secondary_clicked() {
                        self.controller.focus_folder_row(index);
                    }
                    if is_focused {
                        ui.painter().rect_stroke(
                            response.rect,
                            0.0,
                            style::focused_row_stroke(),
                            StrokeKind::Inside,
                        );
                    }
                }
            });
        });
        style::paint_section_border(ui, frame_response.response.rect, focused);
        self.controller.ui.sources.folders.scroll_to = None;
    }
}

fn folder_row_label(
    row: &crate::egui_app::state::FolderRowView,
    row_width: f32,
    ui: &Ui,
) -> String {
    let padding = ui.spacing().button_padding.x * 2.0;
    let indent = "  ".repeat(row.depth);
    let icon = if row.has_children {
        if row.expanded { "v" } else { ">" }
    } else {
        "-"
    };
    let raw = format!("{indent}{icon} {}", row.name);
    clamp_label_for_width(&raw, row_width - padding)
}
