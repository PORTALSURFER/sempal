use eframe::egui::{self, Align, Button, Layout, RichText, ScrollArea};
use std::{path::PathBuf, sync::mpsc, thread};

use sempal::egui_app::ui::style;

use crate::{APP_NAME, install, paths};

pub(crate) fn run_installer_app() -> eframe::Result<()> {
    let mut viewport = egui::ViewportBuilder::default().with_inner_size([600.0, 300.0]);
    if let Some(icon) = load_installer_icon() {
        viewport = viewport.with_icon(icon);
    }
    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "SemPal Installer",
        native_options,
        Box::new(|cc| Ok(Box::new(InstallerApp::new(cc)))),
    )
}

fn load_installer_icon() -> Option<egui::IconData> {
    decode_icon(include_bytes!("../../../assets/logo3.ico")).or_else(|| {
        let fallback = decode_icon(include_bytes!("../../../assets/logo3.png"));
        if fallback.is_none() {
            eprintln!("Failed to decode installer icon assets.");
        }
        fallback
    })
}

fn decode_icon(bytes: &[u8]) -> Option<egui::IconData> {
    let image = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (width, height) = image.dimensions();
    Some(egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    })
}

#[derive(Clone, Copy, PartialEq)]
enum InstallStep {
    Welcome,
    License,
    Location,
    Installing,
    Done,
    Error,
}

struct InstallProgress {
    total_files: usize,
    copied_files: usize,
    current: Option<String>,
}

impl Default for InstallProgress {
    fn default() -> Self {
        Self {
            total_files: 0,
            copied_files: 0,
            current: None,
        }
    }
}

pub(crate) enum InstallerEvent {
    Started { total_files: usize },
    FileCopied { copied_files: usize, name: String },
    Log(String),
    Finished,
    Failed(String),
}

struct InstallerApp {
    step: InstallStep,
    install_dir: PathBuf,
    bundle_dir: PathBuf,
    license_text: String,
    progress: InstallProgress,
    receiver: Option<mpsc::Receiver<InstallerEvent>>,
    error: Option<String>,
    open_folder_on_finish: bool,
    launch_on_finish: bool,
    finish_errors: Vec<String>,
    logs: Vec<String>,
    install_finished: bool,
}

impl InstallerApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut visuals = cc.egui_ctx.style().visuals.clone();
        style::apply_visuals(&mut visuals);
        cc.egui_ctx.set_visuals(visuals);

        Self {
            step: InstallStep::Welcome,
            install_dir: paths::default_install_dir(),
            bundle_dir: paths::default_bundle_dir(),
            license_text: include_str!("../../../LICENSE").to_string(),
            progress: InstallProgress::default(),
            receiver: None,
            error: None,
            open_folder_on_finish: true,
            launch_on_finish: true,
            finish_errors: Vec::new(),
            logs: Vec::new(),
            install_finished: false,
        }
    }

    fn start_install(&mut self) {
        let bundle_dir = self.bundle_dir.clone();
        let install_dir = self.install_dir.clone();
        let (tx, rx) = mpsc::channel();
        self.receiver = Some(rx);
        self.progress = InstallProgress::default();
        self.step = InstallStep::Installing;
        self.install_finished = false;
        self.finish_errors.clear();
        self.logs.clear();
        thread::spawn(move || {
            if let Err(err) = install::run_install(&bundle_dir, &install_dir, tx.clone()) {
                let _ = tx.send(InstallerEvent::Failed(err));
            }
        });
    }

    fn poll_installer(&mut self) {
        let Some(receiver) = &self.receiver else {
            return;
        };
        while let Ok(event) = receiver.try_recv() {
            match event {
                InstallerEvent::Started { total_files } => {
                    self.progress.total_files = total_files;
                }
                InstallerEvent::FileCopied { copied_files, name } => {
                    self.progress.copied_files = copied_files;
                    self.progress.current = Some(name);
                }
                InstallerEvent::Log(message) => {
                    self.logs.push(message);
                }
                InstallerEvent::Finished => {
                    self.install_finished = true;
                }
                InstallerEvent::Failed(err) => {
                    self.error = Some(err);
                    self.step = InstallStep::Error;
                }
            }
        }
    }
}

impl eframe::App for InstallerApp {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.poll_installer();
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(12.0, 12.0);
            ui.heading(APP_NAME);
            ui.add_space(6.0);

            match self.step {
                InstallStep::Welcome => {
                    ui.label("Welcome to the SemPal installer.");
                    ui.label("This will install SemPal and the required ML models.");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Next").clicked() {
                            self.step = InstallStep::License;
                        }
                    });
                }
                InstallStep::License => {
                    ui.label("License");
                    let scroll_height = (ui.available_height() - 64.0).max(160.0);
                    ScrollArea::vertical()
                        .max_height(scroll_height)
                        .show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::multiline(&mut self.license_text)
                                    .desired_rows(16)
                                    .desired_width(ui.available_width())
                                    .font(egui::TextStyle::Monospace)
                                    .interactive(false),
                            );
                        });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Next").clicked() {
                            self.step = InstallStep::Location;
                        }
                        if ui.button("Back").clicked() {
                            self.step = InstallStep::Welcome;
                        }
                    });
                }
                InstallStep::Location => {
                    ui.label("Choose installation folder");
                    ui.horizontal(|ui| {
                        ui.monospace(self.install_dir.display().to_string());
                        if ui.button("Browse").clicked() {
                            if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                                self.install_dir = folder;
                            }
                        }
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Install").clicked() {
                            self.start_install();
                        }
                        if ui.button("Back").clicked() {
                            self.step = InstallStep::License;
                        }
                    });
                }
                InstallStep::Installing => {
                    let progress = (self.progress.copied_files as f32
                        / self.progress.total_files.max(1) as f32)
                        .clamp(0.0, 1.0);
                    ui.label("Installing...");
                    ui.add(egui::ProgressBar::new(progress).show_percentage());
                    if let Some(current) = &self.progress.current {
                        ui.label(format!("Copying {current}"));
                    }
                    ui.separator();
                    ui.label("Install log");
                    let log_width = ui.available_width();
                    let log_color = if self.install_finished {
                        style::palette().success
                    } else {
                        style::palette().warning
                    };
                    ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
                        ui.set_min_width(log_width);
                        for line in &self.logs {
                            ui.label(RichText::new(line).color(log_color));
                        }
                    });
                    if self.install_finished {
                        ui.add_space(8.0);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.button("Continue").clicked() {
                                self.step = InstallStep::Done;
                                self.finish_errors.clear();
                            }
                        });
                    }
                }
                InstallStep::Done => {
                    ui.label(RichText::new("Installation complete.").strong());
                    ui.checkbox(&mut self.open_folder_on_finish, "Open install folder");
                    ui.checkbox(&mut self.launch_on_finish, "Launch SemPal");
                    if !self.finish_errors.is_empty() {
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new("Could not complete all finish actions:")
                                .color(style::palette().warning),
                        );
                        for message in &self.finish_errors {
                            ui.label(RichText::new(message).color(style::palette().warning));
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(Button::new("Finish")).clicked() {
                            self.finish_errors.clear();
                            if self.open_folder_on_finish {
                                if let Err(err) = open::that(&self.install_dir) {
                                    self.finish_errors.push(format!(
                                        "Failed to open install folder: {err}"
                                    ));
                                }
                            }
                            if self.launch_on_finish {
                                let exe = self.install_dir.join("sempal.exe");
                                if let Err(err) = std::process::Command::new(exe).spawn() {
                                    self.finish_errors
                                        .push(format!("Failed to launch SemPal: {err}"));
                                }
                            }
                            if self.finish_errors.is_empty() {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        }
                    });
                }
                InstallStep::Error => {
                    ui.label(RichText::new("Installation failed.").color(style::palette().warning));
                    if let Some(error) = &self.error {
                        ui.label(error);
                    }
                    ui.separator();
                    ui.label("Install log");
                    let log_width = ui.available_width();
                    let log_color = style::semantic_palette().destructive;
                    ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
                        ui.set_min_width(log_width);
                        for line in &self.logs {
                            ui.label(RichText::new(line).color(log_color));
                        }
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                }
            }
        });
    }
}

pub(crate) fn send_started(
    sender: &mpsc::Sender<InstallerEvent>,
    total_files: usize,
) -> Result<(), String> {
    sender
        .send(InstallerEvent::Started { total_files })
        .map_err(|err| format!("Failed to report install start: {err}"))
}

pub(crate) fn send_file_copied(
    sender: &mpsc::Sender<InstallerEvent>,
    copied_files: usize,
    name: String,
) -> Result<(), String> {
    sender
        .send(InstallerEvent::FileCopied { copied_files, name })
        .map_err(|err| format!("Failed to report install progress: {err}"))
}

pub(crate) fn send_finished(sender: &mpsc::Sender<InstallerEvent>) -> Result<(), String> {
    sender
        .send(InstallerEvent::Finished)
        .map_err(|err| format!("Failed to report completion: {err}"))
}

pub(crate) type InstallerSender = mpsc::Sender<InstallerEvent>;
