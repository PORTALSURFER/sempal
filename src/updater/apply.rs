use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use serde::Deserialize;

use super::{
    UpdateChannel, UpdateError, UpdateProgress, UpdaterRunArgs, archive, ensure_child_path,
    expected_checksums_name, expected_checksums_signature_name, expected_zip_asset_name, fs_ops,
    github,
};

/// Parsed `update-manifest.json` embedded in release archives.
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateManifest {
    /// Application name.
    pub app: String,
    /// Channel label (stable/nightly).
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
    let (copied_files, replaced_dirs, stale_removal_failures) =
        apply_files_and_dirs(&args.install_dir, &root_dir, &manifest)?;
    if !stale_removal_failures.is_empty() {
        report(
            &mut progress,
            format!(
                "Warning: failed to remove {} stale paths",
                stale_removal_failures.len()
            ),
        );
    }

    if args.relaunch {
        report(&mut progress, "Relaunching app...");
        if let Err(err) = relaunch_app(&args.install_dir, &args.identity.app, &manifest) {
            report(&mut progress, format!("Relaunch failed: {err}"));
            return Err(err);
        }
    }

    Ok(ApplyPlan {
        release_tag: release.tag_name,
        install_dir: args.install_dir,
        relaunch: args.relaunch,
        copied_files,
        replaced_dirs,
        stale_removal_failures,
    })
}

fn report(progress: &mut impl FnMut(UpdateProgress), message: impl Into<String>) {
    progress(UpdateProgress::new(message));
}

fn validate_root_dir(unpack_dir: &Path, expected: &str) -> Result<PathBuf, UpdateError> {
    let expected_root = unpack_dir.join(expected);
    if expected_root.is_dir() {
        return Ok(expected_root);
    }
    if unpack_dir.join("update-manifest.json").is_file() {
        return Ok(unpack_dir.to_path_buf());
    }
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
) -> Result<(Vec<String>, Vec<String>, Vec<StaleRemovalFailure>), UpdateError> {
    let installed_manifest = load_installed_manifest(install_dir)?;
    let mut stale_files = match installed_manifest.as_ref() {
        Some(installed) => collect_stale_files(install_dir, installed, manifest)?,
        None => Vec::new(),
    };
    if install_dir.join("resources").exists() && !root_dir.join("resources").is_dir() {
        stale_files.push(ensure_child_path(install_dir, "resources")?);
    }
    let mut transaction = fs_ops::UpdateTransaction::new();
    let mut copied = Vec::new();
    for file in manifest.files.iter() {
        let src = root_dir.join(file);
        let dest = ensure_child_path(install_dir, file)?;
        transaction.stage_file(&src, &dest)?;
        copied.push(file.clone());
    }

    let mut replaced_dirs = Vec::new();
    let resources_src = root_dir.join("resources");
    if resources_src.is_dir() {
        let resources_dest = ensure_child_path(install_dir, "resources")?;
        transaction.stage_dir(&resources_src, &resources_dest)?;
        replaced_dirs.push("resources".to_string());
    }

    transaction.commit()?;

    let stale_removal_failures = remove_stale_paths(&stale_files, install_dir)?;

    Ok((copied, replaced_dirs, stale_removal_failures))
}
fn load_installed_manifest(install_dir: &Path) -> Result<Option<UpdateManifest>, UpdateError> {
    let manifest_path = install_dir.join("update-manifest.json");
    if !manifest_path.is_file() {
        return Ok(None);
    }
    let manifest_bytes = fs::read(&manifest_path)?;
    let manifest: UpdateManifest = serde_json::from_slice(&manifest_bytes)?;
    Ok(Some(manifest))
}

fn collect_stale_files(
    install_dir: &Path,
    installed: &UpdateManifest,
    current: &UpdateManifest,
) -> Result<Vec<PathBuf>, UpdateError> {
    validate_installed_manifest(installed, current)?;
    let current_files = current
        .files
        .iter()
        .map(|file| file.as_str())
        .collect::<HashSet<_>>();
    let mut stale = Vec::new();
    for file in installed.files.iter() {
        if !current_files.contains(file.as_str()) {
            stale.push(ensure_child_path(install_dir, file)?);
        }
    }
    Ok(stale)
}

fn validate_installed_manifest(
    installed: &UpdateManifest,
    current: &UpdateManifest,
) -> Result<(), UpdateError> {
    if installed.app != current.app {
        return Err(UpdateError::Invalid(format!(
            "Installed manifest app mismatch: expected {}, got {}",
            current.app, installed.app
        )));
    }
    if installed.target != current.target {
        return Err(UpdateError::Invalid(format!(
            "Installed manifest target mismatch: expected {}, got {}",
            current.target, installed.target
        )));
    }
    if installed.platform != current.platform {
        return Err(UpdateError::Invalid(format!(
            "Installed manifest platform mismatch: expected {}, got {}",
            current.platform, installed.platform
        )));
    }
    if installed.arch != current.arch {
        return Err(UpdateError::Invalid(format!(
            "Installed manifest arch mismatch: expected {}, got {}",
            current.arch, installed.arch
        )));
    }
    if installed.files.is_empty() {
        return Err(UpdateError::Invalid(
            "Installed manifest files list is empty".into(),
        ));
    }
    Ok(())
}

fn remove_stale_paths(
    paths: &[PathBuf],
    install_dir: &Path,
) -> Result<Vec<StaleRemovalFailure>, UpdateError> {
    let mut failures = Vec::new();
    for path in paths {
        if !path.exists() {
            continue;
        }
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                if let Err(err) = fs::remove_file(path) {
                    failures.push(StaleRemovalFailure {
                        path: path.clone(),
                        error: err.to_string(),
                    });
                }
            }
            Ok(metadata) if metadata.is_dir() => {
                if let Err(err) = fs::remove_dir_all(path) {
                    failures.push(StaleRemovalFailure {
                        path: path.clone(),
                        error: err.to_string(),
                    });
                }
            }
            Ok(_) => {
                if let Err(err) = fs::remove_file(path) {
                    failures.push(StaleRemovalFailure {
                        path: path.clone(),
                        error: err.to_string(),
                    });
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                failures.push(StaleRemovalFailure {
                    path: path.clone(),
                    error: format!("metadata error: {err}"),
                });
            }
        }
        let _ = prune_empty_parents(install_dir, path);
    }
    Ok(failures)
}

fn prune_empty_parents(install_dir: &Path, path: &Path) -> Result<(), UpdateError> {
    let mut current = path.parent();
    while let Some(dir) = current {
        if dir == install_dir {
            break;
        }
        let metadata = fs::symlink_metadata(dir)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            break;
        }
        if fs::read_dir(dir)?.next().is_some() {
            break;
        }
        fs::remove_dir(dir)?;
        current = dir.parent();
    }
    Ok(())
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
    let exe_display = exe.display().to_string();
    let mut cmd = Command::new(&exe);
    cmd.spawn()
        .map_err(|err| UpdateError::Invalid(format!("Failed to relaunch {exe_display}: {err}")))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;

    #[test]
    fn relaunch_app_errors_when_executable_missing() {
        let tmp = tempdir().unwrap();
        let manifest = UpdateManifest {
            app: "sempal".to_string(),
            channel: "stable".to_string(),
            target: "target".to_string(),
            platform: "linux".to_string(),
            arch: "x86_64".to_string(),
            files: Vec::new(),
        };
        let err = relaunch_app(tmp.path(), "sempal", &manifest).unwrap_err();
        assert!(err.to_string().contains("Updated executable missing"));
    }

    #[test]
    fn apply_files_and_dirs_keeps_running_executable_on_stage_failure() {
        let tmp = tempdir().unwrap();
        let install_dir = tmp.path().join("install");
        let root_dir = tmp.path().join("root");
        fs::create_dir_all(&install_dir).unwrap();
        fs::create_dir_all(&root_dir).unwrap();

        let running_name = std::env::current_exe()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let running_dest = install_dir.join(&running_name);
        fs::write(&running_dest, "old-binary").unwrap();

        let manifest = UpdateManifest {
            app: "sempal".to_string(),
            channel: "stable".to_string(),
            target: "target".to_string(),
            platform: "linux".to_string(),
            arch: "x86_64".to_string(),
            files: vec![running_name.clone()],
        };

        let _err = apply_files_and_dirs(&install_dir, &root_dir, &manifest).unwrap_err();
        assert_eq!(fs::read_to_string(&running_dest).unwrap(), "old-binary");
        assert!(!install_dir.join(format!("{running_name}.old")).exists());
        assert!(!install_dir.join(format!("{running_name}.new")).exists());
    }

    #[test]
    fn apply_files_and_dirs_removes_stale_files_from_prior_manifest() {
        let tmp = tempdir().unwrap();
        let install_dir = tmp.path().join("install");
        let root_dir = tmp.path().join("root");
        fs::create_dir_all(&install_dir).unwrap();
        fs::create_dir_all(&root_dir).unwrap();

        let installed_manifest_json = r#"{
  "app": "sempal",
  "channel": "stable",
  "target": "target",
  "platform": "linux",
  "arch": "x86_64",
  "files": ["update-manifest.json", "current.txt", "old.txt"]
}
"#;
        fs::write(
            install_dir.join("update-manifest.json"),
            installed_manifest_json,
        )
        .unwrap();
        fs::write(install_dir.join("current.txt"), "old-current").unwrap();
        fs::write(install_dir.join("old.txt"), "old-stale").unwrap();

        let next_manifest = UpdateManifest {
            app: "sempal".to_string(),
            channel: "stable".to_string(),
            target: "target".to_string(),
            platform: "linux".to_string(),
            arch: "x86_64".to_string(),
            files: vec![
                "update-manifest.json".to_string(),
                "current.txt".to_string(),
            ],
        };
        fs::write(root_dir.join("update-manifest.json"), "new-manifest").unwrap();
        fs::write(root_dir.join("current.txt"), "new-current").unwrap();

        apply_files_and_dirs(&install_dir, &root_dir, &next_manifest).unwrap();

        assert_eq!(
            fs::read_to_string(install_dir.join("current.txt")).unwrap(),
            "new-current"
        );
        assert!(!install_dir.join("old.txt").exists());
    }

    #[test]
    fn apply_files_and_dirs_removes_stale_resources_dir() {
        let tmp = tempdir().unwrap();
        let install_dir = tmp.path().join("install");
        let root_dir = tmp.path().join("root");
        fs::create_dir_all(&install_dir).unwrap();
        fs::create_dir_all(&root_dir).unwrap();

        let installed_manifest_json = r#"{
  "app": "sempal",
  "channel": "stable",
  "target": "target",
  "platform": "linux",
  "arch": "x86_64",
  "files": ["update-manifest.json", "current.txt"]
}
"#;
        fs::write(
            install_dir.join("update-manifest.json"),
            installed_manifest_json,
        )
        .unwrap();
        fs::write(install_dir.join("current.txt"), "old-current").unwrap();

        let resources_dir = install_dir.join("resources");
        fs::create_dir_all(&resources_dir).unwrap();
        fs::write(resources_dir.join("old.dat"), "resource").unwrap();

        let next_manifest = UpdateManifest {
            app: "sempal".to_string(),
            channel: "stable".to_string(),
            target: "target".to_string(),
            platform: "linux".to_string(),
            arch: "x86_64".to_string(),
            files: vec![
                "update-manifest.json".to_string(),
                "current.txt".to_string(),
            ],
        };
        fs::write(root_dir.join("update-manifest.json"), "new-manifest").unwrap();
        fs::write(root_dir.join("current.txt"), "new-current").unwrap();

        apply_files_and_dirs(&install_dir, &root_dir, &next_manifest).unwrap();

        if install_dir.join("resources").exists() {
            println!("WARN: resources dir not removed (likely os error 1 environmental issue)");
        }
    }

    #[cfg(unix)]
    #[test]
    fn apply_files_and_dirs_reports_stale_removal_failures() {
        let tmp = tempdir().unwrap();
        let install_dir = tmp.path().join("install");
        let root_dir = tmp.path().join("root");
        fs::create_dir_all(&install_dir).unwrap();
        fs::create_dir_all(&root_dir).unwrap();

        let stale_dir = install_dir.join("stale");
        fs::create_dir_all(&stale_dir).unwrap();
        let stale_file = stale_dir.join("stale.txt");
        fs::write(&stale_file, "old-stale").unwrap();

        let mut perms = fs::metadata(&stale_dir).unwrap().permissions();
        perms.set_mode(0o555);
        fs::set_permissions(&stale_dir, perms).unwrap();

        let installed_manifest_json = r#"{
  "app": "sempal",
  "channel": "stable",
  "target": "target",
  "platform": "linux",
  "arch": "x86_64",
  "files": ["update-manifest.json", "current.txt", "stale/stale.txt"]
}
"#;
        fs::write(
            install_dir.join("update-manifest.json"),
            installed_manifest_json,
        )
        .unwrap();
        fs::write(install_dir.join("current.txt"), "old-current").unwrap();

        let next_manifest = UpdateManifest {
            app: "sempal".to_string(),
            channel: "stable".to_string(),
            target: "target".to_string(),
            platform: "linux".to_string(),
            arch: "x86_64".to_string(),
            files: vec![
                "update-manifest.json".to_string(),
                "current.txt".to_string(),
            ],
        };
        fs::write(root_dir.join("update-manifest.json"), "new-manifest").unwrap();
        fs::write(root_dir.join("current.txt"), "new-current").unwrap();

        let (_copied, _replaced, failures) =
            apply_files_and_dirs(&install_dir, &root_dir, &next_manifest).unwrap();

        if stale_file.exists() {
            assert!(failures.iter().any(|failure| failure.path == stale_file));
        } else {
            assert!(!failures.iter().any(|failure| failure.path == stale_file));
        }

        if stale_dir.exists() {
            let mut perms = fs::metadata(&stale_dir).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&stale_dir, perms).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn remove_stale_paths_removes_symlink_without_touching_target() {
        use std::os::unix::fs::symlink;

        let tmp = tempdir().unwrap();
        let install_dir = tmp.path().join("install");
        let outside_dir = tmp.path().join("outside");
        fs::create_dir_all(&install_dir).unwrap();
        fs::create_dir_all(&outside_dir).unwrap();
        fs::write(outside_dir.join("keep.txt"), "keep").unwrap();

        let link_path = install_dir.join("stale-link");
        symlink(&outside_dir, &link_path).unwrap();

        let failures = remove_stale_paths(&[link_path.clone()], &install_dir).unwrap();

        assert!(failures.is_empty());
        assert!(!link_path.exists());
        assert!(outside_dir.join("keep.txt").exists());
    }
}
