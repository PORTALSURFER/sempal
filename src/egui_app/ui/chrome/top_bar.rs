use eframe::egui::{self, RichText, SliderClamping};

use super::super::EguiApp;
use super::super::style;
use super::buttons;

impl EguiApp {
    pub(crate) fn render_status_controls(&mut self, ui: &mut egui::Ui) {
        let palette = style::palette();
        let mut close_menu = false;
        ui.menu_button("Options", |ui| {
            let palette = style::palette();
            ui.label(RichText::new("Collection export root").color(palette.text_primary));
            let export_label = self
                .controller
                .ui
                .collection_export_root
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "Not set".to_string());
            ui.label(RichText::new(export_label).color(palette.text_muted));
            if ui
                .add(buttons::action_button("Choose collection export root..."))
                .clicked()
            {
                self.controller.pick_collection_export_root();
                close_menu = true;
            }
            if ui
                .add(buttons::action_button("Open collection export root"))
                .clicked()
            {
                self.controller.open_collection_export_root();
                close_menu = true;
            }
            if ui
                .add(buttons::action_button("Clear collection export root"))
                .clicked()
            {
                self.controller.clear_collection_export_root();
                close_menu = true;
            }
            ui.separator();
            ui.label(RichText::new("Trash folder").color(palette.text_primary));
            let trash_label = self
                .controller
                .ui
                .trash_folder
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "Not set".to_string());
            ui.label(RichText::new(trash_label).color(palette.text_muted));
            if ui
                .add(buttons::action_button("Choose trash folder..."))
                .clicked()
            {
                self.controller.pick_trash_folder();
                close_menu = true;
            }
            if ui
                .add(buttons::action_button("Open trash folder"))
                .clicked()
            {
                self.controller.open_trash_folder();
                close_menu = true;
            }
            if ui
                .add(buttons::action_button("Open config folder"))
                .clicked()
            {
                self.controller.open_config_folder();
                close_menu = true;
            }
            if ui
                .add(buttons::action_button("Check for updates"))
                .clicked()
            {
                self.controller.check_for_updates_now();
                close_menu = true;
            }
            ui.separator();
            self.render_audio_options_menu(ui);
            ui.separator();
            self.render_analysis_options_menu(ui);
            ui.separator();
            if ui
                .add(buttons::action_button("Move trashed samples to folder"))
                .clicked()
            {
                self.controller.move_all_trashed_to_folder();
                close_menu = true;
            }
            if ui
                .add(buttons::destructive_button("Take out trash"))
                .clicked()
            {
                self.controller.take_out_trash();
                close_menu = true;
            }
            if close_menu {
                ui.close();
            }
        });
        ui.add_space(10.0);
        const APP_VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));
        match self.controller.ui.update.status {
            crate::egui_app::state::UpdateStatus::Checking => {
                ui.label(RichText::new("Checking updates…").color(palette.text_muted));
                ui.add_space(10.0);
            }
            crate::egui_app::state::UpdateStatus::UpdateAvailable => {
                let label = self
                    .controller
                    .ui
                    .update
                    .available_tag
                    .clone()
                    .unwrap_or_else(|| "Update available".to_string());
                ui.label(
                    RichText::new("Update available")
                        .color(style::destructive_text())
                        .strong(),
                );
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Current:").color(palette.text_muted));
                    ui.label(RichText::new(APP_VERSION).color(palette.text_muted));
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("New:").color(palette.text_muted));
                    ui.label(
                        RichText::new(&label)
                            .color(style::destructive_text())
                            .strong(),
                    );
                });
                if ui.add(buttons::action_button("Open update page")).clicked() {
                    self.controller.open_update_link();
                }
                if ui.add(buttons::action_button("Install")).clicked() {
                    self.controller.install_update_and_exit();
                }
                if ui.add(buttons::action_button("Dismiss")).clicked() {
                    self.controller.dismiss_update_notification();
                }
                ui.add_space(10.0);
            }
            crate::egui_app::state::UpdateStatus::Error => {
                if ui
                    .add(buttons::action_button("Update check failed"))
                    .clicked()
                {
                    self.controller.check_for_updates_now();
                }
                ui.add_space(10.0);
            }
            crate::egui_app::state::UpdateStatus::Idle => {}
        }
        ui.add_space(10.0);
        let mut volume = self.controller.ui.volume;
        let slider = egui::Slider::new(&mut volume, 0.0..=1.0)
            .text("Vol")
            .clamping(SliderClamping::Always);
        if ui.add(slider).changed() {
            self.controller.set_volume(volume);
        }
        if self.controller.ui.progress.visible {
            ui.add_space(10.0);
            let progress = &self.controller.ui.progress;
            let fraction = progress.fraction();
            let mut bar = egui::ProgressBar::new(fraction)
                .desired_width(180.0)
                .animate(true);
            bar = bar.fill(style::status_badge_color(style::StatusTone::Busy));
            bar = if progress.total > 0 {
                bar.text(format!(
                    "{} / {}",
                    progress.completed.min(progress.total),
                    progress.total
                ))
            } else if progress.task == Some(crate::egui_app::state::ProgressTaskKind::Scan)
                && progress.completed > 0
            {
                bar.text(format!("{} files", progress.completed))
            } else {
                bar.text("Working…")
            };
            let tooltip = match progress.detail.as_deref() {
                Some(detail) => format!("{}\n{}", progress.title, detail),
                None => progress.title.clone(),
            };
            ui.add(bar).on_hover_text(tooltip);
            if progress.cancelable {
                let label = if progress.cancel_requested {
                    "Canceling…"
                } else {
                    "Cancel"
                };
                if ui
                    .add_enabled(!progress.cancel_requested, buttons::action_button(label))
                    .clicked()
                {
                    self.controller.ui.progress.cancel_requested = true;
                }
            }
        }
    }
}
