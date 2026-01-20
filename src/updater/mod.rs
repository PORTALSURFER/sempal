//! Update-check and update-application helpers.
//!
//! This module is consumed both by the main egui app (to check for new releases)
//! and by the optional `sempal-updater` helper binary (to apply updates).

mod apply;
mod archive;
mod check;
mod fs_ops;
mod github;

use std::{
    fs,
    io::ErrorKind,
    path::{Component, Path, PathBuf},
};

#[cfg(test)]
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

pub use apply::{ApplyPlan, StaleRemovalFailure, UpdateManifest};
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
    /// Stable release channel.
    Stable,
    /// Nightly/pre-release channel.
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
    /// Application name.
    pub app: String,
    /// Update channel.
    pub channel: UpdateChannel,
    /// Target triple identifier.
    pub target: String,
    /// Platform identifier (windows/linux/macos).
    pub platform: String,
    /// Architecture identifier (x86_64/aarch64).
    pub arch: String,
}

/// Updater run configuration (used by `sempal-updater`).
#[derive(Debug, Clone)]
pub struct UpdaterRunArgs {
    /// GitHub repository slug.
    pub repo: String,
    /// Runtime identity for the update.
    pub identity: RuntimeIdentity,
    /// Installation directory for the update.
    pub install_dir: PathBuf,
    /// Whether to relaunch after update.
    pub relaunch: bool,
    /// Optional release tag override (e.g. `v0.384.0` or `nightly`).
    pub requested_tag: Option<String>,
}

/// Progress update emitted during apply steps.
#[derive(Debug, Clone)]
pub struct UpdateProgress {
    /// Human-readable progress message.
    pub message: String,
}

impl UpdateProgress {
    /// Create a new progress message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Errors returned by update checks or apply steps.
#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    /// HTTP/transport error.
    #[error("HTTP error: {0}")]
    Http(String),
    /// IO error during update.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// JSON parse/serialize error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    /// Archive/zip processing error.
    #[error("Zip error: {0}")]
    Zip(String),
    /// Downloaded checksum did not match.
    #[error("Checksum mismatch for {filename}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        /// File name that failed verification.
        filename: String,
        /// Expected checksum value.
        expected: String,
        /// Actual checksum value.
        actual: String,
    },
    /// Update was invalid or incompatible.
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
    ensure_no_symlink_path(&candidate)?;
    Ok(candidate)
}

fn ensure_no_symlink_path(path: &Path) -> Result<(), UpdateError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        #[cfg(windows)]
        if matches!(component, Component::Prefix(_) | Component::RootDir) {
            continue;
        }
        match symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(UpdateError::Invalid(format!(
                        "Refusing to write through symlink: {}",
                        current.display()
                    )));
                }
            }
            Err(err) if err.kind() == ErrorKind::NotFound => break,
            Err(err) => {
                // Fail closed unless a test/dev override is explicitly enabled.
                if allow_symlink_validation_errors() {
                    return Ok(());
                }
                return Err(UpdateError::Invalid(format!(
                    "Failed to validate update path for symlinks at {}: {err}",
                    current.display()
                )));
            }
        }
    }
    Ok(())
}

fn allow_symlink_validation_errors() -> bool {
    std::env::var("SEMPAL_UPDATER_ALLOW_SYMLINK_ERRORS")
        .ok()
        .map(|value| {
            let value = value.trim().to_ascii_lowercase();
            matches!(value.as_str(), "1" | "true" | "yes")
        })
        .unwrap_or(false)
}

#[cfg(test)]
fn symlink_metadata(path: &Path) -> std::io::Result<fs::Metadata> {
    if let Some(hook) = SYMLINK_METADATA_HOOK.get() {
        if let Ok(guard) = hook.lock() {
            if let Some(hook) = *guard {
                return hook(path);
            }
        }
    }
    fs::symlink_metadata(path)
}

#[cfg(not(test))]
fn symlink_metadata(path: &Path) -> std::io::Result<fs::Metadata> {
    fs::symlink_metadata(path)
}

#[cfg(test)]
static SYMLINK_METADATA_HOOK: OnceLock<Mutex<Option<fn(&Path) -> std::io::Result<fs::Metadata>>>> =
    OnceLock::new();

#[cfg(test)]
struct SymlinkMetadataHookGuard {
    prev: Option<fn(&Path) -> std::io::Result<fs::Metadata>>,
}

#[cfg(test)]
impl SymlinkMetadataHookGuard {
    fn new(hook: Option<fn(&Path) -> std::io::Result<fs::Metadata>>) -> Self {
        let cell = SYMLINK_METADATA_HOOK.get_or_init(|| Mutex::new(None));
        let mut guard = cell.lock().expect("symlink metadata hook lock");
        let prev = std::mem::replace(&mut *guard, hook);
        Self { prev }
    }
}

#[cfg(test)]
impl Drop for SymlinkMetadataHookGuard {
    fn drop(&mut self) {
        if let Some(cell) = SYMLINK_METADATA_HOOK.get() {
            let mut guard = cell.lock().expect("symlink metadata hook lock");
            *guard = self.prev;
        }
    }
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
    use std::io;
    use tempfile::tempdir;

    #[cfg(windows)]
    struct EnvVarGuard {
        key: String,
        previous: Option<String>,
    }

    #[cfg(windows)]
    impl EnvVarGuard {
        fn set(key: &str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            unsafe {
                std::env::set_var(key, value);
            }
            Self {
                key: key.to_string(),
                previous,
            }
        }
    }

    #[cfg(windows)]
    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(value) = self.previous.take() {
                unsafe {
                    std::env::set_var(&self.key, value);
                }
            } else {
                unsafe {
                    std::env::remove_var(&self.key);
                }
            }
        }
    }

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
        #[cfg(windows)]
        let _guard = EnvVarGuard::set("SEMPAL_UPDATER_ALLOW_SYMLINK_ERRORS", "1");
        let dir = tempdir().unwrap();
        let path = ensure_child_path(dir.path(), "./ok/file.txt").unwrap();
        let canonical = dir.path().canonicalize().unwrap();
        assert!(path.starts_with(&canonical));
        assert!(path.ends_with(Path::new("ok").join("file.txt")));
    }

    #[cfg(unix)]
    #[test]
    fn ensure_child_path_rejects_symlinked_component() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let install = dir.path().join("install");
        let external = dir.path().join("external");
        fs::create_dir_all(&install).unwrap();
        fs::create_dir_all(&external).unwrap();
        let link = install.join("link");
        symlink(&external, &link).unwrap();

        let err = ensure_child_path(&install, "link/file.txt").unwrap_err();
        assert!(err.to_string().contains("symlink"));
    }

    #[test]
    fn ensure_child_path_fails_on_symlink_metadata_error() {
        fn fail_metadata(_: &Path) -> io::Result<fs::Metadata> {
            Err(io::Error::new(
                ErrorKind::PermissionDenied,
                "metadata denied",
            ))
        }

        let _guard = SymlinkMetadataHookGuard::new(Some(fail_metadata));
        let dir = tempdir().unwrap();
        let err = ensure_child_path(dir.path(), "ok/file.txt").unwrap_err();
        assert!(err
            .to_string()
            .contains("Failed to validate update path for symlinks"));
    }
}
