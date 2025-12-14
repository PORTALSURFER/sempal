use super::style;
use super::*;
use crate::{
    egui_app::state::{DragSource, DragTarget},
    selection::SelectionEdge,
};
use eframe::egui::{
    self, Align2, Color32, CursorIcon, Rgba, RichText, Stroke, StrokeKind, TextStyle,
    TextureOptions, Ui, text::LayoutJob,
};

mod destructive_prompt;
mod selection_geometry;
mod selection_menu;

use selection_geometry::{
    paint_selection_edge_bracket, selection_edge_handle_rect, selection_handle_rect,
};

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
            let painter = ui.painter();
            let pointer_pos = response.hover_pos();
            let view = self.controller.ui.waveform.view;
            let view_width = view.width();
            let to_screen_x = |position: f32, rect: egui::Rect| {
                let normalized = ((position - view.start) / view_width).clamp(0.0, 1.0);
                rect.left() + rect.width() * normalized
            };
            let to_wave_pos = |pos: egui::Pos2, rect: egui::Rect| {
                let normalized = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                normalized.mul_add(view_width, view.start).clamp(0.0, 1.0)
            };
            if let Some(message) = self.controller.ui.waveform.notice.as_ref() {
                painter.rect_filled(rect, 0.0, palette.bg_primary);
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
                let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                painter.image(id, rect, uv, style::high_contrast_text());
            } else {
                let loading_fill =
                    waveform_loading_fill(ui, palette.bg_primary, palette.accent_copper);
                painter.rect_filled(rect, 0.0, loading_fill);
            }
            if is_loading {
                let glow = style::with_alpha(palette.accent_copper, 28);
                painter.rect_filled(rect.shrink(2.0), 4.0, glow);
            }

            self.controller.update_waveform_hover_time(None);
            let mut hover_x = None;
            let mut hovering = false;
            if let Some(pos) = pointer_pos.filter(|p| rect.contains(*p)) {
                let now = std::time::Instant::now();
                let moved = self
                    .controller
                    .ui
                    .waveform
                    .hover_pointer_pos
                    .map_or(true, |prev| prev.distance(pos) > 0.5);
                if moved {
                    self.controller.ui.waveform.hover_pointer_pos = Some(pos);
                    self.controller.ui.waveform.hover_pointer_last_moved_at = Some(now);
                }

                let normalized = ((pos.x - rect.left()) / rect.width())
                    .mul_add(view_width, view.start)
                    .clamp(0.0, 1.0);
                hovering = true;
                let allow_hover_override = moved
                    || self
                        .controller
                        .ui
                        .waveform
                        .cursor_last_navigation_at
                        .is_none_or(|nav| {
                            self.controller
                                .ui
                                .waveform
                                .hover_pointer_last_moved_at
                                .is_none_or(|moved_at| nav <= moved_at)
                        });

                if allow_hover_override {
                    hover_x = Some(pos.x);
                    self.controller.set_waveform_cursor_from_hover(normalized);
                    self.controller.update_waveform_hover_time(Some(normalized));
                } else if let Some(cursor) = self.controller.ui.waveform.cursor {
                    hover_x = Some(to_screen_x(cursor, rect));
                    self.controller.update_waveform_hover_time(Some(cursor));
                } else {
                    hover_x = Some(pos.x);
                    self.controller.set_waveform_cursor_from_hover(normalized);
                    self.controller.update_waveform_hover_time(Some(normalized));
                }
            }
            let cursor_alpha = self.controller.waveform_cursor_alpha(hovering);
            if let Some(cursor) = self.controller.ui.waveform.cursor {
                let x = to_screen_x(cursor, rect);
                let stroke_alpha = (220.0 * cursor_alpha).round().clamp(0.0, 255.0) as u8;
                if stroke_alpha > 0 {
                    painter.line_segment(
                        [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                        Stroke::new(1.0, style::with_alpha(cursor_color, stroke_alpha)),
                    );
                }
            }
            if let Some(label) = self.controller.ui.waveform.hover_time_label.as_deref()
                && let Some(pointer_x) = hover_x
            {
                let text_color = style::with_alpha(palette.text_primary, 240);
                let galley = ui.ctx().fonts_mut(|f| {
                    f.layout_job(LayoutJob::simple_singleline(
                        label.to_string(),
                        TextStyle::Monospace.resolve(ui.style()),
                        text_color,
                    ))
                });
                let padding = egui::vec2(6.0, 4.0);
                let size = galley.size() + padding * 2.0;
                let min_x = rect.left() + 4.0;
                let max_x = rect.right() - size.x - 4.0;
                let desired_x = pointer_x + 8.0;
                let label_x = desired_x.clamp(min_x, max_x);
                let label_y = rect.top() + 8.0;
                let label_rect = egui::Rect::from_min_size(egui::pos2(label_x, label_y), size);
                let bg = style::with_alpha(palette.bg_primary, 235);
                let border = Stroke::new(1.0, style::with_alpha(palette.panel_outline, 220));
                painter.rect_filled(label_rect, 4.0, bg);
                painter.rect_stroke(label_rect, 4.0, border, StrokeKind::Inside);
                painter.galley(label_rect.min + padding, galley, text_color);
            }

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
                    painter.line_segment([egui::pos2(x, y), egui::pos2(x, end)], stroke);
                    y += dash + gap;
                }
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
                        let alt = ui.input(|i| i.modifiers.alt);
                        if alt {
                            let anchor = to_wave_pos(pos, rect);
                            self.selection_slide = Some(super::SelectionSlide {
                                anchor,
                                range: selection,
                            });
                        } else {
                            let keep_source_focused = ui.input(|i| i.modifiers.shift);
                            self.controller.start_selection_drag_payload(
                                selection,
                                pos,
                                keep_source_focused,
                            );
                        }
                    }
                } else if handle_response.dragged() {
                    if let Some(pos) = handle_response.interact_pointer_pos() {
                        if let Some(slide) = self.selection_slide {
                            let cursor = to_wave_pos(pos, rect);
                            let delta = cursor - slide.anchor;
                            self.controller
                                .set_selection_range(slide.range.shift(delta));
                        } else {
                            let shift_down = ui.input(|i| i.modifiers.shift);
                            self.controller.update_active_drag(
                                pos,
                                DragSource::Waveform,
                                DragTarget::None,
                                shift_down,
                            );
                        }
                    }
                } else if handle_response.drag_stopped() {
                    if self.selection_slide.take().is_some() {
                        self.controller.finish_selection_drag();
                    } else {
                        self.controller.finish_active_drag();
                    }
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
                    if (pointer_down || edge_response.dragged())
                        && let Some(pos) = edge_response.interact_pointer_pos()
                    {
                        let offset = self.selection_edge_offset.unwrap_or(0.0);
                        let view_fraction =
                            ((pos.x - offset - rect.left()) / rect.width()).clamp(0.0, 1.0);
                        let absolute = view.start + view_width.max(f32::EPSILON) * view_fraction;
                        let clamped = absolute.clamp(0.0, 1.0);
                        self.controller.update_selection_drag(clamped);
                    }
                    if edge_response.drag_stopped() {
                        self.selection_edge_offset = None;
                        self.controller.finish_selection_drag();
                    }
                    let edge_hovered = pointer_pos.is_some_and(|p| edge_rect.contains(p))
                        || edge_response.hovered()
                        || pointer_down
                        || edge_response.dragged();
                    if edge_hovered {
                        let color = highlight;
                        paint_selection_edge_bracket(painter, edge_rect, edge, color);
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
                    selection_menu::render_selection_context_menu(self, ui);
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
                painter.rect_filled(scroll_rect, 0.0, scroll_bg);
                let indicator_width = scroll_rect.width() * view_width;
                let indicator_x = scroll_rect.left() + scroll_rect.width() * view.start;
                let indicator_rect = egui::Rect::from_min_size(
                    egui::pos2(indicator_x, scroll_rect.top()),
                    egui::vec2(indicator_width.max(8.0), scroll_rect.height()),
                );
                let thumb_color = style::with_alpha(palette.accent_ice, 200);
                painter.rect_filled(indicator_rect, 0.0, thumb_color);
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
