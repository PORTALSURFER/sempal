use super::hotkey_overlay;
use super::input::InputSnapshot;
use super::progress_overlay;
use crate::egui_app::controller::hotkeys;
use crate::egui_app::state::FocusContext;
use crate::egui_app::ui::{EguiApp, helpers};
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
                ui.scope_builder(
                    UiBuilder::new()
                        .id_salt("sample_browser_area")
                        .max_rect(browser_rect)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                    |ui| {
                        ui.set_min_height(ui.available_height());
                        self.render_sample_browser(ui);
                    },
                );
            }
        });
    }

    pub(super) fn consume_source_panel_drops(&mut self, ctx: &egui::Context) {
        if self.external_drop_handled {
            self.sources_panel_drop_armed = false;
            return;
        }
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
        self.external_drop_handled = false;
        self.update_external_drop_hover(ctx);
        let modal_active = self.modal_overlay_active();
        helpers::set_tooltips_suppressed(ctx, modal_active);
        self.controller.refresh_recording_waveform();
        self.render_panels(ctx);
        self.render_overlays(ctx, input, focus_context);
        
        self.controller.set_repaint_signal(ctx.clone());

        // Only repaint when necessary to reduce idle CPU usage
        if self.controller.is_playing()
            || self.controller.ui.drag.payload.is_some()
            || self.controller.is_recording()
            || self.controller.ui.waveform.loading.is_some()
            || self.controller.ui.waveform.copy_flash_at.is_some()
            || self.controller.ui.browser.copy_flash_at.is_some()
        {
            ctx.request_repaint();
        }
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
                                ui.add_space(8.0);
                                if ui
                                    .add(crate::egui_app::ui::chrome::buttons::action_button(
                                        "Report issue",
                                    ))
                                    .clicked()
                                {
                                    self.controller.open_feedback_issue_prompt();
                                }
                                if ui
                                    .add(crate::egui_app::ui::chrome::buttons::action_button(
                                        "Donate",
                                    ))
                                    .clicked()
                                {
                                    if let Err(err) =
                                        open::that("https://www.buymeacoffee.com/portalsurfer")
                                    {
                                        self.controller.set_status(
                                            format!("Failed to open donate link: {err}"),
                                            style::StatusTone::Warning,
                                        );
                                    }
                                }
                                let mode = self.controller.ui.controls.tooltip_mode;
                                let color = match mode {
                                    crate::sample_sources::config::TooltipMode::Off => palette.text_muted,
                                    crate::sample_sources::config::TooltipMode::Regular => palette.accent_mint,
                                    crate::sample_sources::config::TooltipMode::Extended => palette.accent_copper,
                                };
                                let hints_btn =
                                    egui::Button::new(egui::RichText::new("?").color(color))
                                        .frame(false);
                                let hints_resp = ui.add(hints_btn);

                                helpers::tooltip(
                                    hints_resp.clone(),
                                    "Tooltip Mode",
                                    "Cycle between Off (dark), Regular (mint), and Extended (copper) detail levels for all UI hints.",
                                    mode,
                                );

                                if hints_resp.clicked() {
                                    let next_mode = match mode {
                                        crate::sample_sources::config::TooltipMode::Off => crate::sample_sources::config::TooltipMode::Regular,
                                        crate::sample_sources::config::TooltipMode::Regular => crate::sample_sources::config::TooltipMode::Extended,
                                        crate::sample_sources::config::TooltipMode::Extended => crate::sample_sources::config::TooltipMode::Off,
                                    };
                                    self.controller.set_tooltip_mode(next_mode);
                                }
                            }
                        },
                    );
                });
            });
    }

    fn render_panels(&mut self, ctx: &egui::Context) {
        self.render_status(ctx);
        self.render_drop_target_status(ctx);
        egui::SidePanel::left("sources")
            .resizable(true)
            .default_width(260.0)
            .min_width(220.0)
            .max_width(520.0)
            .show(ctx, |ui| self.render_sources_panel(ui));
        self.consume_source_panel_drops(ctx);
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.set_min_height(ui.available_height());
            self.render_center(ui);
        });
    }

    fn render_drop_target_status(&mut self, ctx: &egui::Context) {
        TopBottomPanel::bottom("drop_target_status")
            .exact_height(22.0)
            .frame(egui::Frame::default().fill(style::palette().bg_secondary))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    let selected = self.controller.ui.sources.drop_targets.selected;
                    let row = selected.and_then(|index| {
                        self.controller
                            .ui
                            .sources
                            .drop_targets
                            .rows
                            .get(index)
                    });
                    if let Some(row) = row {
                        ui.label(egui::RichText::new(row.path.display().to_string()));
                    }
                });
            });
    }

    fn render_overlays(
        &mut self,
        ctx: &egui::Context,
        input: &InputSnapshot,
        focus_context: FocusContext,
    ) {
        let hotkey_overlay_visible = self.controller.ui.hotkeys.overlay_visible;
        let modal_blocking_overlays =
            self.modal_overlay_blocks_overlays() || hotkey_overlay_visible;
        if !modal_blocking_overlays {
            self.render_drag_overlay(ctx);
        }
        self.render_audio_settings_window(ctx);
        progress_overlay::render_progress_overlay(ctx, &mut self.controller.ui.progress);
        self.render_feedback_issue_prompt(ctx);
        self.render_loop_crossfade_prompt(ctx);
        self.render_map_window(ctx);
        if hotkey_overlay_visible && !self.modal_overlay_blocks_overlays() {
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

    fn modal_overlay_active(&self) -> bool {
        self.modal_overlay_blocks_overlays() || self.controller.ui.hotkeys.overlay_visible
    }

    fn modal_overlay_blocks_overlays(&self) -> bool {
        (self.controller.ui.progress.visible && self.controller.ui.progress.modal)
            || self.controller.ui.feedback_issue.open
            || self.controller.ui.feedback_issue.token_modal_open
            || self.controller.ui.loop_crossfade_prompt.is_some()
    }

    fn update_external_drop_hover(&mut self, ctx: &egui::Context) {
        let (hovered_files, dropped_files, pointer_pos) = ctx.input(|i| {
            let pointer_pos = i.pointer.hover_pos().or_else(|| i.pointer.interact_pos());
            (
                !i.raw.hovered_files.is_empty(),
                !i.raw.dropped_files.is_empty(),
                pointer_pos,
            )
        });
        if hovered_files || dropped_files {
            if let Some(pos) = pointer_pos {
                self.external_drop_hover_pos = Some(pos);
            }
            return;
        }
        self.external_drop_hover_pos = None;
    }
}
