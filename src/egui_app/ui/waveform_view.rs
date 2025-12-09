use super::style;
use super::*;
use crate::selection::SelectionEdge;
use eframe::egui::{
    self, Align2, Color32, CursorIcon, Frame, Margin, RichText, Stroke, StrokeKind, TextStyle,
    TextureOptions, Ui, text::LayoutJob,
};

impl EguiApp {
    pub(super) fn render_waveform(&mut self, ui: &mut Ui) {
        let palette = style::palette();
        let highlight = palette.accent_copper;
        let frame = Frame::new()
            .fill(style::compartment_fill())
            .stroke(style::outer_border())
            .inner_margin(Margin::symmetric(10, 6));
        let frame_response = frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Waveform Viewer").color(palette.text_primary));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let loop_enabled = self.controller.ui.waveform.loop_enabled;
                    let text = if loop_enabled { "Loop on" } else { "Loop off" };
                    let button = egui::Button::new(RichText::new(text).color(palette.text_primary));
                    if ui.add(button).clicked() {
                        self.controller.toggle_loop();
                    }
                });
            });
            ui.add_space(8.0);
            let desired = egui::vec2(ui.available_width(), 260.0);
            let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::click_and_drag());
            let target_width = rect.width().round().max(1.0) as u32;
            let target_height = rect.height().round().max(1.0) as u32;
            self.controller
                .update_waveform_size(target_width, target_height);
            let painter = ui.painter();
            let pointer_pos = response.hover_pos();
            let view = self.controller.ui.waveform.view;
            let view_width = view.width();
            let to_screen_x = |position: f32, rect: egui::Rect| {
                let normalized = ((position - view.start) / view_width).clamp(0.0, 1.0);
                rect.left() + rect.width() * normalized
            };
            if let Some(message) = self.controller.ui.waveform.notice.as_ref() {
                painter.rect_filled(rect, 0.0, palette.bg_primary);
                painter.rect_stroke(
                    rect,
                    0.0,
                    Stroke::new(2.0, palette.panel_outline),
                    StrokeKind::Inside,
                );
                let font = TextStyle::Heading.resolve(ui.style());
                painter.text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    message,
                    font,
                    style::missing_text(),
                );
                return;
            }
            let tex_id = if let Some(image) = &self.controller.ui.waveform.image {
                let new_size = image.image.size;
                if let Some(tex) = self.waveform_tex.as_mut() {
                    if tex.size() == new_size {
                        tex.set(image.image.clone(), TextureOptions::LINEAR);
                        Some(tex.id())
                    } else {
                        let tex = ui.ctx().load_texture(
                            "waveform_texture",
                            image.image.clone(),
                            TextureOptions::LINEAR,
                        );
                        let id = tex.id();
                        self.waveform_tex = Some(tex);
                        Some(id)
                    }
                } else {
                    let tex = ui.ctx().load_texture(
                        "waveform_texture",
                        image.image.clone(),
                        TextureOptions::LINEAR,
                    );
                    let id = tex.id();
                    self.waveform_tex = Some(tex);
                    Some(id)
                }
            } else {
                self.waveform_tex = None;
                None
            };

            if let Some(id) = tex_id {
                let uv = egui::Rect::from_min_max(
                    egui::pos2(view.start, 0.0),
                    egui::pos2(view.end, 1.0),
                );
                painter.image(id, rect, uv, style::high_contrast_text());
            } else {
                painter.rect_filled(rect, 0.0, palette.bg_primary);
            }
            painter.rect_stroke(
                rect,
                0.0,
                Stroke::new(2.0, palette.panel_outline),
                StrokeKind::Inside,
            );

            if let Some(pos) = pointer_pos.filter(|p| rect.contains(*p)) {
                let x = pos.x;
                painter.line_segment(
                    [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                    Stroke::new(1.0, highlight),
                );
            }

            let mut edge_dragging = false;
            if let Some(selection) = self.controller.ui.waveform.selection {
                let start_norm = ((selection.start() - view.start) / view_width).clamp(0.0, 1.0);
                let end_norm = ((selection.end() - view.start) / view_width).clamp(0.0, 1.0);
                let width = rect.width() * (end_norm - start_norm).max(0.0);
                let x = rect.left() + rect.width() * start_norm;
                let selection_rect = egui::Rect::from_min_size(
                    egui::pos2(x, rect.top()),
                    egui::vec2(width, rect.height()),
                );
                painter.rect_filled(selection_rect, 0.0, style::with_alpha(highlight, 60));
                let handle_rect = selection_handle_rect(selection_rect);
                let handle_response = ui.interact(
                    handle_rect,
                    ui.id().with("selection_handle"),
                    egui::Sense::drag(),
                );
                let handle_hovered = handle_response.hovered() || handle_response.dragged();
                let handle_color = if handle_hovered {
                    style::with_alpha(highlight, 235)
                } else {
                    style::with_alpha(highlight, 205)
                };
                painter.rect_filled(handle_rect, 0.0, handle_color);
                if handle_response.drag_started() {
                    if let Some(pos) = handle_response.interact_pointer_pos() {
                        self.controller.start_selection_drag_payload(selection, pos);
                    }
                } else if handle_response.dragged() {
                    if let Some(pos) = handle_response.interact_pointer_pos() {
                        self.controller.update_active_drag(pos, None, false, None);
                    }
                } else if handle_response.drag_stopped() {
                    self.controller.finish_active_drag();
                }
                if handle_response.dragged() {
                    ui.output_mut(|o| o.cursor_icon = CursorIcon::Grabbing);
                } else if handle_response.hovered() {
                    ui.output_mut(|o| o.cursor_icon = CursorIcon::Grab);
                }

                if let Some(duration_label) =
                    self.controller.ui.waveform.selection_duration.as_deref()
                {
                    let text_color = style::with_alpha(palette.bg_secondary, 240);
                    let bar_color = style::with_alpha(highlight, 80);
                    let galley = ui.ctx().fonts_mut(|f| {
                        f.layout_job(LayoutJob::simple_singleline(
                            duration_label.to_string(),
                            TextStyle::Small.resolve(ui.style()),
                            text_color,
                        ))
                    });
                    let padding = egui::vec2(8.0, 2.0);
                    let bar_height = galley.size().y + padding.y * 2.0;
                    let bar_rect = egui::Rect::from_min_size(
                        egui::pos2(selection_rect.left(), selection_rect.bottom() - bar_height),
                        egui::vec2(selection_rect.width(), bar_height),
                    );
                    painter.rect_filled(bar_rect, 0.0, bar_color);
                    let text_pos = egui::pos2(
                        (bar_rect.right() - padding.x - galley.size().x)
                            .max(bar_rect.left() + padding.x),
                        bar_rect.top() + (bar_height - galley.size().y) * 0.5,
                    );
                    painter.galley(text_pos, galley, text_color);
                }

                let start_edge_rect =
                    selection_edge_handle_rect(selection_rect, SelectionEdge::Start);
                let end_edge_rect = selection_edge_handle_rect(selection_rect, SelectionEdge::End);
                let start_edge_response = ui.interact(
                    start_edge_rect,
                    ui.id().with("selection_edge_start"),
                    egui::Sense::click_and_drag(),
                );
                let end_edge_response = ui.interact(
                    end_edge_rect,
                    ui.id().with("selection_edge_end"),
                    egui::Sense::click_and_drag(),
                );
                let start_edge_pointer_down = start_edge_response.is_pointer_button_down_on();
                let end_edge_pointer_down = end_edge_response.is_pointer_button_down_on();
                edge_dragging = start_edge_pointer_down
                    || end_edge_pointer_down
                    || start_edge_response.dragged()
                    || start_edge_response.drag_started()
                    || end_edge_response.dragged()
                    || end_edge_response.drag_started();
                for (edge, edge_rect, edge_response) in [
                    (SelectionEdge::Start, start_edge_rect, start_edge_response),
                    (SelectionEdge::End, end_edge_rect, end_edge_response),
                ] {
                    let pointer_down = edge_response.is_pointer_button_down_on();
                    if edge_response.drag_started() || pointer_down {
                        self.controller.start_selection_edge_drag(edge);
                        if self.selection_edge_offset.is_none() {
                            let edge_pos = match edge {
                                SelectionEdge::Start => selection_rect.left(),
                                SelectionEdge::End => selection_rect.right(),
                            };
                            if let Some(pos) = edge_response.interact_pointer_pos() {
                                self.selection_edge_offset = Some(pos.x - edge_pos);
                            } else {
                                self.selection_edge_offset = Some(0.0);
                            }
                        }
                    }
                    if pointer_down || edge_response.dragged() {
                        if let Some(pos) = edge_response.interact_pointer_pos() {
                            let offset = self.selection_edge_offset.unwrap_or(0.0);
                            let normalized =
                                ((pos.x - offset - rect.left()) / rect.width()).clamp(0.0, 1.0);
                            self.controller.update_selection_drag(normalized);
                        }
                    }
                    if edge_response.drag_stopped() {
                        self.selection_edge_offset = None;
                        self.controller.finish_selection_drag();
                    }
                    let edge_hovered = pointer_pos.map(|p| edge_rect.contains(p)).unwrap_or(false)
                        || edge_response.hovered()
                        || pointer_down
                        || edge_response.dragged();
                    if edge_hovered {
                        let color = highlight;
                        paint_selection_edge_bracket(&painter, edge_rect, edge, color);
                        ui.output_mut(|o| o.cursor_icon = CursorIcon::ResizeHorizontal);
                    }
                }
                if !ui.ctx().input(|i| i.pointer.primary_down()) {
                    if self.controller.is_selection_dragging() {
                        self.controller.finish_selection_drag();
                    }
                    self.selection_edge_offset = None;
                }

                let selection_menu = ui.interact(
                    selection_rect,
                    ui.id().with("selection_context_menu"),
                    egui::Sense::click(),
                );
                selection_menu.context_menu(|ui| {
                    let palette = style::palette();
                    let mut close_menu = false;
                    ui.label(RichText::new("Selection actions").color(palette.text_primary));
                    if ui
                        .button("Crop to selection")
                        .on_hover_text("Overwrite the file with just this region")
                        .clicked()
                    {
                        if self.controller.crop_waveform_selection().is_ok() {
                            close_menu = true;
                        }
                    }
                    if ui
                        .button("Trim selection out")
                        .on_hover_text("Remove the selection and close the gap")
                        .clicked()
                    {
                        if self.controller.trim_waveform_selection().is_ok() {
                            close_menu = true;
                        }
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        let fade_lr_button = egui::Button::new(
                            RichText::new("\\ Fade to null").color(palette.text_primary),
                        );
                        let fade_lr = ui
                            .add(fade_lr_button)
                            .on_hover_text("Fade left to right down to silence");
                        if fade_lr.clicked() {
                            if self
                                .controller
                                .fade_waveform_selection_left_to_right()
                                .is_ok()
                            {
                                close_menu = true;
                            }
                        }
                        let fade_rl_button = egui::Button::new(
                            RichText::new("/ Fade to null").color(palette.text_primary),
                        );
                        let fade_rl = ui
                            .add(fade_rl_button)
                            .on_hover_text("Fade right to left down to silence");
                        if fade_rl.clicked() {
                            if self
                                .controller
                                .fade_waveform_selection_right_to_left()
                                .is_ok()
                            {
                                close_menu = true;
                            }
                        }
                    });
                    if ui
                        .button("Mute selection")
                        .on_hover_text("Silence the selection without fades")
                        .clicked()
                    {
                        if self.controller.mute_waveform_selection().is_ok() {
                            close_menu = true;
                        }
                    }
                    if ui
                        .button("Normalize selection")
                        .on_hover_text("Scale selection to full range with 5ms edge fades")
                        .clicked()
                    {
                        if self.controller.normalize_waveform_selection().is_ok() {
                            close_menu = true;
                        }
                    }
                    if close_menu {
                        ui.close();
                    }
                });
            }
            let loop_bar_alpha = if self.controller.ui.waveform.loop_enabled {
                180
            } else {
                25
            };
            if loop_bar_alpha > 0 {
                let (loop_start, loop_end) = self
                    .controller
                    .ui
                    .waveform
                    .selection
                    .map(|range| (range.start(), range.end()))
                    .unwrap_or((0.0, 1.0));
                let clamped_start = loop_start.clamp(0.0, 1.0);
                let clamped_end = loop_end.clamp(clamped_start, 1.0);
                let start_norm = ((clamped_start - view.start) / view_width).clamp(0.0, 1.0);
                let end_norm = ((clamped_end - view.start) / view_width).clamp(0.0, 1.0);
                let width = (end_norm - start_norm).max(0.0) * rect.width();
                let bar_rect = egui::Rect::from_min_size(
                    egui::pos2(rect.left() + rect.width() * start_norm, rect.top()),
                    egui::vec2(width.max(2.0), 6.0),
                );
                painter.rect_filled(bar_rect, 0.0, style::with_alpha(highlight, loop_bar_alpha));
            }
            if self.controller.ui.waveform.playhead.visible {
                let x = to_screen_x(self.controller.ui.waveform.playhead.position, rect);
                painter.line_segment(
                    [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                    Stroke::new(2.0, highlight),
                );
            }

            // Waveform interactions: scroll to zoom, click to seek, drag to select.
            if response.hovered() {
                let scroll_delta = ui.input(|i| i.raw_scroll_delta.y);
                if scroll_delta.abs() > 0.0 {
                    let zoom_in = scroll_delta > 0.0;
                    // Use playhead when visible, otherwise pointer if available, otherwise center.
                    if self.controller.ui.waveform.playhead.visible {
                        self.controller.zoom_waveform(zoom_in);
                    } else if let Some(pos) = pointer_pos {
                        let normalized = ((pos.x - rect.left()) / rect.width())
                            .mul_add(view_width, view.start)
                            .clamp(0.0, 1.0);
                        self.controller.scroll_waveform_view(normalized);
                        self.controller.zoom_waveform(zoom_in);
                    } else {
                        self.controller.zoom_waveform(zoom_in);
                    }
                }
            }
            if !edge_dragging {
                let pointer_pos = response.interact_pointer_pos();
                let normalize_to_waveform =
                    |pos: egui::Pos2| {
                        ((pos.x - rect.left()) / rect.width())
                            .mul_add(view_width, view.start)
                            .clamp(0.0, 1.0)
                    };
                // Anchor creation to the initial press so quick drags keep the original start.
                let drag_start_normalized = if response.drag_started() {
                    if self.controller.ui.waveform.image.is_some() {
                        self.controller.focus_waveform_context();
                    }
                    let press_origin = ui.ctx().input(|i| i.pointer.press_origin());
                    press_origin
                        .map(|pos| {
                            ui.ctx()
                                .layer_transform_from_global(response.layer_id)
                                .map(|transform| transform * pos)
                                .unwrap_or(pos)
                        })
                        .map(normalize_to_waveform)
                        .or_else(|| pointer_pos.map(normalize_to_waveform))
                } else {
                    None
                };
                let normalized = pointer_pos.map(normalize_to_waveform);
                if response.drag_started() {
                    if let Some(value) = drag_start_normalized {
                        self.controller.start_selection_drag(value);
                    }
                } else if response.dragged() {
                    if let Some(value) = normalized {
                        if self.controller.ui.waveform.image.is_some() {
                            self.controller.focus_waveform_context();
                        }
                        self.controller.update_selection_drag(value);
                    }
                } else if response.drag_stopped() {
                    self.controller.finish_selection_drag();
                } else if response.clicked() {
                    if self.controller.ui.waveform.image.is_some() {
                        self.controller.focus_waveform_context();
                    }
                    if self.controller.ui.waveform.selection.is_some() {
                        self.controller.clear_selection();
                    } else if let Some(value) = normalized {
                        self.controller.seek_to(value);
                    }
                }
            }
        });
        ui.add_space(6.0);
        let (scroll_rect, scroll_resp) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 10.0),
            egui::Sense::click_and_drag(),
        );
        let view = self.controller.ui.waveform.view;
        let view_width = view.width();
        let scroll_bg = style::with_alpha(palette.bg_secondary, 180);
        ui.painter().rect_filled(scroll_rect, 4.0, scroll_bg);
        let indicator_width = scroll_rect.width() * view_width;
        let indicator_x = scroll_rect.left() + scroll_rect.width() * view.start;
        let indicator_rect = egui::Rect::from_min_size(
            egui::pos2(indicator_x, scroll_rect.top()),
            egui::vec2(indicator_width.max(8.0), scroll_rect.height()),
        );
        ui.painter().rect_filled(
            indicator_rect,
            4.0,
            style::with_alpha(highlight, 200),
        );
        ui.painter().rect_stroke(
            indicator_rect,
            4.0,
            Stroke::new(1.0, style::with_alpha(palette.text_primary, 180)),
            StrokeKind::Outside,
        );
        if (scroll_resp.dragged() || scroll_resp.clicked()) && scroll_rect.width() > f32::EPSILON {
            if let Some(pos) = scroll_resp.interact_pointer_pos() {
                let frac = ((pos.x - scroll_rect.left()) / scroll_rect.width()).clamp(0.0, 1.0);
                self.controller.scroll_waveform_view(frac);
            }
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

fn selection_handle_rect(selection_rect: egui::Rect) -> egui::Rect {
    let handle_height = (selection_rect.height() / 6.0).max(10.0);
    egui::Rect::from_min_size(
        egui::pos2(
            selection_rect.left(),
            selection_rect.bottom() - handle_height,
        ),
        egui::vec2(selection_rect.width(), handle_height),
    )
}

const EDGE_HANDLE_WIDTH: f32 = 18.0;
const EDGE_ICON_HEIGHT_FRACTION: f32 = 0.8;
const EDGE_ICON_MIN_SIZE: f32 = 12.0;
const EDGE_BRACKET_WIDTH: f32 = 10.0;
const EDGE_BRACKET_STROKE: f32 = 1.5;

fn selection_edge_handle_rect(selection_rect: egui::Rect, edge: SelectionEdge) -> egui::Rect {
    let width = EDGE_HANDLE_WIDTH;
    let height = selection_rect.height();
    let x = match edge {
        SelectionEdge::Start => selection_rect.left() - width * 0.5,
        SelectionEdge::End => selection_rect.right() - width * 0.5,
    };
    egui::Rect::from_min_size(
        egui::pos2(x, selection_rect.bottom() - height),
        egui::vec2(width, height),
    )
}

fn paint_selection_edge_bracket(
    painter: &egui::Painter,
    edge_rect: egui::Rect,
    edge: SelectionEdge,
    color: Color32,
) {
    let height = (edge_rect.height() * EDGE_ICON_HEIGHT_FRACTION)
        .clamp(EDGE_ICON_MIN_SIZE, edge_rect.height());
    let half_height = height * 0.5;
    let center = edge_rect.center();
    let top = center.y - half_height;
    let bottom = center.y + half_height;
    let (vertical_x, horizontal_start, horizontal_end) = match edge {
        SelectionEdge::Start => (center.x, center.x, center.x + EDGE_BRACKET_WIDTH),
        SelectionEdge::End => (center.x, center.x, center.x - EDGE_BRACKET_WIDTH),
    };
    let stroke = Stroke::new(EDGE_BRACKET_STROKE, color);
    painter.line_segment(
        [egui::pos2(vertical_x, top), egui::pos2(vertical_x, bottom)],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(horizontal_start, top),
            egui::pos2(horizontal_end, top),
        ],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(horizontal_start, bottom),
            egui::pos2(horizontal_end, bottom),
        ],
        stroke,
    );
}
