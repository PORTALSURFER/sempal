//! egui renderer for the application UI.
use std::cell::RefCell;
use std::rc::Rc;

use crate::audio::AudioPlayer;
use crate::egui_app::controller::EguiController;
use crate::egui_app::state::{TriageColumn, TriageIndex};
use crate::waveform::WaveformRenderer;
use eframe::egui::{
    self, Align, Align2, Area, Color32, Frame, Margin, Order, RichText, Stroke, TextStyle,
    TextureHandle, TextureOptions, Ui, Vec2,
};

/// Renders the egui UI using the shared controller state.
pub struct EguiApp {
    controller: EguiController,
    visuals_set: bool,
    waveform_tex: Option<TextureHandle>,
}

impl EguiApp {
    fn list_row_height(ui: &Ui) -> f32 {
        ui.spacing().interact_size.y
    }

    fn clamp_label_for_width(text: &str, available_width: f32) -> String {
        // Rough character-based truncation to avoid layout thrash.
        let width = available_width.max(1.0);
        let approx_char_width = 8.0;
        let max_chars = (width / approx_char_width).floor().max(6.0) as usize;
        if text.chars().count() <= max_chars {
            return text.to_string();
        }
        let keep = max_chars.saturating_sub(3);
        let mut clipped = String::with_capacity(max_chars);
        for (i, ch) in text.chars().enumerate() {
            if i >= keep {
                clipped.push_str("...");
                break;
            }
            clipped.push(ch);
        }
        clipped
    }

    fn render_list_row(
        ui: &mut Ui,
        label: &str,
        row_width: f32,
        row_height: f32,
        bg: Option<Color32>,
        text_color: Color32,
        sense: egui::Sense,
    ) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(row_width, row_height), sense);
        let mut fill = bg;
        if response.hovered() && bg.is_none() {
            fill = Some(Color32::from_rgb(26, 26, 26));
        }
        if let Some(color) = fill {
            ui.painter().rect_filled(rect, 0.0, color);
        }
        let padding = ui.spacing().button_padding;
        let font_id = TextStyle::Button.resolve(ui.style());
        let text_pos = rect.left_center() + egui::vec2(padding.x, 0.0);
        ui.painter()
            .text(text_pos, Align2::LEFT_CENTER, label, font_id, text_color);
        response
    }

    /// Create a new egui app, loading persisted configuration.
    pub fn new(
        renderer: WaveformRenderer,
        player: Option<Rc<RefCell<AudioPlayer>>>,
    ) -> Result<Self, String> {
        let mut controller = EguiController::new(renderer, player);
        controller
            .load_configuration()
            .map_err(|err| format!("Failed to load config: {err}"))?;
        controller.select_first_source();
        Ok(Self {
            controller,
            visuals_set: false,
            waveform_tex: None,
        })
    }

    fn apply_visuals(&mut self, ctx: &egui::Context) {
        if self.visuals_set {
            return;
        }
        let mut visuals = egui::Visuals::dark();
        visuals.window_fill = Color32::from_rgb(12, 12, 12);
        visuals.panel_fill = Color32::from_rgb(16, 16, 16);
        visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(16, 16, 16);
        ctx.set_visuals(visuals);
        self.visuals_set = true;
    }

    fn render_top_bar(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top_bar")
            .frame(Frame::none().fill(Color32::from_rgb(24, 24, 24)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Sample Sources").color(Color32::WHITE));
                    ui.add_space(8.0);
                    ui.separator();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button(RichText::new("Close").color(Color32::WHITE))
                            .clicked()
                        {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                });
            });
    }

    fn render_status(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status_bar")
            .frame(Frame::none().fill(Color32::from_rgb(0, 0, 0)))
            .show(ctx, |ui| {
                let status = &self.controller.ui.status;
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    ui.painter().circle_filled(
                        ui.cursor().min + egui::vec2(9.0, 11.0),
                        9.0,
                        status.badge_color,
                    );
                    ui.add_space(8.0);
                    ui.label(RichText::new(&status.badge_label).color(Color32::WHITE));
                    ui.separator();
                    ui.label(RichText::new(&status.text).color(Color32::WHITE));
                });
            });
    }

    fn render_sources_panel(&mut self, ui: &mut Ui) {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Sources").color(Color32::WHITE));
                if ui
                    .button(RichText::new("+").color(Color32::WHITE))
                    .clicked()
                {
                    self.controller.add_source_via_dialog();
                }
            });
            ui.add_space(6.0);
            egui::ScrollArea::vertical()
                .id_source("sources_scroll")
                .show(ui, |ui| {
                    let rows = self.controller.ui.sources.rows.clone();
                    let selected = self.controller.ui.sources.selected;
                    let row_height = Self::list_row_height(ui);
                    for (index, row) in rows.iter().enumerate() {
                        let is_selected = Some(index) == selected;
                        ui.push_id(&row.id, |ui| {
                            let row_width = ui.available_width();
                            let padding = ui.spacing().button_padding.x * 2.0;
                            let label = Self::clamp_label_for_width(&row.name, row_width - padding);
                            let bg = is_selected.then_some(Color32::from_rgb(30, 30, 30));
                            let response = Self::render_list_row(
                                ui,
                                &label,
                                row_width,
                                row_height,
                                bg,
                                Color32::WHITE,
                                egui::Sense::click(),
                            )
                            .on_hover_text(&row.path);
                            if response.clicked() {
                                self.controller.select_source_by_index(index);
                            }
                        });
                    }
                });
        });
    }

    fn render_collections_panel(&mut self, ui: &mut Ui) {
        let drag_active = self.controller.ui.drag.active_path.is_some();
        let pointer_pos = ui
            .input(|i| i.pointer.hover_pos().or_else(|| i.pointer.interact_pos()))
            .or(self.controller.ui.drag.position);
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Collections").color(Color32::WHITE));
                let add_button = ui.add_enabled(
                    self.controller.ui.collections.enabled,
                    egui::Button::new(RichText::new("+").color(Color32::WHITE)),
                );
                if add_button.clicked() {
                    self.controller.add_collection();
                }
                ui.add_space(4.0);
            });
            ui.add_space(6.0);
            let rows = self.controller.ui.collections.rows.clone();
            egui::ScrollArea::vertical()
                .id_source("collections_scroll")
                .show(ui, |ui| {
                    let row_height = Self::list_row_height(ui);
                    for (index, collection) in rows.iter().enumerate() {
                        let selected = collection.selected;
                        let label = format!("{} ({})", collection.name, collection.count);
                        ui.push_id(&collection.id, |ui| {
                            let row_width = ui.available_width();
                            let padding = ui.spacing().button_padding.x * 2.0;
                            let label = Self::clamp_label_for_width(&label, row_width - padding);
                            let bg = selected.then_some(Color32::from_rgb(30, 30, 30));
                            let response = Self::render_list_row(
                                ui,
                                &label,
                                row_width,
                                row_height,
                                bg,
                                Color32::WHITE,
                                egui::Sense::click(),
                            );
                            if response.clicked() {
                                self.controller.select_collection_by_index(Some(index));
                            }
                            if drag_active {
                                if let Some(pointer) = pointer_pos {
                                    if response.rect.contains(pointer) {
                                        self.controller.update_sample_drag(
                                            pointer,
                                            Some(collection.id.clone()),
                                            false,
                                            None,
                                        );
                                    }
                                }
                            }
                        });
                    }
                });
            ui.label(RichText::new("Collection items").color(Color32::WHITE));
            let drag_active = self.controller.ui.drag.active_path.is_some();
            let pointer_pos = ui
                .input(|i| i.pointer.hover_pos().or_else(|| i.pointer.interact_pos()))
                .or(self.controller.ui.drag.position);
            let samples = self.controller.ui.collections.samples.clone();
            let selected_row = self.controller.ui.collections.selected_sample;
            let current_collection_id = self.controller.current_collection_id();
            let hovering_collection =
                self.controller
                    .ui
                    .drag
                    .hovering_collection
                    .clone()
                    .or_else(|| {
                        if self.controller.ui.drag.hovering_drop_zone {
                            current_collection_id.clone()
                        } else {
                            None
                        }
                    });
            let active_drag_path = if drag_active {
                self.controller.ui.drag.active_path.clone()
            } else {
                None
            };
            let duplicate_row = if drag_active
                && hovering_collection
                    .as_ref()
                    .is_some_and(|id| Some(id) == current_collection_id.as_ref())
            {
                active_drag_path
                    .as_ref()
                    .and_then(|p| samples.iter().position(|s| &s.path == p))
            } else {
                None
            };
            let row_height = Self::list_row_height(ui);
            let available_height = ui.available_height();
            let frame = egui::Frame::none().fill(Color32::from_rgb(16, 16, 16));
            let scroll_response = frame.show(ui, |ui| {
                ui.set_min_height(available_height);
                let scroll = egui::ScrollArea::vertical().id_source("collection_items_scroll");
                if samples.is_empty() {
                    scroll.show(ui, |ui| {
                        let height = ui.available_height().max(available_height);
                        ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), height),
                            egui::Sense::hover(),
                        );
                    })
                } else {
                    scroll.show_rows(ui, row_height, samples.len(), |ui, row_range| {
                        for row in row_range {
                            let Some(sample) = samples.get(row) else {
                                continue;
                            };
                            let row_width = ui.available_width();
                            let padding = ui.spacing().button_padding.x * 2.0;
                            let path = sample.path.clone();
                            let label = format!("{} — {}", sample.source, sample.label);
                            let label = Self::clamp_label_for_width(&label, row_width - padding);
                            let is_selected = Some(row) == selected_row;
                            let is_duplicate_hover = drag_active
                                && active_drag_path.as_ref().is_some_and(|p| p == &path);
                            let bg = if is_selected {
                                Some(Color32::from_rgb(30, 30, 30))
                            } else if is_duplicate_hover {
                                Some(Color32::from_rgb(90, 60, 24))
                            } else {
                                None
                            };
                            ui.push_id(
                                format!("{}:{}:{}", sample.source_id, sample.source, sample.label),
                                |ui| {
                                    let response = Self::render_list_row(
                                        ui,
                                        &label,
                                        row_width,
                                        row_height,
                                        bg,
                                        Color32::LIGHT_GRAY,
                                        egui::Sense::click_and_drag(),
                                    );
                                    if response.clicked() {
                                        self.controller.select_collection_sample(row);
                                    }
                                    if is_duplicate_hover {
                                        ui.painter().rect_stroke(
                                            response.rect.expand(2.0),
                                            4.0,
                                            Stroke::new(2.0, Color32::from_rgb(255, 170, 80)),
                                        );
                                    }
                                    if response.drag_started() {
                                        if let Some(pos) = response.interact_pointer_pos() {
                                            self.controller.start_sample_drag(
                                                path.clone(),
                                                sample.label.clone(),
                                                pos,
                                            );
                                        }
                                    } else if drag_active && response.dragged() {
                                        if let Some(pos) = response.interact_pointer_pos() {
                                            self.controller
                                                .update_sample_drag(pos, None, false, None);
                                        }
                                    } else if response.drag_stopped() {
                                        self.controller.finish_sample_drag();
                                    }
                                },
                            );
                        }
                    })
                }
            });
            let viewport_height = scroll_response.inner.inner_rect.height();
            let content_height = scroll_response.inner.content_size.y;
            let max_offset = (content_height - viewport_height).max(0.0);
            let mut desired_offset = scroll_response.inner.state.offset.y;
            if let Some(row) = duplicate_row {
                desired_offset = (row as f32 + 0.5) * row_height - viewport_height * 0.5;
            } else if let Some(row) = selected_row {
                desired_offset = row as f32 * row_height;
            }
            let snapped_offset = (desired_offset / row_height)
                .round()
                .clamp(0.0, max_offset / row_height)
                * row_height;
            let mut state = scroll_response.inner.state;
            state.offset.y = snapped_offset.clamp(0.0, max_offset);
            state.store(ui.ctx(), scroll_response.inner.id);
            if drag_active {
                if let Some(pointer) = pointer_pos {
                    let target_rect = scroll_response.response.rect.expand2(egui::vec2(8.0, 0.0));
                    if target_rect.contains(pointer) {
                        self.controller.update_sample_drag(
                            pointer,
                            current_collection_id.clone(),
                            true,
                            None,
                        );
                        ui.painter().rect_stroke(
                            target_rect,
                            6.0,
                            Stroke::new(2.0, Color32::from_rgba_unmultiplied(80, 140, 200, 180)),
                        );
                    }
                }
            }
        });
    }

    fn render_waveform(&mut self, ui: &mut Ui) {
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

    fn render_triage(&mut self, ui: &mut Ui) {
        let spacing = 8.0;
        let selected = self.controller.ui.triage.selected;
        let loaded = self.controller.ui.triage.loaded;

        ui.columns(3, |columns| {
            self.render_triage_column(
                &mut columns[0],
                "Trash",
                TriageColumn::Trash,
                Color32::from_rgb(198, 143, 143),
                selected,
                loaded,
            );
            columns[0].add_space(spacing);
            self.render_triage_column(
                &mut columns[1],
                "Samples",
                TriageColumn::Neutral,
                Color32::from_rgb(208, 208, 208),
                selected,
                loaded,
            );
            columns[1].add_space(spacing);
            self.render_triage_column(
                &mut columns[2],
                "Keep",
                TriageColumn::Keep,
                Color32::from_rgb(158, 201, 167),
                selected,
                loaded,
            );
        });
    }

    fn render_triage_column(
        &mut self,
        ui: &mut Ui,
        title: &str,
        column: TriageColumn,
        accent: Color32,
        selected: Option<TriageIndex>,
        loaded: Option<TriageIndex>,
    ) {
        ui.label(RichText::new(title).color(accent));
        ui.add_space(6.0);
        let drag_active = self.controller.ui.drag.active_path.is_some();
        let pointer_pos = ui
            .input(|i| i.pointer.hover_pos().or_else(|| i.pointer.interact_pos()))
            .or(self.controller.ui.drag.position);
        let selected_row = match selected {
            Some(TriageIndex { column: c, row }) if c == column => Some(row),
            _ => None,
        };
        let loaded_row = match loaded {
            Some(TriageIndex { column: c, row }) if c == column => Some(row),
            _ => None,
        };
        let triage_autoscroll = self.controller.ui.triage.autoscroll
            && self.controller.ui.collections.selected_sample.is_none();
        let row_height = Self::list_row_height(ui);
        let total_rows = self.controller.triage_indices(column).len();
        let bg_frame = Frame::none().fill(Color32::from_rgb(16, 16, 16));
        let frame_response = bg_frame.show(ui, |ui| {
            let scroll_response = egui::ScrollArea::vertical()
                .id_source(format!("triage_scroll_{title}"))
                .show_rows(ui, row_height, total_rows, |ui, row_range| {
                    for row in row_range {
                        let entry_index = {
                            let indices = self.controller.triage_indices(column);
                            match indices.get(row) {
                                Some(index) => *index,
                                None => continue,
                            }
                        };
                        let Some(entry) = self.controller.wav_entry(entry_index) else {
                            continue;
                        };

                        let is_selected = selected_row == Some(row);
                        let is_loaded = loaded_row == Some(row);
                        let path = entry.relative_path.clone();
                        let row_width = ui.available_width();
                        let padding = ui.spacing().button_padding.x * 2.0;
                        let mut label = self
                            .controller
                            .wav_label(entry_index)
                            .unwrap_or_else(|| path.to_string_lossy().to_string());
                        if is_loaded {
                            label.push_str(" • loaded");
                        }
                        let label = Self::clamp_label_for_width(&label, row_width - padding);
                        let bg = is_selected.then_some(Color32::from_rgb(30, 30, 30));
                        ui.push_id(&path, |ui| {
                            let response = Self::render_list_row(
                                ui,
                                &label,
                                row_width,
                                row_height,
                                bg,
                                Color32::WHITE,
                                egui::Sense::click_and_drag(),
                            );
                            if response.clicked() {
                                self.controller.select_from_triage(&path);
                            }
                            if response.drag_started() {
                                if let Some(pos) = response.interact_pointer_pos() {
                                    let name = path.to_string_lossy().to_string();
                                    self.controller.start_sample_drag(path.clone(), name, pos);
                                }
                            } else if drag_active && response.dragged() {
                                if let Some(pos) = response.interact_pointer_pos() {
                                    self.controller.update_sample_drag(
                                        pos,
                                        None,
                                        false,
                                        Some(column),
                                    );
                                }
                            } else if response.drag_stopped() {
                                self.controller.finish_sample_drag();
                            }
                        });
                    }
                });
            scroll_response
        });
        let viewport_height = frame_response.inner.inner_rect.height();
        let content_height = frame_response.inner.content_size.y;
        let max_offset = (content_height - viewport_height).max(0.0);
        let mut desired_offset = frame_response.inner.state.offset.y;
        if let (Some(row), true) = (selected_row, triage_autoscroll) {
            desired_offset = (row as f32 + 0.5) * row_height - viewport_height * 0.5;
            self.controller.ui.triage.autoscroll = false;
        }
        let snapped_offset = (desired_offset / row_height)
            .round()
            .clamp(0.0, max_offset / row_height)
            * row_height;
        let mut state = frame_response.inner.state;
        state.offset.y = snapped_offset.clamp(0.0, max_offset);
        state.store(ui.ctx(), frame_response.inner.id);
        if drag_active {
            if let Some(pointer) = pointer_pos {
                if frame_response.response.rect.contains(pointer) {
                    self.controller
                        .update_sample_drag(pointer, None, false, Some(column));
                }
            }
        }
        if drag_active {
            if let Some(pointer) = pointer_pos {
                if frame_response.response.rect.contains(pointer) {
                    ui.painter().rect_stroke(
                        frame_response.response.rect,
                        6.0,
                        Stroke::new(2.0, Color32::from_rgba_unmultiplied(80, 140, 200, 180)),
                    );
                }
            }
        }
    }

    fn render_center(&mut self, ui: &mut Ui) {
        ui.vertical(|ui| {
            self.render_waveform(ui);
            ui.add_space(8.0);
            self.render_triage(ui);
        });
    }
}

impl eframe::App for EguiApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.apply_visuals(ctx);
        self.controller.tick_playhead();
        if self.controller.ui.drag.active_path.is_some() && !ctx.input(|i| i.pointer.primary_down())
        {
            self.controller.finish_sample_drag();
        }
        let collection_focus = self.controller.ui.collections.selected_sample.is_some();
        let triage_has_selection = self.controller.ui.triage.selected.is_some();
        if collection_focus {
            self.controller.ui.triage.autoscroll = false;
            self.controller.ui.triage.selected = None;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            self.controller.toggle_play_pause();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
            if collection_focus {
                self.controller.nudge_collection_sample(1);
            } else {
                self.controller.nudge_selection(1);
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
            if collection_focus {
                self.controller.nudge_collection_sample(-1);
            } else {
                self.controller.nudge_selection(-1);
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
            if ctx.input(|i| i.modifiers.ctrl) {
                if triage_has_selection {
                    self.controller.move_selection_column(1);
                }
            } else if triage_has_selection {
                let col = self.controller.ui.triage.selected.map(|t| t.column);
                let target = if matches!(col, Some(crate::egui_app::state::TriageColumn::Trash)) {
                    crate::sample_sources::SampleTag::Neutral
                } else {
                    crate::sample_sources::SampleTag::Keep
                };
                self.controller.tag_selected(target);
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
            if ctx.input(|i| i.modifiers.ctrl) {
                if triage_has_selection {
                    self.controller.move_selection_column(-1);
                }
            } else if triage_has_selection {
                self.controller
                    .tag_selected(crate::sample_sources::SampleTag::Trash);
            }
        }
        self.render_top_bar(ctx, frame);
        egui::SidePanel::left("sources")
            .resizable(false)
            .min_width(220.0)
            .max_width(240.0)
            .show(ctx, |ui| self.render_sources_panel(ui));
        egui::SidePanel::right("collections")
            .resizable(false)
            .min_width(240.0)
            .max_width(280.0)
            .show(ctx, |ui| self.render_collections_panel(ui));
        egui::CentralPanel::default().show(ctx, |ui| {
            self.render_center(ui);
        });
        if let Some(pos) = self.controller.ui.drag.position {
            let label = if self.controller.ui.drag.label.is_empty() {
                "Sample".to_string()
            } else {
                self.controller.ui.drag.label.clone()
            };
            Area::new("drag_preview".into())
                .order(Order::Tooltip)
                .pivot(egui::Align2::CENTER_CENTER)
                .current_pos(pos + Vec2::new(16.0, 16.0))
                .show(ctx, |ui| {
                    Frame::none()
                        .fill(Color32::from_rgba_unmultiplied(26, 39, 51, 220))
                        .stroke(Stroke::new(1.0, Color32::from_rgb(47, 111, 177)))
                        .rounding(6.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.add_space(8.0);
                                ui.colored_label(Color32::from_rgb(90, 176, 255), "●");
                                ui.label(RichText::new(label).color(Color32::WHITE));
                                ui.add_space(8.0);
                            });
                        });
                });
        }
        if self.controller.ui.drag.active_path.is_some() {
            if ctx.input(|i| i.pointer.any_released()) {
                self.controller.finish_sample_drag();
            } else if !ctx.input(|i| i.pointer.primary_down()) {
                // Safety net to clear drag visuals if a release was missed.
                self.controller.finish_sample_drag();
            }
        }
        self.render_status(ctx);
        ctx.request_repaint();
    }
}
