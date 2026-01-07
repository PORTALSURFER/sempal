//! Update-check and update-application helpers.
//!
//! This module is consumed both by the main egui app (to check for new releases)
//! and by the optional `sempal-updater` helper binary (to apply updates).

mod apply;
mod archive;
mod check;
mod fs_ops;
mod github;

use std::path::Component;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use apply::{ApplyPlan, UpdateManifest};
pub use check::{UpdateCheckOutcome, UpdateCheckRequest};
pub use github::ReleaseSummary;

/// Canonical app name used by the release contract.
pub const APP_NAME: &str = "sempal";
/// Canonical GitHub repository slug (`OWNER/REPO`) used for update checks.
pub const REPO_SLUG: &str = "PORTALSURFER/sempal";
/// Base64-encoded Ed25519 public key used to verify checksum signatures.
pub(crate) const CHECKSUMS_PUBLIC_KEY_BASE64: &str = "kicipwnHITr+xoX96bXvp85X2el7+2JyVsYldhtRWDY=";

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
    /// Optional release tag override (e.g. `v0.384.0` or `nightly`).
    pub requested_tag: Option<String>,
}

/// Progress update emitted during apply steps.
#[derive(Debug, Clone)]
pub struct UpdateProgress {
    pub message: String,
}

impl UpdateProgress {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
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
    apply::apply_update_with_progress(args, |_| {})
}

/// Apply an update while reporting progress.
pub fn apply_update_with_progress<F>(
    args: UpdaterRunArgs,
    progress: F,
) -> Result<ApplyPlan, UpdateError>
where
    F: FnMut(UpdateProgress),
{
    apply::apply_update_with_progress(args, progress)
}

/// Check GitHub releases and report whether an update is available.
pub fn check_for_updates(request: UpdateCheckRequest) -> Result<UpdateCheckOutcome, UpdateError> {
    check::check_for_updates(request)
}

/// List recent releases that match the runtime identity and channel.
pub fn list_recent_releases(
    repo: &str,
    channel: UpdateChannel,
    identity: &RuntimeIdentity,
    limit: usize,
) -> Result<Vec<ReleaseSummary>, UpdateError> {
    github::list_releases_with_assets(repo, channel, identity, limit)
}

/// Best-effort open the release page.
pub fn open_release_page(url: &str) -> Result<(), String> {
    open::that(url).map_err(|err| err.to_string())
}

fn expected_zip_asset_name(
    identity: &RuntimeIdentity,
    version: Option<&str>,
) -> Result<String, UpdateError> {
    let platform = match identity.platform.as_str() {
        "windows" | "linux" | "macos" => identity.platform.as_str(),
        _ => {
            return Err(UpdateError::Invalid(format!(
                "Unsupported platform/arch {}/{}",
                identity.platform, identity.arch
            )));
        }
    };
    let arch = match identity.arch.as_str() {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        _ => {
            return Err(UpdateError::Invalid(format!(
                "Unsupported platform/arch {}/{}",
                identity.platform, identity.arch
            )));
        }
    };
    let name = match identity.channel {
        UpdateChannel::Stable => {
            let version =
                version.ok_or_else(|| UpdateError::Invalid("Missing stable version".into()))?;
            format!("{APP_NAME}-v{version}-{platform}-{arch}.zip")
        }
        UpdateChannel::Nightly => format!("{APP_NAME}-nightly-{platform}-{arch}.zip"),
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

fn expected_checksums_signature_name(
    identity: &RuntimeIdentity,
    version: Option<&str>,
) -> Result<String, UpdateError> {
    let name = match identity.channel {
        UpdateChannel::Stable => {
            let version =
                version.ok_or_else(|| UpdateError::Invalid("Missing stable version".into()))?;
            format!("checksums-v{version}.txt.sig")
        }
        UpdateChannel::Nightly => "checksums-nightly.txt.sig".to_string(),
    };
    Ok(name)
}

fn ensure_child_path(dir: &Path, name: &str) -> Result<PathBuf, UpdateError> {
    let relative = sanitize_relative_path(name)?;
    let dir = dir
        .canonicalize()
        .map_err(|err| UpdateError::Invalid(format!("Invalid install dir: {err}")))?;
    let candidate = dir.join(relative);
    if !candidate.starts_with(&dir) {
        return Err(UpdateError::Invalid(format!(
            "Refusing to write outside install dir: {}",
            candidate.display()
        )));
    }
    Ok(candidate)
}

fn sanitize_relative_path(name: &str) -> Result<PathBuf, UpdateError> {
    let mut sanitized = PathBuf::new();
    let mut saw_component = false;
    for component in Path::new(name).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => {
                sanitized.push(part);
                saw_component = true;
            }
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(UpdateError::Invalid(format!("Invalid update path: {name}")));
            }
        }
    }
    if !saw_component {
        return Err(UpdateError::Invalid(format!("Invalid update path: {name}")));
    }
    Ok(sanitized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn ensure_child_path_rejects_parent_dir() {
        let dir = tempdir().unwrap();
        let err = ensure_child_path(dir.path(), "../evil.txt").unwrap_err();
        assert!(err.to_string().contains("Invalid update path"));
    }

    #[test]
    fn ensure_child_path_rejects_absolute_path() {
        let dir = tempdir().unwrap();
        #[cfg(windows)]
        let name = "C:\\evil.txt";
        #[cfg(not(windows))]
        let name = "/tmp/evil.txt";
        let err = ensure_child_path(dir.path(), name).unwrap_err();
        assert!(err.to_string().contains("Invalid update path"));
    }

    #[test]
    fn ensure_child_path_allows_relative_path() {
        let dir = tempdir().unwrap();
        let path = ensure_child_path(dir.path(), "./ok/file.txt").unwrap();
        let canonical = dir.path().canonicalize().unwrap();
        assert!(path.starts_with(&canonical));
        assert!(path.ends_with(Path::new("ok").join("file.txt")));
    }
}
