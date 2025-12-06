//! egui renderer for the application UI.
use std::cell::RefCell;
use std::rc::Rc;

use crate::audio::AudioPlayer;
use crate::egui_app::controller::EguiController;
use crate::egui_app::state::{TriageColumn, TriageIndex};
use crate::waveform::WaveformRenderer;
use eframe::egui::{
    self, Area, Color32, Frame, Margin, Order, RichText, Stroke, TextureHandle, TextureOptions, Ui,
    Vec2,
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
                                        );
                                    }
                                }
                            }
                            ui.add_space(4.0);
                        });
                    }
                });
            ui.add_space(8.0);
            // Drop zone for the currently selected collection.
            let zone_color = if self.controller.ui.drag.hovering_drop_zone {
                Color32::from_rgb(30, 47, 72)
            } else if drag_active && self.controller.ui.collections.selected.is_some() {
                Color32::from_rgb(20, 33, 51)
            } else {
                Color32::from_rgb(11, 11, 11)
            };
            let zone_stroke = if self.controller.ui.drag.hovering_drop_zone {
                Color32::from_rgb(59, 130, 196)
            } else if drag_active && self.controller.ui.collections.selected.is_some() {
                Color32::from_rgb(50, 91, 136)
            } else {
                Color32::from_rgb(48, 48, 48)
            };
            let zone_response = Frame::none()
                .fill(zone_color)
                .stroke(Stroke::new(1.0, zone_stroke))
                .rounding(6.0)
                .show(ui, |ui| {
                    ui.set_height(80.0);
                    ui.set_min_width(ui.available_width());
                    ui.vertical_centered(|ui| {
                        ui.add_space(10.0);
                        let text = if self.controller.ui.collections.selected.is_some() {
                            "Drop to add to selected collection"
                        } else {
                            "Select a collection"
                        };
                        ui.label(RichText::new(text).color(Color32::WHITE));
                    });
                })
                .response;
            if drag_active {
                if let Some(pointer) = pointer_pos {
                    if zone_response.rect.contains(pointer) {
                        self.controller.update_sample_drag(pointer, None, true);
                    }
                }
            }
            ui.add_space(8.0);
            ui.label(RichText::new("Collection items").color(Color32::WHITE));
            egui::ScrollArea::vertical()
                .id_source("collection_items_scroll")
                .show(ui, |ui| {
                    for sample in &self.controller.ui.collections.samples {
                        ui.push_id(format!("{}:{}", sample.source, sample.path), |ui| {
                            ui.label(
                                RichText::new(format!("{} — {}", sample.source, sample.path))
                                    .color(Color32::LIGHT_GRAY),
                            );
                        });
                    }
                });
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
                let needs_refresh = self
                    .waveform_tex
                    .as_ref()
                    .map(|tex| tex.size() != image.image.size)
                    .unwrap_or(true);
                if needs_refresh {
                    self.waveform_tex = Some(ui.ctx().load_texture(
                        "waveform_texture",
                        image.image.clone(),
                        TextureOptions::LINEAR,
                    ));
                }
                self.waveform_tex.as_ref().map(|tex| tex.id())
            } else {
                self.waveform_tex = None;
                None
            };

            if let Some(id) = tex_id {
                painter.image(id, rect, rect, Color32::WHITE);
            } else {
                painter.rect_filled(rect, 6.0, Color32::from_rgb(12, 12, 12));
            }
            painter.rect_stroke(rect, 6.0, Stroke::new(1.0, Color32::from_rgb(64, 64, 64)));
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
        let selected_row = match selected {
            Some(TriageIndex { column: c, row }) if c == column => Some(row),
            _ => None,
        };
        let loaded_row = match loaded {
            Some(TriageIndex { column: c, row }) if c == column => Some(row),
            _ => None,
        };
        const ROW_HEIGHT: f32 = 30.0;
        let total_rows = self.controller.triage_indices(column).len();
        egui::ScrollArea::vertical()
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

                    let mut button = egui::Button::new(RichText::new(label).color(Color32::WHITE))
                        .sense(egui::Sense::click_and_drag());
                    if is_selected {
                        button = button.fill(Color32::from_rgb(30, 30, 30));
                    }
                    ui.push_id(&path, |ui| {
                        let response =
                            ui.add_sized(egui::vec2(ui.available_width(), ROW_HEIGHT), button);
                        if response.clicked() {
                            self.controller.select_wav_by_path(&path);
                        }
                        if response.drag_started() {
                            if let Some(pos) = response.interact_pointer_pos() {
                                let name = path.to_string_lossy().to_string();
                                self.controller.start_sample_drag(path.clone(), name, pos);
                            }
                        } else if drag_active && response.dragged() {
                            if let Some(pos) = response.interact_pointer_pos() {
                                self.controller.update_sample_drag(pos, None, false);
                            }
                        } else if response.drag_stopped() {
                            self.controller.finish_sample_drag();
                        }
                    });
                }
            });
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
