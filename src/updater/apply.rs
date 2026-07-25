//! Update apply transaction logic for staged patching and manifest validation.
//!
//! The functions in this module are intentionally deterministic: unpack and validate
//! an update package, stage file changes through an atomic transaction, and report
//! the final copy/replacement plan.

use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;

use super::{
    PostCommitCleanupFailure, UpdateChannel, UpdateError, UpdateProgress, UpdaterRunArgs,
    ValidatedInstallRoot, archive, expected_checksums_name, expected_checksums_signature_name,
    expected_zip_asset_name, fs_ops, github,
};

mod archive_root;
mod manifest_diff;
mod relaunch;
mod stale_removal;

use archive_root::validate_root_dir;
use manifest_diff::{collect_stale_files, load_installed_manifest};
use relaunch::relaunch_app;
use stale_removal::remove_stale_paths;

#[cfg(test)]
mod tests;

/// Parsed `update-manifest.json` embedded in release archives.
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateManifest {
    /// Application name.
    pub app: String,
    /// Channel label (stable/rc/nightly).
    pub channel: String,
    /// Target identifier.
    pub target: String,
    /// Platform identifier.
    pub platform: String,
    /// Architecture identifier.
    pub arch: String,
    /// List of files expected in the archive.
    pub files: Vec<String>,
}

impl UpdateManifest {
    /// Validate the manifest against a runtime identity.
    pub fn validate(&self, expected: &super::RuntimeIdentity) -> Result<(), UpdateError> {
        if self.app != expected.app {
            return Err(UpdateError::Invalid(format!(
                "Manifest app mismatch: expected {}, got {}",
                expected.app, self.app
            )));
        }
        if !manifest_channel_allowed(&self.channel, expected.channel) {
            return Err(UpdateError::Invalid(format!(
                "Manifest channel mismatch: expected {}, got {}",
                channel_label(expected.channel),
                self.channel
            )));
        }
        if self.target != expected.target {
            return Err(UpdateError::Invalid(format!(
                "Manifest target mismatch: expected {}, got {}",
                expected.target, self.target
            )));
        }
        if self.platform != expected.platform {
            return Err(UpdateError::Invalid(format!(
                "Manifest platform mismatch: expected {}, got {}",
                expected.platform, self.platform
            )));
        }
        if self.arch != expected.arch {
            return Err(UpdateError::Invalid(format!(
                "Manifest arch mismatch: expected {}, got {}",
                expected.arch, self.arch
            )));
        }
        if self.files.is_empty() {
            return Err(UpdateError::Invalid("Manifest files list is empty".into()));
        }
        Ok(())
    }
}

/// Plan describing the changes applied by an update.
#[derive(Debug, Clone)]
pub struct ApplyPlan {
    /// Release tag that was applied.
    pub release_tag: String,
    /// Installation directory used for the update.
    pub install_dir: PathBuf,
    /// Whether the app should relaunch afterward.
    pub relaunch: bool,
    /// Files copied during the update.
    pub copied_files: Vec<String>,
    /// Directories replaced during the update.
    pub replaced_dirs: Vec<String>,
    /// Disposable `.old` or `.new` paths that remained after the update committed.
    pub post_commit_cleanup_failures: Vec<PostCommitCleanupFailure>,
    /// Stale paths that could not be removed after applying the update.
    pub stale_removal_failures: Vec<StaleRemovalFailure>,
}

/// Details about a stale path removal failure during update cleanup.
#[derive(Debug, Clone)]
pub struct StaleRemovalFailure {
    /// Path that failed to be removed.
    pub path: PathBuf,
    /// Human-readable error describing the failure.
    pub error: String,
}

#[derive(Debug)]
struct AppliedFilesPlan {
    copied_files: Vec<String>,
    replaced_dirs: Vec<String>,
    post_commit_cleanup_failures: Vec<PostCommitCleanupFailure>,
    stale_removal_failures: Vec<StaleRemovalFailure>,
}

/// Return type for `apply_files_and_dirs`.
type ApplyFilesPlanResult = Result<AppliedFilesPlan, UpdateError>;

/// Apply the selected update payload into the installation directory.
/// Returns an [`ApplyPlan`] describing copied files, replaced directories, and stale
/// path cleanup failures.
pub(super) fn apply_update_with_progress<F>(
    args: UpdaterRunArgs,
    mut progress: F,
) -> Result<ApplyPlan, UpdateError>
where
    F: FnMut(UpdateProgress),
{
    let release = match args.requested_tag.as_deref() {
        Some(tag) => {
            report(&mut progress, format!("Fetching release {tag}..."));
            github::fetch_release_by_tag_with_assets(
                &args.repo,
                tag,
                args.identity.channel,
                &args.identity,
            )?
        }
        None => {
            report(&mut progress, "Fetching latest release...");
            github::fetch_release_with_assets(&args.repo, args.identity.channel, &args.identity)?
        }
    };
    let version = match args.identity.channel {
        UpdateChannel::Stable => Some(
            release
                .tag_name
                .strip_prefix('v')
                .ok_or_else(|| UpdateError::Invalid(format!("Invalid tag {}", release.tag_name)))?
                .to_string(),
        ),
        UpdateChannel::Rc => Some(
            release
                .tag_name
                .strip_prefix('v')
                .ok_or_else(|| UpdateError::Invalid(format!("Invalid tag {}", release.tag_name)))?
                .to_string(),
        ),
        UpdateChannel::Nightly => None,
    };

    let zip_name = expected_zip_asset_name(&args.identity, version.as_deref())?;
    let checksums_name = expected_checksums_name(&args.identity, version.as_deref())?;
    let checksums_sig_name = expected_checksums_signature_name(&args.identity, version.as_deref())?;

    let tmp = tempfile::tempdir()?;
    let zip_path = tmp.path().join(&zip_name);
    report(&mut progress, format!("Downloading {checksums_name}..."));
    let checksums_bytes = archive::download_release_asset_bytes(&release, &checksums_name)?;
    report(
        &mut progress,
        format!("Downloading {checksums_sig_name}..."),
    );
    let signature_bytes = archive::download_release_asset_bytes(&release, &checksums_sig_name)?;
    report(&mut progress, "Verifying checksums signature...");
    archive::verify_checksums_signature(&checksums_bytes, &signature_bytes)?;
    let expected = archive::parse_checksums_for_asset(&checksums_bytes, &zip_name)?;
    report(&mut progress, format!("Downloading {zip_name}..."));
    archive::download_release_asset(&release, &zip_name, &zip_path)?;
    report(&mut progress, "Verifying checksum...");
    archive::verify_zip_checksum(&zip_path, &expected)?;

    let unpack_dir = tmp.path().join("unpacked");
    fs_ops::ensure_empty_dir(&unpack_dir)?;
    report(&mut progress, "Unpacking update...");
    archive::unzip_to_dir(&zip_path, &unpack_dir)?;

    report(&mut progress, "Validating update manifest...");
    let root_dir = validate_root_dir(&unpack_dir, &args.identity.app)?;
    let manifest_path = root_dir.join("update-manifest.json");
    let manifest_bytes = fs::read(&manifest_path)?;
    let manifest: UpdateManifest = serde_json::from_slice(&manifest_bytes)?;
    manifest.validate(&args.identity)?;
    for file in manifest.files.iter() {
        if !root_dir.join(file).exists() {
            return Err(UpdateError::Invalid(format!(
                "Manifest file missing after unzip: {file}"
            )));
        }
    }

    report(&mut progress, "Applying update transaction...");
    let applied = apply_files_and_dirs(&args.install_dir, &root_dir, &manifest)?;
    finish_applied_update(
        &args,
        release.tag_name,
        &manifest,
        applied,
        &mut progress,
        relaunch_app,
    )
}

fn finish_applied_update(
    args: &UpdaterRunArgs,
    release_tag: String,
    manifest: &UpdateManifest,
    applied: AppliedFilesPlan,
    progress: &mut impl FnMut(UpdateProgress),
    relaunch: impl FnOnce(&Path, &str, &UpdateManifest) -> Result<(), UpdateError>,
) -> Result<ApplyPlan, UpdateError> {
    for failure in &applied.post_commit_cleanup_failures {
        report(
            progress,
            format!(
                "Warning: update committed but cleanup left {}: {}",
                failure.path.display(),
                failure.error
            ),
        );
    }
    for failure in &applied.stale_removal_failures {
        report(
            progress,
            format!(
                "Warning: failed to remove stale path {}: {}",
                failure.path.display(),
                failure.error
            ),
        );
    }

    if args.relaunch {
        report(progress, "Relaunching app...");
        if let Err(err) = relaunch(&args.install_dir, &args.identity.app, manifest) {
            report(progress, format!("Relaunch failed: {err}"));
            return Err(err);
        }
    }

    Ok(ApplyPlan {
        release_tag,
        install_dir: args.install_dir.clone(),
        relaunch: args.relaunch,
        copied_files: applied.copied_files,
        replaced_dirs: applied.replaced_dirs,
        post_commit_cleanup_failures: applied.post_commit_cleanup_failures,
        stale_removal_failures: applied.stale_removal_failures,
    })
}

fn report(progress: &mut impl FnMut(UpdateProgress), message: impl Into<String>) {
    progress(UpdateProgress::new(message));
}

fn apply_files_and_dirs(
    install_dir: &Path,
    root_dir: &Path,
    manifest: &UpdateManifest,
) -> ApplyFilesPlanResult {
    apply_files_and_dirs_with_commit(install_dir, root_dir, manifest, |transaction| {
        transaction.commit()
    })
}

fn apply_files_and_dirs_with_commit(
    install_dir: &Path,
    root_dir: &Path,
    manifest: &UpdateManifest,
    commit: impl FnOnce(
        fs_ops::UpdateTransaction,
    ) -> Result<fs_ops::TransactionCommitOutcome, UpdateError>,
) -> ApplyFilesPlanResult {
    let install_root = ValidatedInstallRoot::new(install_dir)?;
    let installed_manifest = load_installed_manifest(install_root.path())?;
    let mut stale_files = match installed_manifest.as_ref() {
        Some(installed) => collect_stale_files(&install_root, installed, manifest)?,
        None => Vec::new(),
    };
    if install_root.path().join("resources").exists() && !root_dir.join("resources").is_dir() {
        stale_files.push(install_root.child_path("resources")?);
    }
    let mut transaction = fs_ops::UpdateTransaction::new(install_root.clone());
    let mut copied = Vec::new();
    for file in manifest.files.iter() {
        let src = root_dir.join(file);
        transaction.stage_file(&src, file)?;
        copied.push(file.clone());
    }

    let mut replaced_dirs = Vec::new();
    let resources_src = root_dir.join("resources");
    if resources_src.is_dir() {
        transaction.stage_dir(&resources_src, "resources")?;
        replaced_dirs.push("resources".to_string());
    }

    let post_commit_cleanup_failures = match commit(transaction)? {
        fs_ops::TransactionCommitOutcome::Committed => Vec::new(),
        fs_ops::TransactionCommitOutcome::CommittedWithCleanupFailures(failures) => failures,
    };

    let stale_removal_failures = remove_stale_paths(&stale_files, &install_root)?;

    Ok(AppliedFilesPlan {
        copied_files: copied,
        replaced_dirs,
        post_commit_cleanup_failures,
        stale_removal_failures,
    })
}

fn channel_label(channel: UpdateChannel) -> &'static str {
    match channel {
        UpdateChannel::Stable => "stable",
        UpdateChannel::Rc => "rc",
        UpdateChannel::Nightly => "nightly",
    }
}

fn manifest_channel_allowed(manifest_channel: &str, expected_channel: UpdateChannel) -> bool {
    match expected_channel {
        UpdateChannel::Rc => manifest_channel == "rc" || manifest_channel == "stable",
        channel => manifest_channel == channel_label(channel),
    }
}
