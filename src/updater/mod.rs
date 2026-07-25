//! Update-check and update-application helpers.
//!
//! This module is consumed both by the main app shell (to check for new releases)
//! and by the optional `wavecrate-updater` helper binary (to apply updates).

mod apply;
mod archive;
mod asset_names;
mod check;
mod fs_ops;
mod github;
mod path_guard;
mod public_catalog;
mod release_contract;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub(super) use asset_names::{
    expected_checksums_name, expected_checksums_signature_name, expected_zip_asset_name,
};
pub(super) use path_guard::ValidatedInstallRoot;

pub use apply::{ApplyPlan, StaleRemovalFailure, UpdateManifest};
pub use check::{UpdateCheckOutcome, UpdateCheckRequest};
pub use fs_ops::PostCommitCleanupFailure;
pub use github::ReleaseSummary;
pub use public_catalog::{
    PUBLIC_RELEASE_CATALOG_URL, PUBLIC_RELEASE_PAGE_URL, PublicReleaseCheckRequest,
    PublicReleaseInfo, check_public_release_catalog,
};
pub use release_contract::supported_release_target_for_platform_arch;

/// Canonical app name used by the release contract.
pub const APP_NAME: &str = "wavecrate";
/// Canonical GitHub repository slug (`OWNER/REPO`) used for update checks.
pub const REPO_SLUG: &str = "PORTALSURFER/wavecrate";
/// Base64-encoded Ed25519 public key used to verify checksum signatures.
pub(crate) const CHECKSUMS_PUBLIC_KEY_BASE64: &str = "8Z7dQJBRMbxCFkFMeBYa1FMSWOUm6nePFgoK5c43jT4=";

/// Update channel selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    /// Stable release channel.
    #[default]
    Stable,
    /// Release candidate channel.
    Rc,
    /// Nightly/pre-release channel.
    Nightly,
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
    /// Platform identifier for supported app builds (windows/macos).
    pub platform: String,
    /// Architecture identifier (x86_64/aarch64).
    pub arch: String,
}

/// Updater run configuration (used by `wavecrate-updater`).
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
