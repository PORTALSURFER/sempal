//! egui renderer for the application UI.

mod chrome;
mod collections_panel;
mod drag_overlay;
mod helpers;
mod sources_panel;
mod triage_panel;
mod waveform_view;

use crate::{
    audio::AudioPlayer,
    egui_app::controller::EguiController,
    egui_app::state::TriageColumn,
    waveform::WaveformRenderer,
};
use eframe::egui;
use eframe::egui::{Color32, TextureHandle, Ui};

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
        player: Option<std::rc::Rc<std::cell::RefCell<AudioPlayer>>>,
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

    fn render_center(&mut self, ui: &mut Ui) {
        ui.vertical(|ui| {
            self.render_waveform(ui);
            ui.add_space(8.0);
            let triage_top = ui.cursor().min.y;
            let triage_rect = egui::Rect::from_min_max(
                egui::pos2(ui.max_rect().left(), triage_top),
                ui.max_rect().max,
            );
            if triage_rect.height() > 0.0 {
                let mut triage_ui = ui.child_ui_with_id_source(
                    triage_rect,
                    egui::Layout::top_down(egui::Align::Min),
                    "triage_area",
                );
                self.render_triage(&mut triage_ui);
            }
        });
    }
}

impl eframe::App for EguiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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
                let target = if matches!(col, Some(TriageColumn::Trash)) {
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
        self.render_status(ctx);
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
        self.render_drag_overlay(ctx);
        ctx.request_repaint();
    }
}
