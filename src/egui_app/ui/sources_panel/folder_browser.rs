use super::EguiApp;
use super::helpers::{
    NumberColumn, RowBackground, external_dropped_paths, external_hover_has_audio, list_row_height,
    number_column_width, render_list_row,
};
use super::style;
use super::utils::{folder_row_label, sample_housing_folders};
use crate::egui_app::controller::hotkeys;
use crate::egui_app::state::{DragSource, DragTarget, FocusContext, RootFolderFilterMode};
use eframe::egui::{self, Align, Align2, Layout, RichText, StrokeKind, TextStyle, Ui};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

impl EguiApp {
    pub(super) fn render_folder_browser(
        &mut self,
        ui: &mut Ui,
        height: f32,
        folder_drop_active: bool,
        pointer_pos: Option<egui::Pos2>,
    ) {
        let pointer_pos = pointer_pos.or(self.external_drop_hover_pos);
        let external_drop_paths = external_dropped_paths(ui.ctx());
        let mut external_drop_paths = if external_drop_paths.is_empty() {
            None
        } else {
            Some(external_drop_paths)
        };
        let external_drop_has_audio = external_drop_paths.as_ref().is_some_and(|paths| {
            paths
                .iter()
                .any(|path| path.is_file() && crate::sample_sources::is_supported_audio(path))
        });
        if !external_drop_has_audio {
            external_drop_paths = None;
        }
        let external_drop_ready = external_hover_has_audio(ui.ctx());
        let mut external_drop_consumed = false;
        self.controller
            .refresh_folder_browser_if_stale(Duration::from_millis(750));
        let mut sample_parent_folders = HashSet::<PathBuf>::new();
        for path in self.controller.ui.browser.selected_paths.iter() {
            sample_parent_folders.extend(sample_housing_folders(path));
        }
        if let Some(selected_row) = self.controller.ui.browser.selected_visible {
            if let Some(entry_index) = self.controller.visible_browser_index(selected_row)
                && let Some(entry) = self.controller.wav_entry(entry_index)
            {
                sample_parent_folders.extend(sample_housing_folders(&entry.relative_path));
            }
        }

        let palette = style::palette();
        let header_response = ui.horizontal(|ui| {
            ui.label(RichText::new("Folders").color(palette.text_primary));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let mut query = self.controller.ui.sources.folders.search_query.clone();
                let search_hint = format!(
                    "Search folders ({})...",
                    hotkeys::format_keypress(&hotkeys::KeyPress::with_command(egui::Key::F))
                );
                let response = ui.add(
                    egui::TextEdit::singleline(&mut query)
                        .hint_text(search_hint)
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
        self.controller.ui.sources.folders.header_height = header_response.response.rect.height();
        let header_gap = ui.spacing().item_spacing.y;
        ui.add_space(header_gap);
        let content_height = (height - header_response.response.rect.height() - header_gap).max(0.0);
        let frame = style::section_frame();
        let focused = matches!(
            self.controller.ui.focus.context,
            FocusContext::SourceFolders
        );
        let scroll_to = self.controller.ui.sources.folders.scroll_to;
        let mut hovered_folder = None;
        let mut rows = std::mem::take(&mut self.controller.ui.sources.folders.rows);
        let root_row = rows.first().filter(|row| row.is_root);
        let has_folder_rows = rows.iter().any(|row| !row.is_root);
        let show_hotkey_column = rows.iter().any(|row| row.hotkey.is_some());
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
            ui.set_min_height(content_height);
            ui.set_max_height(content_height);
            let row_height = list_row_height(ui);
            let hotkey_width = if show_hotkey_column {
                number_column_width(10, ui)
            } else {
                0.0
            };
            let active_folder_target = match &self.controller.ui.drag.active_target {
                DragTarget::FolderPanel { folder } => folder
                .clone()
                .or_else(|| self.controller.ui.drag.last_folder_target.clone()),
                _ => None,
            };
            if let Some(root_row) = root_row {
                let row_width = ui.available_width();
                let label_width = if show_hotkey_column {
                    row_width - hotkey_width
                } else {
                    row_width
                };
                let is_focused = self.controller.ui.sources.folders.focused == Some(0);
                let is_selected = root_row.selected;
                let bg = RowBackground::from_option(if is_focused {
                    Some(style::row_primary_selection_fill())
                } else if is_selected {
                    Some(style::row_secondary_selection_fill())
                } else {
                    None
                });
                let label = folder_row_label(&root_row, label_width, ui);
                let hotkey_text = root_row
                    .hotkey
                    .map(|key| key.to_string())
                    .unwrap_or_default();
                let response = render_list_row(
                    ui,
                    super::helpers::ListRow {
                        label: &label,
                        row_width,
                        row_height,
                        background: bg,
                        skip_hover: false,
                        text_color: style::high_contrast_text(),
                        sense: egui::Sense::click_and_drag(),
                        number: show_hotkey_column.then_some(NumberColumn {
                            text: hotkey_text.as_str(),
                            width: hotkey_width,
                            color: style::palette().text_muted,
                        }),
                        marker: None,
                        rating: None,
                        looped: false,
                        long_sample: false,
                        bpm_label: None,
                    },
                );
                let mut badge_offset = 0.0;
                if is_selected {
                    if let Some(mode) = root_row.root_filter_mode {
                        badge_offset = paint_root_filter_badge(ui, response.rect, mode, root_row.negated);
                    }
                }
                if sample_parent_folders.contains(&root_row.path) {
                    let offset = badge_offset + if root_row.negated { 8.0 } else { 0.0 };
                    paint_right_side_dot(ui, response.rect, offset);
                }
                if root_row.negated {
                    paint_negation_marker(ui, response.rect);
                }
                if is_selected {
                    let marker_width = 4.0;
                    let marker_rect = egui::Rect::from_min_max(
                        response.rect.left_top(),
                        response.rect.left_top() + egui::vec2(marker_width, row_height),
                    );
                    ui.painter()
                        .rect_filled(marker_rect, 0.0, style::selection_marker_fill());
                }
                if scroll_to == Some(0) {
                    ui.scroll_to_rect(response.rect, None);
                }
                if !external_drop_consumed
                    && let Some(pointer) = pointer_pos
                    && response.rect.contains(pointer)
                    && let Some(paths) = external_drop_paths.take()
                {
                    external_drop_consumed = true;
                    self.external_drop_handled = true;
                    self.controller
                        .import_external_files_to_source_folder(root_row.path.clone(), paths);
                }
                if folder_drop_active {
                    if let Some(pointer) = pointer_pos
                        && response.rect.contains(pointer)
                    {
                        hovered_folder = Some(root_row.path.clone());
                        let shift_down = ui.input(|i| i.modifiers.shift);
                        let alt_down = ui.input(|i| i.modifiers.alt);
                        self.controller.update_active_drag(
                            pointer,
                            DragSource::Folders,
                            DragTarget::FolderPanel {
                                folder: Some(root_row.path.clone()),
                            },
                            shift_down,
                            alt_down,
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
                if external_drop_ready
                    && pointer_pos.is_some_and(|pos| response.rect.contains(pos))
                {
                    ui.painter().rect_stroke(
                        response.rect.expand(2.0),
                        0.0,
                        style::drag_target_stroke(),
                        StrokeKind::Inside,
                    );
                }
                if response.clicked() {
                    let modifiers = ui.input(|i| i.modifiers);
                    if modifiers.alt {
                        self.controller.toggle_folder_row_negation(0);
                    } else if modifiers.shift {
                        self.controller.select_folder_range(0);
                    } else if modifiers.command || modifiers.ctrl {
                        self.controller.toggle_folder_row_selection(0);
                    } else {
                        self.controller.replace_folder_selection(0);
                    }
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
                .max_height(content_height);
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
                    let response = ui.interact(rect, ui.id().with("folder_empty_row"), egui::Sense::click());
                    self.root_row_menu(&response);
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
                    let bg = RowBackground::from_option(if is_focused {
                        Some(style::row_primary_selection_fill())
                    } else if row.selected {
                        Some(style::row_secondary_selection_fill())
                    } else {
                        None
                    });
                    let row_width = ui.available_width();
                    let label_width = if show_hotkey_column {
                        row_width - hotkey_width
                    } else {
                        row_width
                    };
                    let label = if rename_match {
                        String::new()
                    } else {
                        folder_row_label(row, label_width, ui)
                    };
                    let sense = if rename_match {
                        egui::Sense::hover()
                    } else {
                        egui::Sense::click_and_drag()
                    };
                    let hotkey_text = row.hotkey.map(|key| key.to_string()).unwrap_or_default();
                    let response = render_list_row(
                        ui,
                        super::helpers::ListRow {
                            label: &label,
                            row_width,
                            row_height,
                            background: bg,
                            skip_hover: false,
                            text_color: style::high_contrast_text(),
                            sense,
                            number: show_hotkey_column.then_some(NumberColumn {
                                text: hotkey_text.as_str(),
                                width: hotkey_width,
                                color: style::palette().text_muted,
                            }),
                            marker: None,
                            rating: None,
                            looped: false,
                            long_sample: false,
                            bpm_label: None,
                        },
                    );
                    let started_drag = if !rename_match
                        && self.controller.ui.drag.payload.is_none()
                        && (response.drag_started() || response.dragged())
                    {
                        if let Some(pos) = response.interact_pointer_pos() {
                            if let Some(source) = self.controller.current_source() {
                                self.controller.ui.drag.pending_os_drag = None;
                                self.controller.start_folder_drag(
                                    source.id.clone(),
                                    row.path.clone(),
                                    row.name.clone(),
                                    pos,
                                );
                            } else {
                                self.controller.set_status(
                                    "Select a source before dragging",
                                    style::StatusTone::Warning,
                                );
                            }
                        }
                        true
                    } else {
                        false
                    };
                    if sample_parent_folders.contains(&row.path) {
                        let offset = if row.negated { 8.0 } else { 0.0 };
                        paint_right_side_dot(ui, response.rect, offset);
                    }
                    if row.negated {
                        paint_negation_marker(ui, response.rect);
                    }
                    if Some(index) == scroll_to {
                        ui.scroll_to_rect(response.rect, None);
                    }
                    if !external_drop_consumed
                        && let Some(pointer) = pointer_pos
                        && response.rect.contains(pointer)
                        && let Some(paths) = external_drop_paths.take()
                    {
                        external_drop_consumed = true;
                        self.external_drop_handled = true;
                        self.controller
                            .import_external_files_to_source_folder(row.path.clone(), paths);
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
                    if folder_drop_active {
                        if let Some(pointer) = pointer_pos
                            && response.rect.contains(pointer)
                        {
                            hovered_folder = Some(row.path.clone());
                            let shift_down = ui.input(|i| i.modifiers.shift);
                            let alt_down = ui.input(|i| i.modifiers.alt);
                            self.controller.update_active_drag(
                                pointer,
                                DragSource::Folders,
                                DragTarget::FolderPanel {
                                    folder: Some(row.path.clone()),
                                },
                                shift_down,
                                alt_down,
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
                    if external_drop_ready
                        && pointer_pos.is_some_and(|pos| response.rect.contains(pos))
                    {
                        ui.painter().rect_stroke(
                            response.rect.expand(2.0),
                            0.0,
                            style::drag_target_stroke(),
                            StrokeKind::Inside,
                        );
                    }
                    if rename_match {
                        self.render_folder_rename_editor(ui, &response, row);
                    } else if !started_drag && response.clicked() {
                        let pointer = response.interact_pointer_pos();
                        let hit_expand = row.has_children
                            && pointer.is_some_and(|pos| {
                                let padding = ui.spacing().button_padding.x;
                                let indent = row.depth as f32 * 12.0;
                                pos.x <= response.rect.left() + padding + indent + 14.0
                            });
                        let modifiers = ui.input(|i| i.modifiers);
                        if modifiers.alt {
                            self.controller.toggle_folder_row_negation(index);
                        } else if hit_expand {
                            self.controller.toggle_folder_expanded(index);
                        } else if modifiers.shift {
                            self.controller.select_folder_range(index);
                        } else if modifiers.command || modifiers.ctrl {
                            self.controller.toggle_folder_row_selection(index);
                        } else {
                            self.controller.replace_folder_selection(index);
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
                if folder_drop_active && hovered_folder.is_none() {
                    if let Some(pointer) = pointer_pos {
                        let shift_down = ui.input(|i| i.modifiers.shift);
                        let alt_down = ui.input(|i| i.modifiers.alt);
                        self.controller.update_active_drag(
                            pointer,
                            DragSource::Folders,
                            DragTarget::FolderPanel { folder: None },
                            shift_down,
                            alt_down,
                        );
                    }
                }
                let empty_height = ui.available_height();
                if empty_height > 0.0 {
                    let response = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), empty_height),
                        egui::Sense::click(),
                    ).1;
                    self.root_row_menu(&response);
                }
            });
        });
        if folder_drop_active && let Some(pointer) = pointer_pos {
            if frame_response.response.rect.contains(pointer) {
                if hovered_folder.is_none() {
                    let shift_down = ui.input(|i| i.modifiers.shift);
                    let alt_down = ui.input(|i| i.modifiers.alt);
                    self.controller.update_active_drag(
                        pointer,
                        DragSource::Folders,
                        DragTarget::FolderPanel { folder: None },
                        shift_down,
                        alt_down,
                    );
                }
            } else {
                let shift_down = ui.input(|i| i.modifiers.shift);
                let alt_down = ui.input(|i| i.modifiers.alt);
                self.controller.update_active_drag(
                    pointer,
                    DragSource::Folders,
                    DragTarget::None,
                    shift_down,
                    alt_down,
                );
            }
        }
        if external_drop_ready
            && pointer_pos.is_some_and(|pos| frame_response.response.rect.contains(pos))
        {
            ui.painter().rect_stroke(
                frame_response.response.rect,
                6.0,
                style::drag_target_stroke(),
                StrokeKind::Inside,
            );
        }
        style::paint_section_border(ui, frame_response.response.rect, focused);
        self.controller.ui.sources.folders.scroll_to = None;
        self.controller.ui.sources.folders.rows = rows;
    }
}

fn paint_right_side_dot(ui: &mut Ui, rect: egui::Rect, offset_x: f32) {
    let padding = ui.spacing().button_padding.x;
    let radius = 3.0;
    let center = egui::pos2(rect.right() - padding - radius - offset_x, rect.center().y);
    ui.painter()
        .circle_filled(center, radius, egui::Color32::WHITE);
}

fn paint_negation_marker(ui: &mut Ui, rect: egui::Rect) {
    let padding = ui.spacing().button_padding.x;
    let width = 4.0;
    let marker_rect = egui::Rect::from_min_max(
        egui::pos2(rect.right() - padding - width, rect.top() + 2.0),
        egui::pos2(rect.right() - padding, rect.bottom() - 2.0),
    );
    ui.painter()
        .rect_filled(marker_rect, 1.0, style::destructive_text());
}

fn paint_root_filter_badge(
    ui: &mut Ui,
    rect: egui::Rect,
    mode: RootFolderFilterMode,
    negated: bool,
) -> f32 {
    let palette = style::palette();
    let (label, color) = match mode {
        RootFolderFilterMode::AllDescendants => ("ALL", palette.accent_mint),
        RootFolderFilterMode::RootOnly => ("ROOT", palette.accent_copper),
    };
    let font_id = TextStyle::Button.resolve(ui.style());
    let galley = ui.ctx().fonts_mut(|fonts| {
        fonts.layout_no_wrap(label.to_string(), font_id.clone(), color)
    });
    let padding = ui.spacing().button_padding.x;
    let dot_radius = 3.0;
    let dot_gap = 6.0;
    let negation_offset = if negated { 8.0 } else { 0.0 };
    let right_edge = rect.right() - padding - negation_offset;
    let text_pos = egui::pos2(right_edge, rect.center().y);
    ui.painter()
        .text(text_pos, Align2::RIGHT_CENTER, label, font_id, color);
    let text_left = right_edge - galley.size().x;
    let dot_center = egui::pos2(text_left - dot_gap - dot_radius, rect.center().y);
    ui.painter()
        .circle_filled(dot_center, dot_radius, color);
    galley.size().x + dot_gap + dot_radius * 2.0 + negation_offset + 6.0
}
