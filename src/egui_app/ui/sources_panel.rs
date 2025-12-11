use super::helpers::{clamp_label_for_width, list_row_height, render_list_row};
use super::style;
use super::*;
use crate::egui_app::state::{DragPayload, DragSource, DragTarget, FocusContext};
use eframe::egui::{self, Align, Align2, Layout, RichText, StrokeKind, TextStyle, Ui};

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
            let sample_drag_active = matches!(drag_payload, Some(DragPayload::Sample { .. }));
            let pointer_pos = ui
                .input(|i| i.pointer.hover_pos().or_else(|| i.pointer.interact_pos()))
                .or(self.controller.ui.drag.position);
            self.render_folder_browser(ui, folder_height, sample_drag_active, pointer_pos);
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

    fn update_sources_panel_drop_state(&mut self, ctx: &egui::Context, rect: egui::Rect) -> bool {
        self.sources_panel_drop_hovered = ctx.input(|i| {
            let pointer_pos = i.pointer.hover_pos().or_else(|| i.pointer.interact_pos());
            let pointer_over = pointer_pos.is_none_or(|pos| rect.contains(pos));
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

    fn render_folder_browser(
        &mut self,
        ui: &mut Ui,
        height: f32,
        sample_drag_active: bool,
        pointer_pos: Option<egui::Pos2>,
    ) {
        let palette = style::palette();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Folders").color(palette.text_primary));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let mut query = self.controller.ui.sources.folders.search_query.clone();
                let response = ui.add(
                    egui::TextEdit::singleline(&mut query)
                        .hint_text("Search folders (f)...")
                        .desired_width(180.0),
                );
                if self.controller.ui.sources.folders.search_focus_requested {
                    response.request_focus();
                    self.controller.ui.sources.folders.search_focus_requested = false;
                }
                if response.changed() {
                    self.controller.set_folder_search(query);
                }
            });
        });
        let frame = style::section_frame();
        let focused = matches!(
            self.controller.ui.focus.context,
            FocusContext::SourceFolders
        );
        let scroll_to = self.controller.ui.sources.folders.scroll_to;
        let mut hovered_folder = None;
        let rows = self.controller.ui.sources.folders.rows.clone();
        let root_row = rows.first().filter(|row| row.is_root).cloned();
        let has_folder_rows = rows.iter().any(|row| !row.is_root);
        let mut inline_parent = self
            .controller
            .ui
            .sources
            .folders
            .new_folder
            .as_ref()
            .map(|state| state.parent.clone());
        if let Some(parent) = inline_parent.clone() {
            if !parent.as_os_str().is_empty() && !rows.iter().any(|row| row.path == parent) {
                self.controller.cancel_new_folder_creation();
                inline_parent = None;
            }
        }
        let inline_parent_for_rows = inline_parent.clone();
        let frame_response = frame.show(ui, |ui| {
            ui.set_min_height(height);
            ui.set_max_height(height);
            let row_height = list_row_height(ui);
            let active_folder_target = match &self.controller.ui.drag.active_target {
                DragTarget::FolderPanel { folder } => folder
                    .clone()
                    .or_else(|| self.controller.ui.drag.last_folder_target.clone()),
                _ => None,
            };
            if let Some(root_row) = root_row.clone() {
                let row_width = ui.available_width();
                let is_focused = self.controller.ui.sources.folders.focused == Some(0);
                let bg = if is_focused {
                    Some(style::row_selected_fill())
                } else {
                    None
                };
                let response = render_list_row(
                    ui,
                    &folder_row_label(&root_row, row_width, ui),
                    row_width,
                    row_height,
                    bg,
                    style::high_contrast_text(),
                    egui::Sense::click(),
                    None,
                    None,
                );
                if scroll_to == Some(0) {
                    ui.scroll_to_rect(response.rect, None);
                }
                if sample_drag_active {
                    if let Some(pointer) = pointer_pos
                        && response.rect.contains(pointer)
                    {
                        hovered_folder = Some(root_row.path.clone());
                        self.controller.update_active_drag(
                            pointer,
                            DragSource::Folders,
                            DragTarget::FolderPanel {
                                folder: Some(root_row.path.clone()),
                            },
                        );
                    }
                    if hovered_folder
                        .as_ref()
                        .is_some_and(|path| path == &root_row.path)
                        || active_folder_target
                            .as_ref()
                            .is_some_and(|path| path == &root_row.path)
                    {
                        ui.painter().rect_stroke(
                            response.rect.expand(2.0),
                            0.0,
                            style::drag_target_stroke(),
                            StrokeKind::Inside,
                        );
                    }
                }
                if response.clicked() {
                    self.controller.focus_folder_row(0);
                    self.controller.clear_folder_selection();
                } else if response.secondary_clicked() {
                    self.controller.focus_folder_row(0);
                }
                self.root_row_menu(&response);
                if is_focused {
                    ui.painter().rect_stroke(
                        response.rect,
                        0.0,
                        style::focused_row_stroke(),
                        StrokeKind::Inside,
                    );
                }
                ui.add_space(2.0);
            }
            let inline_parent = inline_parent_for_rows.clone();
            let scroll = egui::ScrollArea::vertical()
                .id_salt("folder_browser_scroll")
                .max_height(height);
            scroll.show(ui, |ui| {
                let mut inline_rendered = false;
                let inline_is_root = inline_parent
                    .as_ref()
                    .is_some_and(|path| path.as_os_str().is_empty());
                if inline_is_root {
                    inline_rendered = true;
                    let row_width = ui.available_width();
                    self.render_inline_new_folder_row(ui, 0, row_width, row_height);
                }
                if !has_folder_rows {
                    if inline_is_root {
                        return;
                    }
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
                let active_folder_target = match &self.controller.ui.drag.active_target {
                    DragTarget::FolderPanel { folder } => folder
                        .clone()
                        .or_else(|| self.controller.ui.drag.last_folder_target.clone()),
                    _ => None,
                };
                for (index, row) in rows.iter().enumerate() {
                    if row.is_root {
                        continue;
                    }
                    let is_focused = Some(index) == focused_row;
                    let rename_match = matches!(
                        self.controller.ui.sources.folders.pending_action,
                        Some(crate::egui_app::state::FolderActionPrompt::Rename {
                            ref target,
                            ..
                        }) if target == &row.path
                    );
                    let bg = if row.selected || is_focused {
                        Some(style::row_selected_fill())
                    } else {
                        None
                    };
                    let row_width = ui.available_width();
                    let label = if rename_match {
                        String::new()
                    } else {
                        folder_row_label(row, row_width, ui)
                    };
                    let sense = if rename_match {
                        egui::Sense::hover()
                    } else {
                        egui::Sense::click()
                    };
                    let response = render_list_row(
                        ui,
                        &label,
                        row_width,
                        row_height,
                        bg,
                        style::high_contrast_text(),
                        sense,
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
                    if sample_drag_active {
                        if let Some(pointer) = pointer_pos
                            && response.rect.contains(pointer)
                        {
                            hovered_folder = Some(row.path.clone());
                            self.controller.update_active_drag(
                                pointer,
                                DragSource::Folders,
                                DragTarget::FolderPanel {
                                    folder: Some(row.path.clone()),
                                },
                            );
                        }
                        if hovered_folder
                            .as_ref()
                            .is_some_and(|path| path == &row.path)
                            || active_folder_target
                                .as_ref()
                                .is_some_and(|path| path == &row.path)
                        {
                            ui.painter().rect_stroke(
                                response.rect.expand(2.0),
                                0.0,
                                style::drag_target_stroke(),
                                StrokeKind::Inside,
                            );
                        }
                    }
                    if rename_match {
                        self.render_folder_rename_editor(ui, &response, row);
                    } else if response.clicked() {
                        let pointer = response.interact_pointer_pos();
                        let hit_expand = row.has_children
                            && pointer.is_some_and(|pos| {
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
                    self.folder_row_menu(&response, index, row);
                    if is_focused {
                        ui.painter().rect_stroke(
                            response.rect,
                            0.0,
                            style::focused_row_stroke(),
                            StrokeKind::Inside,
                        );
                    }
                    if let Some(parent) = inline_parent.as_ref() {
                        if parent == &row.path && !inline_rendered {
                            let row_width = ui.available_width();
                            self.render_inline_new_folder_row(
                                ui,
                                row.depth + 1,
                                row_width,
                                row_height,
                            );
                            inline_rendered = true;
                        }
                    }
                }
                if hovered_folder.is_none() {
                    let pointer = pointer_pos.unwrap_or_default();
                    self.controller.update_active_drag(
                        pointer,
                        DragSource::Folders,
                        DragTarget::FolderPanel { folder: None },
                    );
                }
            });
        });
        if sample_drag_active && let Some(pointer) = pointer_pos {
            if frame_response.response.rect.contains(pointer) {
                if hovered_folder.is_none() {
                    self.controller.update_active_drag(
                        pointer,
                        DragSource::Folders,
                        DragTarget::FolderPanel { folder: None },
                    );
                }
            } else {
                self.controller
                    .update_active_drag(pointer, DragSource::Folders, DragTarget::None);
            }
        }
        style::paint_section_border(ui, frame_response.response.rect, focused);
        self.controller.ui.sources.folders.scroll_to = None;
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
                        RichText::new(format!("• {}", path.display())).color(palette.text_primary),
                    );
                }
            });
    }

    fn render_folder_rename_editor(
        &mut self,
        ui: &mut Ui,
        row_response: &egui::Response,
        row: &crate::egui_app::state::FolderRowView,
    ) {
        let Some(prompt) = self.controller.ui.sources.folders.pending_action.as_mut() else {
            return;
        };
        let name = match prompt {
            crate::egui_app::state::FolderActionPrompt::Rename { name, .. } => name,
        };
        let padding = ui.spacing().button_padding.x;
        let indent = row.depth as f32 * 12.0;
        let mut edit_rect = row_response.rect;
        edit_rect.min.x += padding + indent + 14.0;
        edit_rect.max.x -= padding;
        edit_rect.min.y += 2.0;
        edit_rect.max.y -= 2.0;
        let edit = egui::TextEdit::singleline(name)
            .hint_text("Rename folder")
            .frame(false)
            .desired_width(edit_rect.width());
        let response = ui.put(edit_rect, edit);
        if self.controller.ui.sources.folders.rename_focus_requested && !response.has_focus() {
            response.request_focus();
            self.controller.ui.sources.folders.rename_focus_requested = false;
        }
        let enter = response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let escape = response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape));
        if enter {
            self.apply_pending_folder_rename();
        } else if escape || response.lost_focus() {
            self.controller.cancel_folder_rename();
        }
    }

    fn render_inline_new_folder_row(
        &mut self,
        ui: &mut Ui,
        depth: usize,
        row_width: f32,
        row_height: f32,
    ) {
        let Some(inline) = self.controller.ui.sources.folders.new_folder.as_mut() else {
            return;
        };
        let response = render_list_row(
            ui,
            "",
            row_width,
            row_height,
            Some(style::row_selected_fill()),
            style::high_contrast_text(),
            egui::Sense::hover(),
            None,
            None,
        );
        let padding = ui.spacing().button_padding.x;
        let indent = depth as f32 * 12.0;
        let mut edit_rect = response.rect;
        edit_rect.min.x += padding + indent + 14.0;
        edit_rect.max.x -= padding;
        edit_rect.min.y += 2.0;
        edit_rect.max.y -= 2.0;
        let edit = egui::TextEdit::singleline(&mut inline.name)
            .hint_text("New folder name")
            .frame(false)
            .desired_width(edit_rect.width());
        let edit_response = ui.put(edit_rect, edit);
        if inline.focus_requested && !edit_response.has_focus() {
            edit_response.request_focus();
            inline.focus_requested = false;
        }
        let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));
        let escape_pressed = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if enter_pressed && (edit_response.has_focus() || edit_response.lost_focus()) {
            self.apply_pending_folder_creation();
        } else if escape_pressed || (edit_response.lost_focus() && !enter_pressed) {
            self.controller.cancel_new_folder_creation();
        }
    }

    fn apply_pending_folder_rename(&mut self) {
        let action = self.controller.ui.sources.folders.pending_action.clone();
        if let Some(crate::egui_app::state::FolderActionPrompt::Rename { target, name }) = action {
            match self.controller.rename_folder(&target, &name) {
                Ok(()) => {
                    self.controller.cancel_folder_rename();
                }
                Err(err) => self.controller.set_status(err, style::StatusTone::Error),
            }
        }
    }

    fn apply_pending_folder_creation(&mut self) {
        let inline = self.controller.ui.sources.folders.new_folder.clone();
        if let Some(state) = inline {
            match self.controller.create_folder(&state.parent, &state.name) {
                Ok(()) => self.controller.ui.sources.folders.new_folder = None,
                Err(err) => self.controller.set_status(err, style::StatusTone::Error),
            }
        }
    }

    fn folder_row_menu(
        &mut self,
        response: &egui::Response,
        index: usize,
        row: &crate::egui_app::state::FolderRowView,
    ) {
        response.context_menu(|ui| {
            let palette = style::palette();
            ui.label(RichText::new(row.name.clone()).color(palette.text_primary));
            ui.separator();
            let mut close_menu = false;
            if ui.button("New subfolder").clicked() {
                self.controller.focus_folder_row(index);
                self.controller.start_new_folder();
                close_menu = true;
            }
            if ui.button("Rename").clicked() {
                self.controller.focus_folder_row(index);
                self.controller.start_folder_rename();
                close_menu = true;
            }
            let delete_button = egui::Button::new(
                RichText::new("Delete")
                    .color(style::destructive_text())
                    .strong(),
            );
            if ui.add(delete_button).clicked() {
                self.controller.focus_folder_row(index);
                self.controller.delete_focused_folder();
                close_menu = true;
            }
            if close_menu {
                ui.close();
            }
        });
    }

    fn root_row_menu(&mut self, response: &egui::Response) {
        response.context_menu(|ui| {
            let palette = style::palette();
            ui.label(RichText::new(".").color(palette.text_primary));
            ui.separator();
            if ui.button("New folder at root").clicked() {
                self.controller.start_new_folder_at_root();
                ui.close();
            }
        });
    }
}

fn folder_row_label(
    row: &crate::egui_app::state::FolderRowView,
    row_width: f32,
    ui: &Ui,
) -> String {
    if row.is_root {
        return ".".to_string();
    }
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
