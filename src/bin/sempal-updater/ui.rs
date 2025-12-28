use std::{
    sync::mpsc::{self, Receiver},
    thread,
};
use eframe::egui::{self, RichText};
use sempal::{
    egui_app::ui::style,
    updater::{
        APP_NAME, ApplyPlan, ReleaseSummary, UpdateChannel, UpdateProgress, UpdaterRunArgs,
        apply_update_with_progress, list_recent_releases,
    },
};
const MAX_LOG_LINES: usize = 200;
const RELEASE_LIST_LIMIT: usize = 5;
pub fn run_gui(args: UpdaterRunArgs) -> Result<(), String> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([560.0, 420.0])
            .with_min_inner_size([440.0, 320.0])
            .with_title(format!("{APP_NAME} updater")),
        ..Default::default()
    };
    eframe::run_native(
        &format!("{APP_NAME} updater"),
        options,
        Box::new(|_cc| Ok(Box::new(UpdateUiApp::new(args)))),
    )
    .map_err(|err| err.to_string())
}

struct UpdateUiApp {
    args: UpdaterRunArgs,
    visuals_set: bool,
    release_state: ReleaseState,
    release_rx: Option<Receiver<Result<Vec<ReleaseSummary>, String>>>,
    selected_tag: Option<String>,
    status: UiStatus,
    log: Vec<String>,
    progress_rx: Option<Receiver<UpdateProgress>>,
    result_rx: Option<Receiver<Result<ApplyPlan, String>>>,
}

impl UpdateUiApp {
    fn new(args: UpdaterRunArgs) -> Self {
        let mut app = Self {
            args,
            visuals_set: false,
            release_state: ReleaseState::Idle,
            release_rx: None,
            selected_tag: None,
            status: UiStatus::Idle,
            log: Vec::new(),
            progress_rx: None,
            result_rx: None,
        };
        app.refresh_release_list();
        app
    }

    fn refresh_release_list(&mut self) {
        if self.args.identity.channel == UpdateChannel::Nightly {
            self.release_state = ReleaseState::Loaded(vec![ReleaseOption {
                tag: "nightly".to_string(),
                label: "nightly".to_string(),
            }]);
            self.selected_tag = Some("nightly".to_string());
            return;
        }
        let repo = self.args.repo.clone();
        let identity = self.args.identity.clone();
        let channel = self.args.identity.channel;
        let (tx, rx) = mpsc::channel();
        self.release_state = ReleaseState::Loading;
        self.release_rx = Some(rx);
        thread::spawn(move || {
            let result = list_recent_releases(&repo, channel, &identity, RELEASE_LIST_LIMIT)
                .map_err(|err| err.to_string());
            let _ = tx.send(result);
        });
    }

    fn ensure_selected_tag(&mut self) {
        if self.selected_tag.is_some() {
            return;
        }
        if let ReleaseState::Loaded(options) = &self.release_state {
            if let Some(first) = options.first() {
                self.selected_tag = Some(first.tag.clone());
            }
        }
    }

    fn start_update(&mut self) {
        if matches!(self.status, UiStatus::Updating) {
            return;
        }
        let mut args = self.args.clone();
        args.requested_tag = self.selected_tag.clone();
        let (progress_tx, progress_rx) = mpsc::channel();
        let (result_tx, result_rx) = mpsc::channel();
        self.progress_rx = Some(progress_rx);
        self.result_rx = Some(result_rx);
        self.log.clear();
        self.push_log("Starting update...");
        self.status = UiStatus::Updating;
        thread::spawn(move || {
            let result = apply_update_with_progress(args, |progress| {
                let _ = progress_tx.send(progress);
            })
            .map_err(|err| err.to_string());
            let _ = result_tx.send(result);
        });
    }

    fn push_log(&mut self, message: impl Into<String>) {
        self.log.push(message.into());
        if self.log.len() > MAX_LOG_LINES {
            let trim = self.log.len() - MAX_LOG_LINES;
            self.log.drain(0..trim);
        }
    }

    fn handle_background_updates(&mut self, ctx: &egui::Context) {
        self.handle_release_updates(ctx);
        self.handle_progress_updates(ctx);
        self.handle_result_updates(ctx);
    }

    fn handle_release_updates(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.release_rx else {
            return;
        };
        if let Ok(result) = rx.try_recv() {
            self.release_rx = None;
            match result {
                Ok(list) => {
                    let options = list.into_iter().map(format_release_option).collect();
                    self.release_state = ReleaseState::Loaded(options);
                    self.ensure_selected_tag();
                }
                Err(err) => {
                    self.release_state = ReleaseState::Error(err);
                }
            }
            ctx.request_repaint();
        }
    }

    fn handle_progress_updates(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.progress_rx else {
            return;
        };
        let messages: Vec<String> = rx
            .try_iter()
            .map(|progress| progress.message)
            .collect();
        if messages.is_empty() {
            return;
        }
        for message in messages {
            self.push_log(message);
        }
        if !self.log.is_empty() {
            ctx.request_repaint();
        }
    }

    fn handle_result_updates(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.result_rx else {
            return;
        };
        if let Ok(result) = rx.try_recv() {
            self.result_rx = None;
            self.progress_rx = None;
            match result {
                Ok(plan) => {
                    self.push_log(format!(
                        "Installed {} into {}",
                        plan.release_tag,
                        plan.install_dir.display()
                    ));
                    self.status = UiStatus::Success(format!(
                        "Updated to {}",
                        plan.release_tag
                    ));
                }
                Err(err) => {
                    self.push_log(format!("Update failed: {err}"));
                    self.status = UiStatus::Error(err);
                }
            }
            ctx.request_repaint();
        }
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

    fn render_status(&self, ui: &mut egui::Ui) {
        let palette = style::palette();
        match &self.status {
            UiStatus::Idle => {
                ui.label(RichText::new("Ready to update.").color(palette.text_muted));
            }
            UiStatus::Updating => {
                ui.label(RichText::new("Applying update...").color(palette.accent_ice));
            }
            UiStatus::Success(message) => {
                ui.label(RichText::new(message).color(palette.accent_mint));
            }
            UiStatus::Error(message) => {
                ui.label(RichText::new(message).color(palette.warning));
            }
        }
    }

    fn render_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let palette = style::palette();
        ui.spacing_mut().item_spacing = egui::vec2(10.0, 8.0);
        self.render_header(ui, &palette);
        self.render_metadata(ui, &palette);
        self.render_version_selector(ui, &palette);
        self.render_status(ui);
        self.render_actions(ui, ctx);
        self.render_progress(ui, &palette);
    }

    fn render_header(&self, ui: &mut egui::Ui, palette: &style::Palette) {
        ui.vertical_centered(|ui| {
            ui.heading(RichText::new(format!("{APP_NAME} updater")).color(palette.text_primary));
            ui.label(
                RichText::new("Install updates or pick a recent version to downgrade.")
                    .color(palette.text_muted),
            );
        });
        ui.add_space(8.0);
    }

    fn render_metadata(&self, ui: &mut egui::Ui, palette: &style::Palette) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Install dir:").color(palette.text_muted));
            ui.label(
                RichText::new(self.args.install_dir.display().to_string())
                    .color(palette.text_primary),
            );
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("Channel:").color(palette.text_muted));
            ui.label(
                RichText::new(channel_label(self.args.identity.channel))
                    .color(palette.text_primary),
            );
        });
    }

    fn render_version_selector(&mut self, ui: &mut egui::Ui, palette: &style::Palette) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Version:").color(palette.text_muted));
            let mut refresh_clicked = false;
            let mut ensure_selected = false;
            match self.release_state.clone() {
                ReleaseState::Loading => {
                    ui.label(RichText::new("Loading...").color(palette.text_muted));
                }
                ReleaseState::Error(message) => {
                    ui.label(RichText::new("Unavailable").color(palette.warning));
                    if ui.button("Retry").clicked() {
                        refresh_clicked = true;
                    }
                    ui.label(RichText::new(message).color(palette.text_muted).small());
                }
                ReleaseState::Loaded(options) => {
                    if options.is_empty() {
                        ui.label(RichText::new("No releases found").color(palette.warning));
                        if ui.button("Refresh").clicked() {
                            refresh_clicked = true;
                        }
                    } else {
                        if self.selected_tag.is_none() {
                            ensure_selected = true;
                        }
                        let selected = self
                            .selected_tag
                            .clone()
                            .unwrap_or_else(|| options[0].tag.clone());
                        egui::ComboBox::from_id_source("version_select")
                            .selected_text(selected_label(&options, &selected))
                            .show_ui(ui, |ui| {
                                for option in options.iter() {
                                    ui.selectable_value(
                                        &mut self.selected_tag,
                                        Some(option.tag.clone()),
                                        &option.label,
                                    );
                                }
                            });
                        if ui.button("Refresh").clicked() {
                            refresh_clicked = true;
                        }
                    }
                }
                ReleaseState::Idle => {
                    ui.label(RichText::new("Waiting...").color(palette.text_muted));
                }
            }
            if ensure_selected {
                self.ensure_selected_tag();
            }
            if refresh_clicked {
                self.refresh_release_list();
            }
        });
        ui.add_space(8.0);
    }

    fn render_actions(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let updating = matches!(self.status, UiStatus::Updating);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!updating, egui::Button::new("Install update"))
                .clicked()
            {
                self.start_update();
            }
            if ui.button("Close").clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
        ui.add_space(8.0);
    }

    fn render_progress(&self, ui: &mut egui::Ui, palette: &style::Palette) {
        ui.label(RichText::new("Progress").color(palette.text_muted));
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .max_height(160.0)
            .show(ui, |ui| {
                if self.log.is_empty() {
                    ui.label(RichText::new("No activity yet.").color(palette.text_muted));
                } else {
                    for line in &self.log {
                        ui.label(RichText::new(line).color(palette.text_primary));
                    }
                }
            });
    }
}

impl eframe::App for UpdateUiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.apply_visuals(ctx);
        self.handle_background_updates(ctx);
        egui::CentralPanel::default().show(ctx, |ui| self.render_panel(ui, ctx));
    }
}

#[derive(Debug, Clone)]
struct ReleaseOption {
    tag: String,
    label: String,
}

#[derive(Debug, Clone)]
enum ReleaseState {
    Idle,
    Loading,
    Loaded(Vec<ReleaseOption>),
    Error(String),
}

#[derive(Debug, Clone)]
enum UiStatus {
    Idle,
    Updating,
    Success(String),
    Error(String),
}

fn format_release_option(summary: ReleaseSummary) -> ReleaseOption {
    let label = match summary.published_at.as_deref() {
        Some(date) => format!("{} ({})", summary.tag, short_date(date)),
        None => summary.tag.clone(),
    };
    ReleaseOption {
        tag: summary.tag,
        label,
    }
}

fn short_date(value: &str) -> String {
    value.get(0..10).unwrap_or(value).to_string()
}

fn selected_label(options: &[ReleaseOption], tag: &str) -> String {
    options
        .iter()
        .find(|option| option.tag == tag)
        .map(|option| option.label.clone())
        .unwrap_or_else(|| tag.to_string())
}

fn channel_label(channel: UpdateChannel) -> &'static str {
    match channel {
        UpdateChannel::Stable => "stable",
        UpdateChannel::Nightly => "nightly",
    }
}
