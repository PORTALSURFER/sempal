use super::style;
use super::*;
use eframe::egui::{
    self, Align2, Color32, Rgba, RichText, Stroke, StrokeKind, TextStyle,
    TextureOptions, Ui,
};

mod destructive_prompt;
mod hover_overlay;
mod selection_geometry;
mod selection_menu;
mod selection_overlay;

impl EguiApp {
    pub(super) fn render_waveform(&mut self, ui: &mut Ui) {
        let palette = style::palette();
        let highlight = palette.accent_copper;
        let cursor_color = palette.accent_mint;
        let start_marker_color = palette.accent_ice;
        let is_loading = self.controller.ui.waveform.loading.is_some();
        let mut view_mode = self.controller.ui.waveform.channel_view;
        ui.horizontal(|ui| {
            let mono = ui.selectable_value(
                &mut view_mode,
                crate::waveform::WaveformChannelView::Mono,
                "Mono envelope",
            );
            mono.on_hover_text("Show peak envelope across all channels");
            let split = ui.selectable_value(
                &mut view_mode,
                crate::waveform::WaveformChannelView::SplitStereo,
                "Split L/R",
            );
            split.on_hover_text("Render the first two channels separately");
            ui.add_space(10.0);
            let loop_enabled = self.controller.ui.waveform.loop_enabled;
            let loop_label = if loop_enabled {
                RichText::new("Loop: On").color(palette.accent_mint)
            } else {
                RichText::new("Loop: Off").color(palette.text_muted)
            };
            if ui
                .add(egui::Button::new(loop_label))
                .on_hover_text("Toggle loop playback for the current selection (or whole sample)")
                .clicked()
            {
                self.controller.toggle_loop();
            }
        });
        if view_mode != self.controller.ui.waveform.channel_view {
            self.controller.set_waveform_channel_view(view_mode);
        }
        let frame = style::section_frame();
        let frame_response = frame.show(ui, |ui| {
            let desired = egui::vec2(ui.available_width(), 260.0);
            let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::click_and_drag());
            let target_width = rect.width().round().max(1.0) as u32;
            let target_height = rect.height().round().max(1.0) as u32;
            self.controller
                .update_waveform_size(target_width, target_height);
            let pointer_pos = response.hover_pos();
            let view = self.controller.ui.waveform.view;
            let view_width = view.width();
            let to_screen_x = |position: f32, rect: egui::Rect| {
                let normalized = ((position - view.start) / view_width).clamp(0.0, 1.0);
                rect.left() + rect.width() * normalized
            };
            if let Some(message) = self.controller.ui.waveform.notice.as_ref() {
                ui.painter().rect_filled(rect, 0.0, palette.bg_primary);
                let font = TextStyle::Heading.resolve(ui.style());
                ui.painter().text(
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
                let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                ui.painter()
                    .image(id, rect, uv, style::high_contrast_text());
            } else {
                let loading_fill =
                    waveform_loading_fill(ui, palette.bg_primary, palette.accent_copper);
                ui.painter().rect_filled(rect, 0.0, loading_fill);
            }
            if is_loading {
                let glow = style::with_alpha(palette.accent_copper, 28);
                ui.painter().rect_filled(rect.shrink(2.0), 4.0, glow);
            }

            hover_overlay::render_hover_overlay(
                self,
                ui,
                rect,
                pointer_pos,
                view,
                view_width,
                cursor_color,
                &to_screen_x,
            );

            if let Some(marker_pos) = self.controller.ui.waveform.last_start_marker
                && marker_pos >= view.start
                && marker_pos <= view.end
            {
                let x = to_screen_x(marker_pos, rect);
                let stroke = Stroke::new(1.5, style::with_alpha(start_marker_color, 230));
                let mut y = rect.top();
                let bottom = rect.bottom();
                let dash = 6.0;
                let gap = 4.0;
                while y < bottom {
                    let end = (y + dash).min(bottom);
                    ui.painter()
                        .line_segment([egui::pos2(x, y), egui::pos2(x, end)], stroke);
                    y += dash + gap;
                }
            }

            let edge_dragging = selection_overlay::render_selection_overlay(
                self,
                ui,
                rect,
                &palette,
                view,
                view_width,
                highlight,
                pointer_pos,
            );
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
                ui.painter()
                    .rect_filled(bar_rect, 0.0, style::with_alpha(highlight, loop_bar_alpha));
            }
            if self.controller.ui.waveform.playhead.visible {
                let x = to_screen_x(self.controller.ui.waveform.playhead.position, rect);
                ui.painter().line_segment(
                    [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                    Stroke::new(2.0, highlight),
                );
            }

            // Waveform interactions: scroll to zoom, click to seek, drag to select.
            if response.hovered() {
                let scroll_delta = ui.input(|i| i.raw_scroll_delta);
                if scroll_delta != egui::Vec2::ZERO {
                    let shift_down = ui.input(|i| i.modifiers.shift);
                    if shift_down && view_width < 1.0 {
                        // Pan the zoomed view horizontally when shift is held.
                        let pan_delta =
                            scroll_delta * self.controller.ui.controls.waveform_scroll_speed;
                        let invert = if self.controller.ui.controls.invert_waveform_scroll {
                            -1.0
                        } else {
                            1.0
                        };
                        let delta_x = if pan_delta.x.abs() > 0.0 {
                            pan_delta.x
                        } else {
                            pan_delta.y
                        } * invert;
                        if delta_x.abs() > 0.0 {
                            let view_center = view.start + view_width * 0.5;
                            let fraction_delta = (delta_x / rect.width()) * view_width;
                            let target_center = view_center + fraction_delta;
                            self.controller.scroll_waveform_view(target_center);
                        }
                    } else {
                        let zoom_delta = scroll_delta * 0.6;
                        let zoom_in = zoom_delta.y > 0.0;
                        let per_step_factor = self.controller.ui.controls.wheel_zoom_factor;
                        // Use playhead when visible, otherwise pointer if available, otherwise center.
                        let zoom_steps = zoom_delta.y.abs().round().max(1.0) as u32;
                        let focus_override = response
                            .hover_pos()
                            .or_else(|| response.interact_pointer_pos())
                            .map(|pos| {
                                ((pos.x - rect.left()) / rect.width())
                                    .mul_add(view_width, view.start)
                                    .clamp(0.0, 1.0)
                            });
                        self.controller.zoom_waveform_steps_with_factor(
                            zoom_in,
                            zoom_steps,
                            focus_override,
                            Some(per_step_factor),
                            false,
                            false,
                        );
                    }
                }
            }
            if !edge_dragging {
                let pointer_pos = response.interact_pointer_pos();
                let normalize_to_waveform = |pos: egui::Pos2| {
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

            let view = self.controller.ui.waveform.view;
            let view_width = view.width();
            if view_width < 1.0 {
                let bar_height = 12.0;
                let scroll_rect = egui::Rect::from_min_size(
                    egui::pos2(rect.left(), rect.bottom() - bar_height),
                    egui::vec2(rect.width(), bar_height),
                );
                let scroll_resp = ui.interact(
                    scroll_rect,
                    ui.id().with("waveform_scrollbar"),
                    egui::Sense::click_and_drag(),
                );
                let scroll_bg = style::with_alpha(palette.bg_primary, 140);
                ui.painter().rect_filled(scroll_rect, 0.0, scroll_bg);
                let indicator_width = scroll_rect.width() * view_width;
                let indicator_x = scroll_rect.left() + scroll_rect.width() * view.start;
                let indicator_rect = egui::Rect::from_min_size(
                    egui::pos2(indicator_x, scroll_rect.top()),
                    egui::vec2(indicator_width.max(8.0), scroll_rect.height()),
                );
                let thumb_color = style::with_alpha(palette.accent_ice, 200);
                ui.painter()
                    .rect_filled(indicator_rect, 0.0, thumb_color);
                if (scroll_resp.dragged() || scroll_resp.clicked())
                    && scroll_rect.width() > f32::EPSILON
                    && let Some(pos) = scroll_resp.interact_pointer_pos()
                {
                    let frac = ((pos.x - scroll_rect.left()) / scroll_rect.width()).clamp(0.0, 1.0);
                    self.controller.scroll_waveform_view(frac);
                }
            }
        });
        style::paint_section_border(ui, frame_response.response.rect, false);
        if let Some(prompt) = self.controller.ui.waveform.pending_destructive.clone() {
            self.render_destructive_edit_prompt(ui.ctx(), prompt);
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

fn waveform_loading_fill(ui: &Ui, base: Color32, accent: Color32) -> Color32 {
    let time = ui.input(|i| i.time) as f32;
    let pulse = ((time * 2.4).sin() * 0.5 + 0.5).clamp(0.0, 1.0);
    let base_rgba: Rgba = base.into();
    let accent_rgba: Rgba = accent.into();
    let mixed = base_rgba * (1.0 - pulse * 0.12) + accent_rgba * (pulse * 0.08);
    Color32::from_rgba_unmultiplied(
        (mixed.r() * 255.0) as u8,
        (mixed.g() * 255.0) as u8,
        (mixed.b() * 255.0) as u8,
        255,
    )
}
