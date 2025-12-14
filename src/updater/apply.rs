use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use serde::Deserialize;

use super::{
    UpdateChannel, UpdateError, UpdaterRunArgs, archive, ensure_child_path,
    expected_checksums_name, expected_zip_asset_name, fs_ops, github,
};

/// Parsed `update-manifest.json` embedded in release archives.
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateManifest {
    pub app: String,
    pub channel: String,
    pub target: String,
    pub platform: String,
    pub arch: String,
    pub files: Vec<String>,
}

impl UpdateManifest {
    pub fn validate(&self, expected: &super::RuntimeIdentity) -> Result<(), UpdateError> {
        if self.app != expected.app {
            return Err(UpdateError::Invalid(format!(
                "Manifest app mismatch: expected {}, got {}",
                expected.app, self.app
            )));
        }
        if self.channel != channel_label(expected.channel) {
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

#[derive(Debug, Clone)]
pub struct ApplyPlan {
    pub release_tag: String,
    pub install_dir: PathBuf,
    pub relaunch: bool,
    pub copied_files: Vec<String>,
    pub replaced_dirs: Vec<String>,
}

pub(super) fn apply_update(args: UpdaterRunArgs) -> Result<ApplyPlan, UpdateError> {
    let release = github::fetch_release(&args.repo, args.identity.channel)?;
    let version = match args.identity.channel {
        UpdateChannel::Stable => Some(
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

    let tmp = tempfile::tempdir()?;
    let zip_path = tmp.path().join(&zip_name);
    let checksums_bytes = archive::download_release_asset_bytes(&release, &checksums_name)?;
    let expected = archive::parse_checksums_for_asset(&checksums_bytes, &zip_name)?;
    archive::download_release_asset(&release, &zip_name, &zip_path)?;
    archive::verify_zip_checksum(&zip_path, &expected)?;

    let unpack_dir = tmp.path().join("unpacked");
    fs_ops::ensure_empty_dir(&unpack_dir)?;
    archive::unzip_to_dir(&zip_path, &unpack_dir)?;

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

    let (copied_files, replaced_dirs) =
        apply_files_and_dirs(&args.install_dir, &root_dir, &manifest)?;

    if args.relaunch {
        relaunch_app(&args.install_dir, &args.identity.app, &manifest)?;
    }

    Ok(ApplyPlan {
        release_tag: release.tag_name,
        install_dir: args.install_dir,
        relaunch: args.relaunch,
        copied_files,
        replaced_dirs,
    })
}

fn validate_root_dir(unpack_dir: &Path, expected: &str) -> Result<PathBuf, UpdateError> {
    let entries = fs_ops::list_root_entries(unpack_dir)?;
    let mut dirs = entries
        .into_iter()
        .filter(|p| p.is_dir())
        .collect::<Vec<_>>();
    if dirs.len() != 1 {
        return Err(UpdateError::Invalid(
            "Archive must contain exactly one root directory".into(),
        ));
    }
    let root = dirs.pop().unwrap();
    let Some(name) = root.file_name().and_then(|s| s.to_str()) else {
        return Err(UpdateError::Invalid(
            "Invalid archive root directory".into(),
        ));
    };
    if name != expected {
        return Err(UpdateError::Invalid(format!(
            "Archive root directory must be '{expected}/', got '{name}/'"
        )));
    }
    Ok(root)
}

fn apply_files_and_dirs(
    install_dir: &Path,
    root_dir: &Path,
    manifest: &UpdateManifest,
) -> Result<(Vec<String>, Vec<String>), UpdateError> {
    let running_name = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|s| s.to_owned()));

    let mut copied = Vec::new();
    for file in manifest.files.iter() {
        let src = root_dir.join(file);
        let dest = ensure_child_path(install_dir, file)?;
        if running_name.as_deref() == dest.file_name() {
            continue;
        }
        fs_ops::copy_file_atomic(&src, &dest)?;
        copied.push(file.clone());
    }

    let mut replaced_dirs = Vec::new();
    let resources_src = root_dir.join("resources");
    if resources_src.is_dir() {
        let resources_dest = install_dir.join("resources");
        fs_ops::replace_dir(&resources_src, &resources_dest)?;
        replaced_dirs.push("resources".to_string());
    }

    Ok((copied, replaced_dirs))
}

fn relaunch_app(
    install_dir: &Path,
    app: &str,
    manifest: &UpdateManifest,
) -> Result<(), UpdateError> {
    let candidate = app_executable_name(app, manifest);
    let exe = install_dir.join(&candidate);
    if !exe.exists() {
        return Err(UpdateError::Invalid(format!(
            "Updated executable missing: {}",
            exe.display()
        )));
    }
    let mut cmd = Command::new(exe);
    let _ = cmd.spawn();
    Ok(())
}

fn app_executable_name(app: &str, manifest: &UpdateManifest) -> String {
    let exe = format!("{app}.exe");
    if manifest.files.iter().any(|f| f == &exe) {
        return exe;
    }
    app.to_string()
}

fn channel_label(channel: UpdateChannel) -> &'static str {
    match channel {
        UpdateChannel::Stable => "stable",
        UpdateChannel::Nightly => "nightly",
    }
}
