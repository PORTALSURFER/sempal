//! egui renderer for the application UI.

mod chrome;
mod collections_panel;
mod drag_overlay;
mod helpers;
mod sample_browser_panel;
mod sample_menus;
mod sources_panel;
mod waveform_view;

/// Default viewport sizes used when creating or restoring the window.
pub const DEFAULT_VIEWPORT_SIZE: [f32; 2] = [960.0, 560.0];
pub const MIN_VIEWPORT_SIZE: [f32; 2] = [640.0, 400.0];

use crate::{
    audio::AudioPlayer, egui_app::controller::EguiController, egui_app::state::TriageFlagColumn,
    waveform::WaveformRenderer,
};
use eframe::egui;
use eframe::egui::{Color32, TextureHandle, Ui, UiBuilder};

/// Renders the egui UI using the shared controller state.
pub struct EguiApp {
    controller: EguiController,
    visuals_set: bool,
    waveform_tex: Option<TextureHandle>,
    is_fullscreen: bool,
    windowed_size: egui::Vec2,
    window_was_maximized: bool,
    pending_window_restore: bool,
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
            is_fullscreen: false,
            windowed_size: Self::default_windowed_size(),
            window_was_maximized: false,
            pending_window_restore: false,
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

    fn default_windowed_size() -> egui::Vec2 {
        egui::vec2(DEFAULT_VIEWPORT_SIZE[0], DEFAULT_VIEWPORT_SIZE[1])
    }

    fn update_viewport_state(&mut self, ctx: &egui::Context) {
        let (inner_rect, maximized) =
            ctx.input(|i| (i.viewport().inner_rect, i.viewport().maximized));
        if !self.is_fullscreen {
            if let Some(maximized) = maximized {
                self.window_was_maximized = maximized;
            }
            if let Some(inner_rect) = inner_rect {
                self.windowed_size = inner_rect.size();
            }
        }
    }

    fn toggle_fullscreen(&mut self, ctx: &egui::Context) {
        let was_fullscreen = self.is_fullscreen;
        let new_fullscreen = !was_fullscreen;
        if !self.is_fullscreen {
            if let Some(inner_rect) = ctx.input(|i| i.viewport().inner_rect) {
                self.windowed_size = inner_rect.size();
            }
            if let Some(maximized) = ctx.input(|i| i.viewport().maximized) {
                self.window_was_maximized = maximized;
            }
        }
        self.is_fullscreen = new_fullscreen;
        self.pending_window_restore = was_fullscreen;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(new_fullscreen));
    }

    fn handle_fullscreen_shortcut(&mut self, ctx: &egui::Context) {
        let mut should_toggle = false;
        ctx.input(|i| {
            for ev in &i.events {
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    repeat,
                    ..
                } = ev
                {
                    if *key == egui::Key::F11 && !repeat {
                        should_toggle = true;
                    }
                }
            }
        });

        if should_toggle {
            self.toggle_fullscreen(ctx);
        }
    }

    fn render_center(&mut self, ui: &mut Ui) {
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
                self.render_sample_browser(&mut browser_ui);
            }
        });
    }

    fn restore_windowed_viewport(&mut self, ctx: &egui::Context) {
        if !self.pending_window_restore {
            return;
        }
        let fullscreen_state = ctx.input(|i| i.viewport().fullscreen);
        if fullscreen_state == Some(false) {
            if self.window_was_maximized {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(self.windowed_size));
            }
            self.pending_window_restore = false;
        }
    }
}

impl eframe::App for EguiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.apply_visuals(ctx);
        self.update_viewport_state(ctx);
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
        self.handle_fullscreen_shortcut(ctx);
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
                if browser_has_selection {
                    self.controller.move_selection_column(1);
                }
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
            } else if browser_has_selection {
                self.controller.tag_selected_left();
            }
        }
        self.restore_windowed_viewport(ctx);
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
