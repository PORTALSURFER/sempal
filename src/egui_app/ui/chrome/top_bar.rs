use eframe::egui::{self, RichText, SliderClamping};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::super::EguiApp;
use super::super::style;
use super::buttons;

impl EguiApp {
    pub(crate) fn render_status_controls(&mut self, ui: &mut egui::Ui) {
        let palette = style::palette();
        let mut close_menu = false;
        ui.menu_button("Options", |ui| {
            let palette = style::palette();
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
            ui.add(bar).on_hover_ui(|ui| {
                ui.label(&progress.title);
                if let Some(detail) = progress.detail.as_deref() {
                    ui.label(detail);
                }
                if let Some(snapshot) = progress.analysis.as_ref() {
                    ui.add_space(4.0);
                    let jobs_fraction = progress.fraction();
                    let jobs_label = format!(
                        "Jobs {}/{}",
                        progress.completed.min(progress.total),
                        progress.total
                    );
                    ui.label(format!(
                        "Queue: {} pending • {} running • {} failed",
                        snapshot.pending, snapshot.running, snapshot.failed
                    ));
                    ui.add(
                        egui::ProgressBar::new(jobs_fraction)
                            .desired_width(180.0)
                            .text(jobs_label),
                    );
                    let samples_fraction = if snapshot.samples_total == 0 {
                        0.0
                    } else {
                        snapshot.samples_completed as f32 / snapshot.samples_total as f32
                    };
                    let samples_label = format!(
                        "Samples {}/{}",
                        snapshot.samples_completed, snapshot.samples_total
                    );
                    ui.add(
                        egui::ProgressBar::new(samples_fraction.clamp(0.0, 1.0))
                            .desired_width(180.0)
                            .text(samples_label),
                    );
                    if !snapshot.running_jobs.is_empty() {
                        ui.add_space(4.0);
                        ui.label("Running jobs");
                        let now_epoch = now_epoch_seconds();
                        for job in &snapshot.running_jobs {
                            let age = job.last_heartbeat_at.and_then(|ts| {
                                now_epoch
                                    .and_then(|now| now.checked_sub(ts))
                                    .and_then(|delta| u64::try_from(delta).ok())
                                    .map(Duration::from_secs)
                            });
                            let age_label = age
                                .map(format_elapsed)
                                .unwrap_or_else(|| "unknown".to_string());
                            let mut line = format!("{} • last heartbeat {}", job.label, age_label);
                            if job.possibly_stalled {
                                line.push_str(" • possibly stalled");
                            }
                            ui.label(line);
                            if let (Some(age), Some(stale_after)) = (age, snapshot.stale_after_secs)
                            {
                                let fraction =
                                    (age.as_secs_f32() / stale_after as f32).clamp(0.0, 1.0);
                                ui.add(
                                    egui::ProgressBar::new(fraction)
                                        .desired_width(180.0)
                                        .text(format!("{}s timeout", stale_after)),
                                );
                            }
                        }
                    }
                }
                ui.add_space(4.0);
                let heartbeat = heartbeat_frame(ui.input(|i| i.time));
                if let Some(last_update) = progress.last_update_at {
                    ui.label(format!(
                        "Heartbeat {} • last update {} ago",
                        heartbeat,
                        format_elapsed(last_update.elapsed())
                    ));
                } else {
                    ui.label(format!("Heartbeat {}", heartbeat));
                }
                if let Some(last_progress) = progress.last_progress_at {
                    ui.label(format!(
                        "Last progress {} ago",
                        format_elapsed(last_progress.elapsed())
                    ));
                }
            });
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

fn heartbeat_frame(time: f64) -> &'static str {
    const FRAMES: [&str; 4] = ["|", "/", "-", "\\"];
    let idx = ((time * 4.0) as usize) % FRAMES.len();
    FRAMES[idx]
}

fn format_elapsed(elapsed: Duration) -> String {
    let millis = elapsed.as_millis();
    if millis < 1000 {
        format!("{}ms", millis)
    } else if millis < 60_000 {
        format!("{:.1}s", elapsed.as_secs_f32())
    } else {
        let secs = elapsed.as_secs();
        let minutes = secs / 60;
        let rem = secs % 60;
        format!("{}m {:02}s", minutes, rem)
    }
}

fn now_epoch_seconds() -> Option<i64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs() as i64)
}
