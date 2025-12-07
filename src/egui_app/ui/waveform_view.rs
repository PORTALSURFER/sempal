use super::style;
use super::*;
use crate::selection::SelectionEdge;
use eframe::egui::{
    self, Color32, CursorIcon, Frame, Margin, RichText, Stroke, StrokeKind, TextureOptions, Ui,
};

impl EguiApp {
    pub(super) fn render_waveform(&mut self, ui: &mut Ui) {
        let palette = style::palette();
        let highlight = palette.accent_copper;
        let frame = Frame::new()
            .fill(style::compartment_fill())
            .stroke(style::outer_border())
            .inner_margin(Margin::symmetric(10, 6));
        frame.show(ui, |ui| {
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
                let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
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
                let width = rect.width() * (selection.end() - selection.start()) as f32;
                let x = rect.left() + rect.width() * selection.start() as f32;
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
                    style::with_alpha(highlight, 200)
                } else {
                    style::with_alpha(palette.grid_strong, 180)
                };
                painter.rect_filled(handle_rect, 0.0, handle_color);
                painter.rect_stroke(
                    handle_rect,
                    0.0,
                    Stroke::new(1.5, highlight),
                    StrokeKind::Inside,
                );
                if handle_response.drag_started() {
                    if ui.input(|i| i.modifiers.alt) {
                        self.controller.start_external_drag_for_selection(selection);
                    } else if let Some(pos) = handle_response.interact_pointer_pos() {
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
                edge_dragging = start_edge_response.dragged()
                    || start_edge_response.drag_started()
                    || end_edge_response.dragged()
                    || end_edge_response.drag_started();
                for (edge, edge_rect, edge_response) in [
                    (SelectionEdge::Start, start_edge_rect, start_edge_response),
                    (SelectionEdge::End, end_edge_rect, end_edge_response),
                ] {
                    if edge_response.drag_started() {
                        self.controller.start_selection_edge_drag(edge);
                    }
                    if edge_response.dragged() {
                        if let Some(pos) = edge_response.interact_pointer_pos() {
                            let normalized = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                            self.controller.update_selection_drag(normalized);
                        }
                    } else if edge_response.drag_stopped() {
                        self.controller.finish_selection_drag();
                    }
                    let edge_hovered = pointer_pos.map(|p| edge_rect.contains(p)).unwrap_or(false)
                        || edge_response.hovered()
                        || edge_response.dragged();
                    if edge_hovered {
                        let color = highlight;
                        paint_selection_edge_bracket(&painter, edge_rect, edge, color);
                        ui.output_mut(|o| o.cursor_icon = CursorIcon::ResizeHorizontal);
                    }
                }
            }
            if self.controller.ui.waveform.playhead.visible {
                let x = rect.left() + rect.width() * self.controller.ui.waveform.playhead.position;
                painter.line_segment(
                    [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                    Stroke::new(2.0, highlight),
                );
            }

            // Waveform interactions: click to seek, shift-drag to select.
            if !edge_dragging {
                let shift_down = ui.input(|i| i.modifiers.shift);
                let pointer_pos = response.interact_pointer_pos();
                let normalized =
                    pointer_pos.map(|pos| ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0));
                if shift_down && response.drag_started() {
                    if let Some(value) = normalized {
                        self.controller.start_selection_drag(value);
                    }
                } else if shift_down && response.dragged() {
                    if let Some(value) = normalized {
                        self.controller.update_selection_drag(value);
                    }
                } else if shift_down && response.drag_stopped() {
                    self.controller.finish_selection_drag();
                } else if response.clicked() {
                    if shift_down {
                        self.controller.clear_selection();
                    } else if let Some(value) = normalized {
                        self.controller.seek_to(value);
                    }
                }
            }
        });
    }
}

fn selection_handle_rect(selection_rect: egui::Rect) -> egui::Rect {
    let handle_height = (selection_rect.height() / 3.0).max(12.0);
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
