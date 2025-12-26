use super::hotkey_overlay;
use super::input::InputSnapshot;
use super::progress_overlay;
use crate::egui_app::controller::hotkeys;
use crate::egui_app::state::FocusContext;
use crate::egui_app::ui::EguiApp;
use crate::egui_app::ui::style;
use eframe::egui;
use eframe::egui::{TopBottomPanel, Ui, UiBuilder};

impl EguiApp {
    pub(super) fn apply_visuals(&mut self, ctx: &egui::Context) {
        if self.visuals_set {
            return;
        }
        let mut visuals = egui::Visuals::dark();
        style::apply_visuals(&mut visuals);
        ctx.set_visuals(visuals);
        self.visuals_set = true;
    }

    pub(super) fn ensure_initial_focus(&mut self, ctx: &egui::Context) {
        if self.requested_initial_focus {
            return;
        }
        let is_focused = ctx.input(|i| i.viewport().focused.unwrap_or(false));
        if is_focused {
            self.requested_initial_focus = true;
            return;
        }
        self.requested_initial_focus = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    pub(super) fn render_center(&mut self, ui: &mut Ui) {
        ui.set_min_height(ui.available_height());
        ui.vertical(|ui| {
            self.render_waveform(ui);
            ui.add_space(8.0);
            let browser_rect = ui.available_rect_before_wrap();
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

    pub(super) fn consume_source_panel_drops(&mut self, ctx: &egui::Context) {
        let panel_hit = if self.sources_panel_drop_hovered || self.sources_panel_drop_armed {
            true
        } else if let Some(rect) = self.sources_panel_rect {
            ctx.input(|i| {
                i.pointer
                    .hover_pos()
                    .or_else(|| i.pointer.interact_pos())
                    .is_some_and(|pos| rect.contains(pos))
            })
        } else {
            false
        };
        if !panel_hit {
            return;
        }
        let dropped_files = ctx.input(|i| i.raw.dropped_files.clone());
        if dropped_files.is_empty() {
            return;
        }
        let mut handled_directory = false;
        for file in dropped_files {
            let Some(path) = file.path else {
                continue;
            };
            if !path.is_dir() {
                continue;
            }
            handled_directory = true;
            if let Err(err) = self.controller.add_source_from_path(path) {
                self.controller.set_status(err, style::StatusTone::Error);
            }
        }
        if !handled_directory {
            self.controller.set_status(
                "Drop a folder onto Sources to add it",
                style::StatusTone::Warning,
            );
        }
        self.sources_panel_drop_armed = false;
    }

    pub(super) fn render_ui(
        &mut self,
        ctx: &egui::Context,
        input: &InputSnapshot,
        focus_context: FocusContext,
    ) {
        self.render_panels(ctx);
        self.render_overlays(ctx, input, focus_context);
        ctx.request_repaint();
    }

    fn render_status(&mut self, ctx: &egui::Context) {
        TopBottomPanel::top("status_bar")
            .frame(egui::Frame::default())
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    self.render_status_controls(ui);
                    let palette = style::palette();
                    const APP_VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));
                    ui.allocate_ui_with_layout(
                        ui.available_size(),
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            if !matches!(
                                self.controller.ui.update.status,
                                crate::egui_app::state::UpdateStatus::UpdateAvailable
                            ) {
                                ui.label(
                                    egui::RichText::new(APP_VERSION).color(palette.text_muted),
                                );
                            }
                        },
                    );
                });
            });
    }

    fn render_panels(&mut self, ctx: &egui::Context) {
        self.render_status(ctx);
        egui::SidePanel::left("sources")
            .resizable(true)
            .default_width(260.0)
            .min_width(220.0)
            .max_width(520.0)
            .show(ctx, |ui| self.render_sources_panel(ui));
        self.consume_source_panel_drops(ctx);
        egui::SidePanel::right("collections")
            .resizable(true)
            .default_width(260.0)
            .min_width(220.0)
            .max_width(560.0)
            .show(ctx, |ui| self.render_collections_panel(ui));
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.set_min_height(ui.available_height());
            self.render_center(ui);
        });
    }

    fn render_overlays(
        &mut self,
        ctx: &egui::Context,
        input: &InputSnapshot,
        focus_context: FocusContext,
    ) {
        self.render_drag_overlay(ctx);
        self.render_audio_settings_window(ctx);
        progress_overlay::render_progress_overlay(ctx, &mut self.controller.ui.progress);
        self.render_feedback_issue_prompt(ctx);
        self.render_map_window(ctx);
        if self.controller.ui.hotkeys.overlay_visible {
            if input.escape {
                self.controller.ui.hotkeys.overlay_visible = false;
            }
            let focus_actions = hotkeys::focused_actions(focus_context);
            let global_actions = hotkeys::global_actions();
            hotkey_overlay::render_hotkey_overlay(
                ctx,
                focus_context,
                &focus_actions,
                &global_actions,
                &mut self.controller.ui.hotkeys.overlay_visible,
            );
        }
    }
}
