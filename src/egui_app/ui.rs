use std::cell::RefCell;
use std::rc::Rc;

use crate::audio::AudioPlayer;
use crate::egui_app::controller::EguiController;
use crate::egui_app::state::{TriageColumn, TriageIndex};
use crate::waveform::WaveformRenderer;
use eframe::egui::{self, Color32, Frame, Margin, RichText, Stroke, Ui};

/// Thin wrapper that renders the egui UI using the shared controller state.
pub struct EguiApp {
    controller: EguiController,
    visuals_set: bool,
}

impl EguiApp {
    /// Create a new egui app, loading persisted configuration.
    pub fn new(renderer: WaveformRenderer, player: Rc<RefCell<AudioPlayer>>) -> Result<Self, String> {
        let mut controller = EguiController::new(renderer, player);
        controller
            .load_configuration()
            .map_err(|err| format!("Failed to load config: {err}"))?;
        Ok(Self {
            controller,
            visuals_set: false,
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
                        if ui.button(RichText::new("Close").color(Color32::WHITE)).clicked() {
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
                    ui.painter()
                        .circle_filled(ui.cursor().min + egui::vec2(9.0, 11.0), 9.0, status.badge_color);
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
            egui::ScrollArea::vertical().show(ui, |ui| {
                let rows = self.controller.ui.sources.rows.clone();
                let selected = self.controller.ui.sources.selected;
                for (index, row) in rows.iter().enumerate() {
                    let is_selected = Some(index) == selected;
                    let mut response = ui.selectable_label(
                        is_selected,
                        RichText::new(&row.name).color(Color32::WHITE),
                    );
                    response = response.on_hover_text(&row.path);
                    if response.clicked() {
                        self.controller.select_source_by_index(index);
                    }
                    ui.add_space(4.0);
                }
            });
        });
    }

    fn render_collections_panel(&mut self, ui: &mut Ui) {
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
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (index, collection) in rows.iter().enumerate() {
                    let selected = collection.selected;
                    let label = format!("{} ({})", collection.name, collection.count);
                    if ui
                        .selectable_label(
                            selected,
                            RichText::new(label).color(Color32::WHITE),
                        )
                        .clicked()
                    {
                        self.controller.select_collection_by_index(Some(index));
                    }
                    ui.add_space(4.0);
                }
            });
            ui.add_space(8.0);
            ui.label(RichText::new("Collection items").color(Color32::WHITE));
            egui::ScrollArea::vertical().show(ui, |ui| {
                for sample in &self.controller.ui.collections.samples {
                    ui.label(RichText::new(format!("{} — {}", sample.source, sample.path)).color(Color32::LIGHT_GRAY));
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
                        self.controller.ui.waveform.loop_enabled = !loop_enabled;
                    }
                });
            });
            ui.add_space(8.0);
            let available = ui.available_size();
            let rect = ui.allocate_space(available).1;
            let painter = ui.painter();
            painter.rect_filled(rect, 6.0, Color32::from_rgb(12, 12, 12));
            painter.rect_stroke(
                rect,
                6.0,
                Stroke::new(1.0, Color32::from_rgb(64, 64, 64)),
            );
            if let Some(selection) = self.controller.ui.waveform.selection {
                let width = rect.width() * (selection.end() - selection.start()) as f32;
                let x = rect.left() + rect.width() * selection.start() as f32;
                let selection_rect = egui::Rect::from_min_size(
                    egui::pos2(x, rect.top()),
                    egui::vec2(width, rect.height()),
                );
                painter.rect_filled(selection_rect, 4.0, Color32::from_rgba_unmultiplied(28, 63, 106, 90));
            }
            if self.controller.ui.waveform.playhead.visible {
                let x = rect.left() + rect.width() * self.controller.ui.waveform.playhead.position;
                let line = egui::Rect::from_min_max(
                    egui::pos2(x, rect.top()),
                    egui::pos2(x, rect.bottom()),
                );
                painter.rect_stroke(line, 0.0, Stroke::new(2.0, Color32::from_rgb(51, 153, 255)));
            }
        });
    }

    fn render_triage(&mut self, ui: &mut Ui) {
        let spacing = 8.0;
        let triage = self.controller.ui.triage.clone();
        let selected = triage.selected;
        let loaded = triage.loaded;
        let trash_rows = triage.trash;
        let neutral_rows = triage.neutral;
        let keep_rows = triage.keep;

        ui.columns(3, |columns| {
            self.render_triage_column(
                &mut columns[0],
                "Trash",
                &trash_rows,
                TriageColumn::Trash,
                Color32::from_rgb(198, 143, 143),
                selected,
                loaded,
            );
            columns[0].add_space(spacing);
            self.render_triage_column(
                &mut columns[1],
                "Samples",
                &neutral_rows,
                TriageColumn::Neutral,
                Color32::from_rgb(208, 208, 208),
                selected,
                loaded,
            );
            columns[1].add_space(spacing);
            self.render_triage_column(
                &mut columns[2],
                "Keep",
                &keep_rows,
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
        rows: &[crate::egui_app::state::WavRowView],
        column: TriageColumn,
        accent: Color32,
        selected: Option<TriageIndex>,
        loaded: Option<TriageIndex>,
    ) {
        ui.label(RichText::new(title).color(accent));
        ui.add_space(6.0);
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (idx, row) in rows.iter().enumerate() {
                let is_selected = matches!(
                    selected,
                    Some(TriageIndex { column: c, row: r }) if c == column && r == idx
                );
                let is_loaded = matches!(
                    loaded,
                    Some(TriageIndex { column: c, row: r }) if c == column && r == idx
                );

                let mut text = row.name.clone();
                if is_loaded {
                    text.push_str(" • loaded");
                }
                let label = egui::SelectableLabel::new(
                    is_selected,
                    RichText::new(text).color(Color32::WHITE),
                );
                if ui.add(label).clicked() {
                    self.controller.select_wav_by_path(&row.path);
                }
                ui.add_space(4.0);
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
        self.render_status(ctx);
        ctx.request_repaint();
    }
}
