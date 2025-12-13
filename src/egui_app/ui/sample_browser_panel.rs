use super::flat_items_list::{FlatItemsListConfig, render_flat_items_list};
use super::helpers::{NumberColumn, RowMarker, clamp_label_for_width, render_list_row};
use super::style;
use super::*;
use crate::egui_app::state::{
    DragSource, DragTarget, FocusContext, SampleBrowserActionPrompt, TriageFlagFilter,
};
use crate::egui_app::ui::style::StatusTone;
use crate::egui_app::view_model;
use eframe::egui::{self, RichText, StrokeKind, Ui};
use std::path::Path;

impl EguiApp {
    pub(super) fn render_sample_browser(&mut self, ui: &mut Ui) {
        let palette = style::palette();
        let selected_row = self.controller.ui.browser.selected_visible;
        let loaded_row = self.controller.ui.browser.loaded_visible;
        let drop_target = self.controller.triage_flag_drop_target();
        self.render_sample_browser_filter(ui);
        ui.add_space(6.0);

        let list_height = ui.available_height().max(0.0);
        let drag_active = self.controller.ui.drag.payload.is_some();
        let pointer_pos = ui
            .input(|i| i.pointer.hover_pos().or_else(|| i.pointer.interact_pos()))
            .or(self.controller.ui.drag.position);
        let autoscroll_enabled = self.controller.ui.browser.autoscroll
            && self.controller.ui.collections.selected_sample.is_none();
        let total_rows = self.controller.visible_browser_indices().len();
        let focused_section = matches!(
            self.controller.ui.focus.context,
            FocusContext::SampleBrowser
        );
        let autoscroll_to = selected_row.filter(|_| autoscroll_enabled);

        let list_response = render_flat_items_list(
            ui,
            FlatItemsListConfig {
                scroll_id_salt: "sample_browser_scroll",
                min_height: list_height,
                total_rows,
                focused_section,
                autoscroll_to,
                autoscroll_padding_rows: 1.0,
            },
            |ui, row, metrics| {
                let entry_index = {
                    let indices = self.controller.visible_browser_indices();
                    match indices.get(row) {
                        Some(index) => *index,
                        None => return,
                    }
                };
                let (tag, path, missing) = match self.controller.wav_entry(entry_index) {
                    Some(entry) => (entry.tag, entry.relative_path.clone(), entry.missing),
                    None => return,
                };
                let rename_match = matches!(
                    self.controller.ui.browser.pending_action,
                    Some(SampleBrowserActionPrompt::Rename { ref target, .. })
                        if target == &path
                );
                let is_focused = selected_row == Some(row);
                let is_selected = self
                    .controller
                    .ui
                    .browser
                    .selected_paths
                    .iter()
                    .any(|p| p == &path);
                let is_loaded = loaded_row == Some(row);
                let row_width = ui.available_width();
                let triage_marker = style::triage_marker_color(tag).map(|color| RowMarker {
                    width: style::triage_marker_width(),
                    color,
                });
                let trailing_space = triage_marker
                    .as_ref()
                    .map(|marker| marker.width + metrics.padding * 0.5)
                    .unwrap_or(0.0);

                let mut label = self
                    .controller
                    .wav_label(entry_index)
                    .unwrap_or_else(|| view_model::sample_display_label(&path));
                if is_loaded {
                    label.push_str(" • loaded");
                }
                if missing {
                    label = format!("! {label}");
                }
                let display_label = label.clone();

                let row_label_width = row_width
                    - metrics.padding
                    - metrics.number_width
                    - metrics.number_gap
                    - trailing_space;
                let row_label = clamp_label_for_width(&label, row_label_width);
                let bg = if drag_active
                    && pointer_pos
                        .as_ref()
                        .is_some_and(|pos| ui.cursor().contains(*pos))
                    && is_selected
                {
                    Some(style::duplicate_hover_fill())
                } else if is_focused {
                    Some(style::row_selected_fill())
                } else if is_selected {
                    Some(style::row_multi_selected_fill())
                } else {
                    None
                };
                let number_text = format!("{}", row + 1);
                let text_color = if missing {
                    style::missing_text()
                } else {
                    style::triage_label_color(tag)
                };

                ui.push_id(&path, |ui| {
                    let sense = if rename_match {
                        egui::Sense::hover()
                    } else {
                        egui::Sense::click_and_drag()
                    };
                    let response = render_list_row(
                        ui,
                        super::helpers::ListRow {
                            label: &row_label,
                            row_width,
                            row_height: metrics.row_height,
                            bg,
                            text_color,
                            sense,
                            number: Some(NumberColumn {
                                text: &number_text,
                                width: metrics.number_width,
                                color: palette.text_muted,
                            }),
                            marker: triage_marker,
                        },
                    );

                    if is_selected {
                        let marker_width = 4.0;
                        let marker_rect = egui::Rect::from_min_max(
                            response.rect.left_top(),
                            response.rect.left_top()
                                + egui::vec2(marker_width, metrics.row_height),
                        );
                        ui.painter()
                            .rect_filled(marker_rect, 0.0, style::selection_marker_fill());
                    }
                    if response.clicked() {
                        let modifiers = ui.input(|i| i.modifiers);
                        let ctrl = modifiers.command || modifiers.ctrl;
                        if modifiers.shift && ctrl {
                            self.controller.add_range_browser_selection(row);
                        } else if modifiers.shift {
                            self.controller.extend_browser_selection_to_row(row);
                        } else if ctrl {
                            self.controller.toggle_browser_row_selection(row);
                        } else {
                            self.controller.clear_browser_selection();
                            self.controller.focus_browser_row_only(row);
                        }
                    }
                    if is_focused {
                        ui.painter().rect_stroke(
                            response.rect,
                            0.0,
                            style::focused_row_stroke(),
                            StrokeKind::Inside,
                        );
                    }

                    if rename_match {
                        self.render_browser_rename_editor(
                            ui,
                            &response,
                            metrics.padding,
                            metrics.number_width,
                            metrics.number_gap,
                            trailing_space,
                        );
                    } else {
                        self.browser_sample_menu(&response, row, &path, &display_label);
                    }

                    if response.drag_started() {
                        if let Some(pos) = response.interact_pointer_pos() {
                            if let Some(source) = self.controller.current_source() {
                                let name = view_model::sample_display_label(&path);
                                self.controller.start_sample_drag(
                                    source.id.clone(),
                                    path.clone(),
                                    name,
                                    pos,
                                );
                            } else {
                                self.controller.set_status(
                                    "Select a source before dragging",
                                    StatusTone::Warning,
                                );
                            }
                        }
                    } else if drag_active && response.dragged() {
                        if let Some(pos) = response.interact_pointer_pos() {
                            let shift_down = ui.input(|i| i.modifiers.shift);
                            self.controller.update_active_drag(
                                pos,
                                DragSource::Browser,
                                DragTarget::BrowserTriage(drop_target),
                                shift_down,
                            );
                        }
                    } else if response.drag_stopped() {
                        self.controller.finish_active_drag();
                    }
                });
            },
        );

        if autoscroll_to.is_some() {
            self.controller.ui.browser.autoscroll = false;
        }

        if drag_active
            && let Some(pointer) = pointer_pos
            && list_response.frame_rect.contains(pointer)
        {
            let shift_down = ui.input(|i| i.modifiers.shift);
            self.controller.update_active_drag(
                pointer,
                DragSource::Browser,
                DragTarget::BrowserTriage(drop_target),
                shift_down,
            );
            ui.painter().rect_stroke(
                list_response.frame_rect,
                0.0,
                style::drag_target_stroke(),
                StrokeKind::Inside,
            );
        }
    }

    fn browser_sample_menu(
        &mut self,
        response: &egui::Response,
        row: usize,
        path: &Path,
        label: &str,
    ) {
        response.context_menu(|ui| {
            let palette = style::palette();
            let mut close_menu = false;
            let action_rows = self.controller.action_rows_from_primary(row);
            ui.label(RichText::new(label.to_string()).color(palette.text_primary));
            if ui.button("Open in file explorer").clicked() {
                self.controller
                    .reveal_browser_sample_in_file_explorer(path);
                close_menu = true;
            }
            ui.separator();
            self.sample_tag_menu(ui, &mut close_menu, |app, tag| {
                app.controller
                    .tag_browser_samples(&action_rows, tag, row)
                    .is_ok()
            });
            if ui
                .button("Normalize (overwrite)")
                .on_hover_text("Scale to full range and overwrite the wav")
                .clicked()
                && self
                    .controller
                    .normalize_browser_samples(&action_rows)
                    .is_ok()
            {
                close_menu = true;
            }
            let default_name = view_model::sample_display_label(path);
            let rename_id = ui.make_persistent_id(format!("rename:triage:{}", path.display()));
            if self.sample_rename_controls(ui, rename_id, default_name.as_str(), |app, value| {
                app.controller.rename_browser_sample(row, value).is_ok()
            }) {
                close_menu = true;
            }
            let delete_btn =
                egui::Button::new(RichText::new("Delete file").color(style::destructive_text()));
            if ui.add(delete_btn).clicked()
                && self.controller.delete_browser_samples(&action_rows).is_ok()
            {
                close_menu = true;
            }
            if close_menu {
                ui.close();
            }
        });
    }

    fn render_browser_rename_editor(
        &mut self,
        ui: &mut Ui,
        row_response: &egui::Response,
        padding: f32,
        number_width: f32,
        number_gap: f32,
        trailing_space: f32,
    ) {
        let Some(prompt) = self.controller.ui.browser.pending_action.as_mut() else {
            return;
        };
        let name = match prompt {
            SampleBrowserActionPrompt::Rename { name, .. } => name,
        };
        let mut edit_rect = row_response.rect;
        edit_rect.min.x += number_width + number_gap + padding;
        edit_rect.max.x -= padding + trailing_space;
        edit_rect.min.y += 2.0;
        edit_rect.max.y -= 2.0;
        let response = ui.put(
            edit_rect,
            egui::TextEdit::singleline(name)
                .hint_text("Rename sample")
                .frame(false)
                .desired_width(edit_rect.width()),
        );
        if self.controller.ui.browser.rename_focus_requested && !response.has_focus() {
            response.request_focus();
            self.controller.ui.browser.rename_focus_requested = false;
        }
        let enter = response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let escape = response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape));
        if enter {
            self.controller.apply_pending_browser_rename();
        } else if escape || response.lost_focus() {
            self.controller.cancel_browser_rename();
        }
    }

    fn render_sample_browser_filter(&mut self, ui: &mut Ui) {
        let palette = style::palette();
        let visible_count = self.controller.visible_browser_indices().len();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Filter").color(palette.text_primary));
            for filter in [
                TriageFlagFilter::All,
                TriageFlagFilter::Keep,
                TriageFlagFilter::Trash,
                TriageFlagFilter::Untagged,
            ] {
                let selected = self.controller.ui.browser.filter == filter;
                let label = match filter {
                    TriageFlagFilter::All => "All",
                    TriageFlagFilter::Keep => "Keep",
                    TriageFlagFilter::Trash => "Trash",
                    TriageFlagFilter::Untagged => "Untagged",
                };
                if ui.selectable_label(selected, label).clicked() {
                    self.controller.set_browser_filter(filter);
                }
            }
            ui.add_space(ui.spacing().item_spacing.x);
            let mut query = self.controller.ui.browser.search_query.clone();
            let response = ui.add(
                egui::TextEdit::singleline(&mut query)
                    .hint_text("Search samples (f)...")
                    .desired_width(160.0),
            );
            if self.controller.ui.browser.search_focus_requested {
                response.request_focus();
                self.controller.ui.browser.search_focus_requested = false;
            }
            if response.changed() {
                self.controller.set_browser_search(query);
            }
            ui.add_space(ui.spacing().item_spacing.x);
            let random_mode_enabled = self.controller.random_navigation_mode_enabled();
            let dice_label = RichText::new("🎲").color(if random_mode_enabled {
                palette.text_primary
            } else {
                palette.text_muted
            });
            let dice_button = egui::Button::new(dice_label).selected(random_mode_enabled);
            let dice_response = ui.add(dice_button).on_hover_text(
                "Play a random visible sample (click)\nToggle sticky random navigation (Shift+click)",
            );
            if dice_response.clicked() {
                let modifiers = ui.input(|i| i.modifiers);
                if modifiers.shift {
                    self.controller.toggle_random_navigation_mode();
                } else {
                    self.controller.play_random_visible_sample();
                }
            }
            let count_label = format!(
                "{} item{}",
                visible_count,
                if visible_count == 1 { "" } else { "s" }
            );
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), 0.0),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    ui.label(RichText::new(count_label).color(palette.text_muted).small());
                },
            );
        });
    }
}
