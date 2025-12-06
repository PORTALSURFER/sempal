//! egui renderer for the application UI.
use std::cell::RefCell;
use std::rc::Rc;

use crate::audio::AudioPlayer;
use crate::egui_app::controller::EguiController;
use crate::egui_app::state::{TriageColumn, TriageIndex};
use crate::waveform::WaveformRenderer;
use eframe::egui::{
    self, Align, Area, Color32, Frame, Margin, Order, RichText, Stroke, TextureHandle,
    TextureOptions, Ui, Vec2,
};

/// Renders the egui UI using the shared controller state.
pub struct EguiApp {
    controller: EguiController,
    visuals_set: bool,
    waveform_tex: Option<TextureHandle>,
}

impl EguiApp {
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
                    for (index, row) in rows.iter().enumerate() {
                        let is_selected = Some(index) == selected;
                        ui.push_id(&row.id, |ui| {
                            let mut response = ui.selectable_label(
                                is_selected,
                                RichText::new(&row.name).color(Color32::WHITE),
                            );
                            response = response.on_hover_text(&row.path);
                            if response.clicked() {
                                self.controller.select_source_by_index(index);
                            }
                            ui.add_space(4.0);
                        });
                    }
                });
        });
    }

    fn render_collections_panel(&mut self, ui: &mut Ui) {
        let drag_active = self.controller.ui.drag.active_path.is_some();
        let pointer_pos = ui.input(|i| i.pointer.hover_pos());
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
                    for (index, collection) in rows.iter().enumerate() {
                        let selected = collection.selected;
                        let label = format!("{} ({})", collection.name, collection.count);
                        ui.push_id(&collection.id, |ui| {
                            let response = ui.selectable_label(
                                selected,
                                RichText::new(label).color(Color32::WHITE),
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
                            ui.add_space(4.0);
                        });
                    }
                });
            ui.label(RichText::new("Collection items").color(Color32::WHITE));
            let drag_active = self.controller.ui.drag.active_path.is_some();
            let pointer_pos = ui.input(|i| i.pointer.hover_pos());
            let samples = self.controller.ui.collections.samples.clone();
            let selected_row = self.controller.ui.collections.selected_sample;
            const ROW_HEIGHT: f32 = 28.0;
            let frame = egui::Frame::none().fill(Color32::from_rgb(16, 16, 16));
            let scroll_response = frame.show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_source("collection_items_scroll")
                    .show_rows(ui, ROW_HEIGHT, samples.len(), |ui, row_range| {
                        for row in row_range {
                            let Some(sample) = samples.get(row) else {
                                continue;
                            };
                            let path = sample.path.clone();
                            let label = format!("{} — {}", sample.source, sample.label);
                            let is_selected = Some(row) == selected_row;
                            let mut button =
                                egui::Button::new(RichText::new(label).color(Color32::LIGHT_GRAY))
                                    .sense(egui::Sense::click_and_drag());
                            if is_selected {
                                button = button.fill(Color32::from_rgb(30, 30, 30));
                            }
                            ui.push_id(
                                format!("{}:{}:{}", sample.source_id, sample.source, sample.label),
                                |ui| {
                                    let response = ui.add_sized(
                                        egui::vec2(ui.available_width(), ROW_HEIGHT),
                                        button,
                                    );
                                    if response.clicked() {
                                        self.controller.select_collection_sample(row);
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
            });
            if let Some(row) = selected_row {
                let viewport_height = scroll_response.inner.inner_rect.height();
                let content_height = scroll_response.inner.content_size.y;
                let target = (row as f32 + 0.5) * ROW_HEIGHT - viewport_height * 0.5;
                let max_offset = (content_height - viewport_height).max(0.0);
                let desired_offset = target.clamp(0.0, max_offset);
                let mut state = scroll_response.inner.state;
                state.offset.y = desired_offset;
                state.store(ui.ctx(), scroll_response.inner.id);
            }
            if drag_active {
                if let Some(pointer) = pointer_pos {
                    if scroll_response.response.rect.contains(pointer) {
                        self.controller
                            .update_sample_drag(pointer, None, false, None);
                        ui.painter().rect_stroke(
                            scroll_response.response.rect,
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
        let pointer_pos = ui.input(|i| i.pointer.hover_pos());
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
        const ROW_HEIGHT: f32 = 30.0;
        let total_rows = self.controller.triage_indices(column).len();
        let bg_frame = Frame::none().fill(Color32::from_rgb(16, 16, 16));
        let frame_response = bg_frame.show(ui, |ui| {
            let scroll_response = egui::ScrollArea::vertical()
                .id_source(format!("triage_scroll_{title}"))
                .show_rows(ui, ROW_HEIGHT, total_rows, |ui, row_range| {
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
                        let mut label = self
                            .controller
                            .wav_label(entry_index)
                            .unwrap_or_else(|| path.to_string_lossy().to_string());
                        if is_loaded {
                            label.push_str(" • loaded");
                        }

                        let mut button =
                            egui::Button::new(RichText::new(label).color(Color32::WHITE))
                                .sense(egui::Sense::click_and_drag());
                        if is_selected {
                            button = button.fill(Color32::from_rgb(30, 30, 30));
                        }
                        ui.push_id(&path, |ui| {
                            let response =
                                ui.add_sized(egui::vec2(ui.available_width(), ROW_HEIGHT), button);
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
        if let (Some(row), true) = (selected_row, triage_autoscroll) {
            let viewport_height = frame_response.inner.inner_rect.height();
            let content_height = frame_response.inner.content_size.y;
            let target = (row as f32 + 0.5) * ROW_HEIGHT - viewport_height * 0.5;
            let max_offset = (content_height - viewport_height).max(0.0);
            let desired_offset = target.clamp(0.0, max_offset);
            let mut state = frame_response.inner.state;
            state.offset.y = desired_offset;
            state.store(ui.ctx(), frame_response.inner.id);
        }
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
        let collection_focus = self.controller.ui.collections.selected_sample.is_some();
        let triage_has_selection = self.controller.ui.triage.selected.is_some();
        if collection_focus {
            self.controller.ui.triage.autoscroll = false;
            self.controller.ui.triage.selected = None;
        } else if triage_has_selection {
            self.controller.ui.triage.autoscroll = true;
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
            }
        }
        self.render_status(ctx);
        ctx.request_repaint();
    }
}
