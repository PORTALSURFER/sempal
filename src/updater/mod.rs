//! Update-check and update-application helpers.
//!
//! This module is consumed both by the main egui app (to check for new releases)
//! and by the optional `sempal-updater` helper binary (to apply updates).

mod apply;
mod archive;
mod check;
mod fs_ops;
mod github;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use apply::{ApplyPlan, UpdateManifest};
pub use check::{UpdateCheckOutcome, UpdateCheckRequest};

/// Canonical app name used by the release contract.
pub const APP_NAME: &str = "sempal";
/// Canonical GitHub repository slug (`OWNER/REPO`) used for update checks.
pub const REPO_SLUG: &str = "PORTALSURFER/sempal";

/// Update channel selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    Stable,
    Nightly,
}

impl Default for UpdateChannel {
    fn default() -> Self {
        Self::Stable
    }
}

/// Context for the running app used to validate manifests.
#[derive(Debug, Clone)]
pub struct RuntimeIdentity {
    pub app: String,
    pub channel: UpdateChannel,
    pub target: String,
    pub platform: String,
    pub arch: String,
}

/// Updater run configuration (used by `sempal-updater`).
#[derive(Debug, Clone)]
pub struct UpdaterRunArgs {
    pub repo: String,
    pub identity: RuntimeIdentity,
    pub install_dir: PathBuf,
    pub relaunch: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Zip error: {0}")]
    Zip(String),
    #[error("Checksum mismatch for {filename}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        filename: String,
        expected: String,
        actual: String,
    },
    #[error("Invalid update: {0}")]
    Invalid(String),
}

/// Apply an update for `args.identity` into `args.install_dir`.
pub fn apply_update(args: UpdaterRunArgs) -> Result<ApplyPlan, UpdateError> {
    apply::apply_update(args)
}

/// Check GitHub releases and report whether an update is available.
pub fn check_for_updates(request: UpdateCheckRequest) -> Result<UpdateCheckOutcome, UpdateError> {
    check::check_for_updates(request)
}

/// Best-effort open the release page.
pub fn open_release_page(url: &str) -> Result<(), String> {
    open::that(url).map_err(|err| err.to_string())
}

fn expected_zip_asset_name(
    identity: &RuntimeIdentity,
    version: Option<&str>,
) -> Result<String, UpdateError> {
    if identity.platform != "windows" || identity.arch != "x86_64" {
        return Err(UpdateError::Invalid(format!(
            "Unsupported platform/arch {}/{}",
            identity.platform, identity.arch
        )));
    }
    let name = match identity.channel {
        UpdateChannel::Stable => {
            let version =
                version.ok_or_else(|| UpdateError::Invalid("Missing stable version".into()))?;
            format!("{APP_NAME}-v{version}-windows-x86_64.zip")
        }
        UpdateChannel::Nightly => format!("{APP_NAME}-nightly-windows-x86_64.zip"),
    };
    Ok(name)
}

fn expected_checksums_name(
    identity: &RuntimeIdentity,
    version: Option<&str>,
) -> Result<String, UpdateError> {
    let name = match identity.channel {
        UpdateChannel::Stable => {
            let version =
                version.ok_or_else(|| UpdateError::Invalid("Missing stable version".into()))?;
            format!("checksums-v{version}.txt")
        }
        UpdateChannel::Nightly => "checksums-nightly.txt".to_string(),
    };
    Ok(name)
}

fn ensure_child_path(dir: &Path, name: &str) -> Result<PathBuf, UpdateError> {
    let candidate = dir.join(name);
    let dir = dir
        .canonicalize()
        .map_err(|err| UpdateError::Invalid(format!("Invalid install dir: {err}")))?;
    let candidate = candidate.canonicalize().unwrap_or(candidate);
    if !candidate.starts_with(&dir) {
        return Err(UpdateError::Invalid(format!(
            "Refusing to write outside install dir: {}",
            candidate.display()
        )));
    }
    Ok(candidate)
}
