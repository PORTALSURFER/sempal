//! egui renderer for the application UI.

mod chrome;
mod collections_panel;
mod drag_overlay;
mod helpers;
mod sample_browser_panel;
mod sample_menus;
mod sources_panel;
pub mod style;
mod waveform_view;

/// Default viewport sizes used when creating or restoring the window.
pub const DEFAULT_VIEWPORT_SIZE: [f32; 2] = [960.0, 560.0];
pub const MIN_VIEWPORT_SIZE: [f32; 2] = [640.0, 400.0];

use crate::{
    audio::AudioPlayer, egui_app::controller::EguiController, egui_app::state::TriageFlagColumn,
    sample_sources::SampleTag, waveform::WaveformRenderer,
};
use eframe::egui;
use eframe::egui::{TextureHandle, Ui, UiBuilder};

/// Renders the egui UI using the shared controller state.
pub struct EguiApp {
    controller: EguiController,
    visuals_set: bool,
    waveform_tex: Option<TextureHandle>,
    last_viewport_log: Option<(u32, u32, u32, u32, &'static str)>,
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
            last_viewport_log: None,
        })
    }

    fn apply_visuals(&mut self, ctx: &egui::Context) {
        if self.visuals_set {
            return;
        }
        let mut visuals = egui::Visuals::dark();
        style::apply_visuals(&mut visuals);
        ctx.set_visuals(visuals);
        self.visuals_set = true;
    }

    fn render_center(&mut self, ui: &mut Ui) {
        ui.set_min_height(ui.available_height());
        ui.vertical(|ui| {
            self.render_waveform(ui);
            ui.add_space(8.0);
            let browser_top = ui.cursor().min.y;
            let browser_rect = egui::Rect::from_min_max(
                egui::pos2(ui.max_rect().left(), browser_top),
                ui.max_rect().max,
            );
            if browser_rect.height() > 0.0 {
                let mut browser_ui = ui.new_child(
                    UiBuilder::new()
                        .id_salt("sample_browser_area")
                        .max_rect(browser_rect)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                browser_ui.set_min_height(browser_ui.available_height());
                self.render_sample_browser(&mut browser_ui);
            }
        });
    }
}

impl eframe::App for EguiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.apply_visuals(ctx);
        self.controller.tick_playhead();
        if let Some(pos) = ctx.input(|i| i.pointer.hover_pos().or_else(|| i.pointer.interact_pos()))
        {
            self.controller.refresh_drag_position(pos);
        }
        if self.controller.ui.drag.payload.is_some() && !ctx.input(|i| i.pointer.primary_down()) {
            self.controller.finish_active_drag();
        }
        let collection_focus = self.controller.ui.collections.selected_sample.is_some();
        let browser_has_selection = self.controller.ui.browser.selected.is_some();
        if collection_focus {
            self.controller.ui.browser.autoscroll = false;
            self.controller.ui.browser.selected = None;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            self.controller.toggle_play_pause();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if !self.controller.ui.browser.selected_paths.is_empty() {
                self.controller.clear_browser_selection();
            }
        }
        if let Some(new_maximized) = ctx.input(|i| {
            if i.key_pressed(egui::Key::F11) {
                Some(!i.viewport().maximized.unwrap_or(false))
            } else {
                None
            }
        }) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(new_maximized));
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown) && i.modifiers.shift) {
            if collection_focus {
                self.controller.nudge_collection_sample(1);
            } else {
                self.controller.grow_selection(1);
            }
        } else if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
            if collection_focus {
                self.controller.nudge_collection_sample(1);
            } else {
                self.controller.nudge_selection(1);
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp) && i.modifiers.shift) {
            if collection_focus {
                self.controller.nudge_collection_sample(-1);
            } else {
                self.controller.grow_selection(-1);
            }
        } else if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
            if collection_focus {
                self.controller.nudge_collection_sample(-1);
            } else {
                self.controller.nudge_selection(-1);
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
            if ctx.input(|i| i.modifiers.ctrl) {
                if browser_has_selection {
                    self.controller.move_selection_column(1);
                }
            } else if collection_focus {
                self.controller
                    .tag_selected_collection_sample(SampleTag::Keep);
            } else if browser_has_selection {
                let col = self.controller.ui.browser.selected.map(|t| t.column);
                let target = if matches!(col, Some(TriageFlagColumn::Trash)) {
                    crate::sample_sources::SampleTag::Neutral
                } else {
                    crate::sample_sources::SampleTag::Keep
                };
                self.controller.tag_selected(target);
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
            if ctx.input(|i| i.modifiers.ctrl) {
                if browser_has_selection {
                    self.controller.move_selection_column(-1);
                }
            } else if collection_focus {
                self.controller.tag_selected_collection_left();
            } else if browser_has_selection {
                self.controller.tag_selected_left();
            }
        }
        if !collection_focus && ctx.input(|i| i.key_pressed(egui::Key::X)) {
            self.controller.toggle_focused_selection();
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
            ui.set_min_height(ui.available_height());
            self.render_center(ui);
        });
        self.render_drag_overlay(ctx);
        ctx.request_repaint();
    }
}
