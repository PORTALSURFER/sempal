use eframe::egui::{self, Align, Button, Layout, RichText, ScrollArea};
use std::{
    env,
    fs,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
};

use sempal::app_dirs;
use sempal::egui_app::ui::style;

#[cfg(target_os = "windows")]
use winreg::enums::{HKEY_CURRENT_USER, KEY_WRITE};
#[cfg(target_os = "windows")]
use winreg::RegKey;

const APP_NAME: &str = "SemPal";
const APP_PUBLISHER: &str = "SemPal";
const UNINSTALL_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\SemPal";

fn main() -> eframe::Result<()> {
    if env::args().any(|arg| arg == "--uninstall") {
        if let Err(err) = run_uninstall() {
            eprintln!("Uninstall failed: {err}");
        }
        return Ok(());
    }

    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "SemPal Installer",
        native_options,
        Box::new(|cc| Ok(Box::new(InstallerApp::new(cc)))),
    )
}

fn run_uninstall() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let uninstall_key = hkcu
            .open_subkey_with_flags(UNINSTALL_KEY, KEY_WRITE)
            .map_err(|err| format!("Failed to open uninstall registry key: {err}"))?;
        let install_location: String = uninstall_key
            .get_value("InstallLocation")
            .map_err(|err| format!("Failed to read InstallLocation: {err}"))?;
        schedule_delete_after_exit(&install_location)?;
        hkcu.delete_subkey_all(UNINSTALL_KEY)
            .map_err(|err| format!("Failed to remove uninstall registry key: {err}"))?;
        return Ok(());
    }
    #[allow(unreachable_code)]
    Err("Uninstall is only supported on Windows.".to_string())
}

#[cfg(target_os = "windows")]
fn schedule_delete_after_exit(install_location: &str) -> Result<(), String> {
    let temp_dir = env::temp_dir();
    let script_path = temp_dir.join("sempal_uninstall.cmd");
    let script = format!(
        "@echo off\r\n\
        ping 127.0.0.1 -n 3 > nul\r\n\
        rmdir /s /q \"{install_location}\"\r\n\
        del \"%~f0\"\r\n"
    );
    fs::write(&script_path, script)
        .map_err(|err| format!("Failed to write uninstall script: {err}"))?;
    std::process::Command::new("cmd")
        .args(["/C", "start", "", script_path.to_string_lossy().as_ref()])
        .spawn()
        .map_err(|err| format!("Failed to launch uninstall script: {err}"))?;
    Ok(())
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

enum InstallerEvent {
    Started { total_files: usize },
    FileCopied { copied_files: usize, name: String },
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
}

impl InstallerApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut visuals = cc.egui_ctx.style().visuals.clone();
        style::apply_visuals(&mut visuals);
        cc.egui_ctx.set_visuals(visuals);

        Self {
            step: InstallStep::Welcome,
            install_dir: default_install_dir(),
            bundle_dir: default_bundle_dir(),
            license_text: include_str!("../../LICENSE").to_string(),
            progress: InstallProgress::default(),
            receiver: None,
            error: None,
            open_folder_on_finish: true,
            launch_on_finish: true,
        }
    }

    fn start_install(&mut self) {
        let bundle_dir = self.bundle_dir.clone();
        let install_dir = self.install_dir.clone();
        let (tx, rx) = mpsc::channel();
        self.receiver = Some(rx);
        self.progress = InstallProgress::default();
        self.step = InstallStep::Installing;
        thread::spawn(move || {
            if let Err(err) = run_install(&bundle_dir, &install_dir, tx.clone()) {
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
                InstallerEvent::Finished => {
                    self.step = InstallStep::Done;
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
                    ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.license_text)
                                .desired_rows(16)
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
                    ui.label(format!(
                        "Bundle source: {}",
                        self.bundle_dir.display()
                    ));
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
                    let progress =
                        (self.progress.copied_files as f32 / self.progress.total_files.max(1) as f32)
                            .clamp(0.0, 1.0);
                    ui.label("Installing...");
                    ui.add(egui::ProgressBar::new(progress).show_percentage());
                    if let Some(current) = &self.progress.current {
                        ui.label(format!("Copying {current}"));
                    }
                }
                InstallStep::Done => {
                    ui.label(RichText::new("Installation complete.").strong());
                    ui.checkbox(&mut self.open_folder_on_finish, "Open install folder");
                    ui.checkbox(&mut self.launch_on_finish, "Launch SemPal");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(Button::new("Finish")).clicked() {
                            if self.open_folder_on_finish {
                                let _ = open::that(&self.install_dir);
                            }
                            if self.launch_on_finish {
                                let exe = self.install_dir.join("sempal.exe");
                                let _ = std::process::Command::new(exe).spawn();
                            }
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                }
                InstallStep::Error => {
                    ui.label(RichText::new("Installation failed.").color(style::palette().warning));
                    if let Some(error) = &self.error {
                        ui.label(error);
                    }
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

fn default_install_dir() -> PathBuf {
    if let Ok(local_app_data) = env::var("LOCALAPPDATA") {
        return PathBuf::from(local_app_data)
            .join("Programs")
            .join(APP_NAME);
    }
    if let Ok(program_files) = env::var("ProgramFiles") {
        return PathBuf::from(program_files).join(APP_NAME);
    }
    PathBuf::from("C:\\Program Files").join(APP_NAME)
}

fn default_bundle_dir() -> PathBuf {
    env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("bundle")))
        .unwrap_or_else(|| PathBuf::from("bundle"))
}

fn run_install(bundle_dir: &Path, install_dir: &Path, sender: mpsc::Sender<InstallerEvent>) -> Result<(), String> {
    let entries = collect_bundle_entries(bundle_dir)?;
    sender
        .send(InstallerEvent::Started {
            total_files: entries.len(),
        })
        .map_err(|err| format!("Failed to report install start: {err}"))?;

    fs::create_dir_all(install_dir)
        .map_err(|err| format!("Failed to create install dir: {err}"))?;

    for (idx, (source, relative)) in entries.iter().enumerate() {
        let target = install_dir.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("Failed to create folder {}: {err}", parent.display()))?;
        }
        fs::copy(source, &target)
            .map_err(|err| format!("Failed to copy {}: {err}", source.display()))?;
        sender
            .send(InstallerEvent::FileCopied {
                copied_files: idx + 1,
                name: relative.display().to_string(),
            })
            .map_err(|err| format!("Failed to report install progress: {err}"))?;
    }

    ensure_app_data_models(bundle_dir)?;
    register_uninstall_entry(install_dir)?;

    sender
        .send(InstallerEvent::Finished)
        .map_err(|err| format!("Failed to report completion: {err}"))?;
    Ok(())
}

fn collect_bundle_entries(bundle_dir: &Path) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    if !bundle_dir.exists() {
        return Err(format!(
            "Bundle directory not found at {}",
            bundle_dir.display()
        ));
    }
    let mut files = Vec::new();
    visit_bundle(bundle_dir, bundle_dir, &mut files)?;
    Ok(files)
}

fn visit_bundle(root: &Path, dir: &Path, files: &mut Vec<(PathBuf, PathBuf)>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|err| format!("Failed to read bundle: {err}"))? {
        let entry = entry.map_err(|err| format!("Failed to read bundle entry: {err}"))?;
        let path = entry.path();
        if path.is_dir() {
            visit_bundle(root, &path, files)?;
        } else {
            let relative = path
                .strip_prefix(root)
                .map_err(|err| format!("Failed to build relative path: {err}"))?
                .to_path_buf();
            files.push((path, relative));
        }
    }
    Ok(())
}

fn ensure_app_data_models(bundle_dir: &Path) -> Result<(), String> {
    let app_root = app_dirs::app_root_dir().map_err(|err| err.to_string())?;
    let models_dir = app_root.join("models");
    fs::create_dir_all(&models_dir)
        .map_err(|err| format!("Failed to create models directory: {err}"))?;

    let bundle_models = bundle_dir.join("models");
    if bundle_models.exists() {
        for (source, relative) in collect_bundle_entries(&bundle_models)? {
            let target = models_dir.join(relative);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)
                    .map_err(|err| format!("Failed to create model folder: {err}"))?;
            }
            fs::copy(&source, &target)
                .map_err(|err| format!("Failed to copy model {}: {err}", source.display()))?;
        }
    }
    Ok(())
}

fn register_uninstall_entry(install_dir: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (key, _) = hkcu
            .create_subkey(UNINSTALL_KEY)
            .map_err(|err| format!("Failed to create uninstall registry key: {err}"))?;
        let exe_path = install_dir.join("sempal-installer.exe");
        let uninstall = format!("\"{}\" --uninstall", exe_path.display());
        key.set_value("DisplayName", &APP_NAME)
            .map_err(|err| format!("Failed to set DisplayName: {err}"))?;
        key.set_value("DisplayVersion", &env!("CARGO_PKG_VERSION"))
            .map_err(|err| format!("Failed to set DisplayVersion: {err}"))?;
        key.set_value("Publisher", &APP_PUBLISHER)
            .map_err(|err| format!("Failed to set Publisher: {err}"))?;
        key.set_value("InstallLocation", &install_dir.display().to_string())
            .map_err(|err| format!("Failed to set InstallLocation: {err}"))?;
        key.set_value("UninstallString", &uninstall)
            .map_err(|err| format!("Failed to set UninstallString: {err}"))?;
        key.set_value(
            "DisplayIcon",
            &install_dir.join("sempal.ico").display().to_string(),
        )
        .map_err(|err| format!("Failed to set DisplayIcon: {err}"))?;
        return Ok(());
    }
    #[allow(unreachable_code)]
    Err("Uninstall registry entry is only supported on Windows.".to_string())
}
