use super::*;
use eframe::egui::{
    self, Color32, CursorIcon, Frame, Margin, RichText, Stroke, TextureOptions, Ui,
};

impl EguiApp {
    pub(super) fn render_waveform(&mut self, ui: &mut Ui) {
        let frame = Frame::none()
            .fill(Color32::from_rgb(16, 16, 16))
            .stroke(Stroke::new(1.0, Color32::from_rgb(48, 48, 48)))
            .inner_margin(Margin::symmetric(10.0, 8.0));
        frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Waveform Viewer").color(Color32::WHITE));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let loop_enabled = self.controller.ui.waveform.loop_enabled;
                    let text = if loop_enabled { "Loop on" } else { "Loop off" };
                    let button = egui::Button::new(RichText::new(text).color(Color32::WHITE));
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
                painter.image(id, rect, uv, Color32::WHITE);
            } else {
                painter.rect_filled(rect, 6.0, Color32::from_rgb(12, 12, 12));
            }
            painter.rect_stroke(rect, 6.0, Stroke::new(1.0, Color32::from_rgb(64, 64, 64)));

            if let Some(pos) = response.hover_pos().filter(|p| rect.contains(*p)) {
                let x = pos.x;
                let hover_line = egui::Rect::from_min_max(
                    egui::pos2(x, rect.top()),
                    egui::pos2(x, rect.bottom()),
                );
                painter.rect_stroke(
                    hover_line,
                    0.0,
                    Stroke::new(1.0, Color32::from_rgba_unmultiplied(80, 140, 200, 160)),
                );
            }

            if let Some(selection) = self.controller.ui.waveform.selection {
                let width = rect.width() * (selection.end() - selection.start()) as f32;
                let x = rect.left() + rect.width() * selection.start() as f32;
                let selection_rect = egui::Rect::from_min_size(
                    egui::pos2(x, rect.top()),
                    egui::vec2(width, rect.height()),
                );
                painter.rect_filled(
                    selection_rect,
                    4.0,
                    Color32::from_rgba_unmultiplied(28, 63, 106, 90),
                );
                let handle_rect = selection_handle_rect(selection_rect);
                let handle_response = ui.interact(
                    handle_rect,
                    ui.id().with("selection_handle"),
                    egui::Sense::drag(),
                );
                let handle_hovered = handle_response.hovered() || handle_response.dragged();
                let handle_color = if handle_hovered {
                    Color32::from_rgba_unmultiplied(74, 147, 221, 180)
                } else {
                    Color32::from_rgba_unmultiplied(54, 104, 164, 150)
                };
                painter.rect_filled(handle_rect, 4.0, handle_color);
                painter.rect_stroke(
                    handle_rect,
                    4.0,
                    Stroke::new(1.5, Color32::from_rgb(96, 168, 240)),
                );
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
            }
            if self.controller.ui.waveform.playhead.visible {
                let x = rect.left() + rect.width() * self.controller.ui.waveform.playhead.position;
                let line = egui::Rect::from_min_max(
                    egui::pos2(x, rect.top()),
                    egui::pos2(x, rect.bottom()),
                );
                painter.rect_stroke(line, 0.0, Stroke::new(2.0, Color32::from_rgb(51, 153, 255)));
            }

            // Waveform interactions: click to seek, shift-drag to select.
            if let Some(pos) = response.interact_pointer_pos() {
                if rect.contains(pos) {
                    let normalized = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                    let shift_down = ui.input(|i| i.modifiers.shift);
                    if response.drag_started() && shift_down {
                        self.controller.start_selection_drag(normalized);
                    } else if response.dragged() && shift_down {
                        self.controller.update_selection_drag(normalized);
                    } else if response.drag_stopped() && shift_down {
                        self.controller.finish_selection_drag();
                    } else if response.clicked() {
                        if shift_down {
                            self.controller.clear_selection();
                        } else {
                            self.controller.seek_to(normalized);
                        }
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
