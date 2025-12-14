use super::*;

use crate::updater::{UpdateChannel, UpdateCheckOutcome, UpdateCheckRequest, check_for_updates};

impl EguiController {
    pub(super) fn maybe_check_for_updates_on_startup(&mut self) {
        if !self.settings.updates.check_on_startup {
            return;
        }
        if self.ui.update.status != crate::egui_app::state::UpdateStatus::Idle {
            return;
        }
        self.begin_update_check();
    }

    /// Start an update check regardless of user settings.
    pub fn check_for_updates_now(&mut self) {
        self.begin_update_check();
    }

    /// Open the currently available update URL in the OS browser (if present).
    pub fn open_update_link(&mut self) {
        let Some(url) = self.ui.update.available_url.clone() else {
            return;
        };
        if let Err(err) = crate::updater::open_release_page(&url) {
            self.set_status(format!("Could not open update link: {err}"), StatusTone::Error);
        }
    }

    /// Dismiss the current update notification (nightly dismisses are persisted).
    pub fn dismiss_update_notification(&mut self) {
        if let Some(published) = self.ui.update.available_published_at.clone() {
            self.ui.update.last_seen_nightly_published_at = Some(published.clone());
            self.settings.updates.last_seen_nightly_published_at = Some(published);
        }
        self.ui.update.status = crate::egui_app::state::UpdateStatus::Idle;
        self.ui.update.available_tag = None;
        self.ui.update.available_url = None;
        self.ui.update.available_published_at = None;
        let _ = self.save_full_config();
    }

    /// On Windows, launch `sempal-updater.exe` and terminate the app so the helper can swap binaries.
    pub fn install_update_and_exit(&mut self) {
        #[cfg(not(target_os = "windows"))]
        {
            self.open_update_link();
            return;
        }
        #[cfg(target_os = "windows")]
        {
            let Some(url) = self.ui.update.available_url.clone() else {
                self.set_status("No update available", StatusTone::Info);
                return;
            };
            let install_dir = match std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            {
                Some(dir) => dir,
                None => {
                    self.set_status("Could not resolve install directory", StatusTone::Error);
                    return;
                }
            };
            let updater_path = install_dir.join("sempal-updater.exe");
            if !updater_path.exists() {
                self.set_status(
                    "Updater helper missing (sempal-updater.exe); opening release page",
                    StatusTone::Warning,
                );
                let _ = crate::updater::open_release_page(&url);
                return;
            }
            let channel = map_channel(self.settings.updates.channel);
            let mut cmd = std::process::Command::new(&updater_path);
            cmd.arg("--install-dir")
                .arg(&install_dir)
                .arg("--repo")
                .arg(crate::updater::REPO_SLUG)
                .arg("--channel")
                .arg(match channel {
                    UpdateChannel::Stable => "stable",
                    UpdateChannel::Nightly => "nightly",
                })
                .arg("--target")
                .arg("x86_64-pc-windows-msvc")
                .arg("--platform")
                .arg("windows")
                .arg("--arch")
                .arg("x86_64");
            let spawned = cmd.spawn();
            match spawned {
                Ok(_) => std::process::exit(0),
                Err(err) => self.set_status(
                    format!("Could not launch updater: {err}"),
                    StatusTone::Error,
                ),
            }
        }
    }

    pub(in crate::egui_app::controller) fn apply_update_check_result(
        &mut self,
        result: UpdateCheckOutcome,
    ) {
        match result {
            UpdateCheckOutcome::UpToDate => {
                self.ui.update.status = crate::egui_app::state::UpdateStatus::Idle;
                self.ui.update.available_tag = None;
                self.ui.update.available_url = None;
                self.ui.update.available_published_at = None;
            }
            UpdateCheckOutcome::UpdateAvailable {
                tag,
                html_url,
                published_at,
            } => {
                self.ui.update.status = crate::egui_app::state::UpdateStatus::UpdateAvailable;
                self.ui.update.available_tag = Some(tag);
                self.ui.update.available_url = Some(html_url);
                self.ui.update.available_published_at = published_at;
            }
        }
    }

    pub(in crate::egui_app::controller) fn apply_update_check_error(&mut self, err: String) {
        self.ui.update.status = crate::egui_app::state::UpdateStatus::Error;
        self.ui.update.last_error = Some(err.clone());
        self.ui.update.available_tag = None;
        self.ui.update.available_url = None;
        self.ui.update.available_published_at = None;
        self.set_status(format!("Update check failed: {err}"), StatusTone::Warning);
    }

    fn begin_update_check(&mut self) {
        if self.runtime.jobs.update_check_in_progress() {
            return;
        }
        let current_version = match semver::Version::parse(env!("CARGO_PKG_VERSION")) {
            Ok(v) => v,
            Err(_) => semver::Version::new(0, 0, 0),
        };
        let request = UpdateCheckRequest {
            repo: crate::updater::REPO_SLUG.to_string(),
            channel: map_channel(self.settings.updates.channel),
            current_version,
            last_seen_nightly_published_at: self.ui.update.last_seen_nightly_published_at.clone(),
        };
        self.ui.update.status = crate::egui_app::state::UpdateStatus::Checking;
        self.ui.update.last_error = None;
        self.runtime.jobs.begin_update_check(request);
    }
}

fn map_channel(channel: crate::sample_sources::config::UpdateChannel) -> UpdateChannel {
    match channel {
        crate::sample_sources::config::UpdateChannel::Stable => UpdateChannel::Stable,
        crate::sample_sources::config::UpdateChannel::Nightly => UpdateChannel::Nightly,
    }
}

pub(in crate::egui_app::controller) fn run_update_check(
    request: UpdateCheckRequest,
) -> Result<UpdateCheckOutcome, String> {
    check_for_updates(request).map_err(|err| err.to_string())
}
