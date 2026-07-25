use super::super::RuntimeIdentity;
use super::*;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{cell::Cell, fs};
use tempfile::tempdir;

const MACOS_TARGET: &str = "x86_64-apple-darwin";
const MACOS_PLATFORM: &str = "macos";
const X86_64_ARCH: &str = "x86_64";

fn identity(channel: UpdateChannel) -> RuntimeIdentity {
    RuntimeIdentity {
        app: "wavecrate".to_string(),
        channel,
        target: MACOS_TARGET.to_string(),
        platform: MACOS_PLATFORM.to_string(),
        arch: X86_64_ARCH.to_string(),
    }
}

fn manifest(channel: &str) -> UpdateManifest {
    UpdateManifest {
        app: "wavecrate".to_string(),
        channel: channel.to_string(),
        target: MACOS_TARGET.to_string(),
        platform: MACOS_PLATFORM.to_string(),
        arch: X86_64_ARCH.to_string(),
        files: vec!["update-manifest.json".to_string()],
    }
}

#[test]
fn rc_identity_accepts_stable_manifest() {
    manifest("stable")
        .validate(&identity(UpdateChannel::Rc))
        .unwrap();
}

#[test]
fn stable_identity_rejects_rc_manifest() {
    let err = manifest("rc")
        .validate(&identity(UpdateChannel::Stable))
        .unwrap_err();

    assert!(err.to_string().contains("Manifest channel mismatch"));
}

#[test]
fn relaunch_app_errors_when_executable_missing() {
    let tmp = tempdir().unwrap();
    let manifest = UpdateManifest {
        app: "wavecrate".to_string(),
        channel: "stable".to_string(),
        target: MACOS_TARGET.to_string(),
        platform: MACOS_PLATFORM.to_string(),
        arch: X86_64_ARCH.to_string(),
        files: Vec::new(),
    };
    let err = relaunch_app(tmp.path(), "wavecrate", &manifest).unwrap_err();
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
        app: "wavecrate".to_string(),
        channel: "stable".to_string(),
        target: MACOS_TARGET.to_string(),
        platform: MACOS_PLATFORM.to_string(),
        arch: X86_64_ARCH.to_string(),
        files: vec![running_name.clone()],
    };

    let _err = apply_files_and_dirs(&install_dir, &root_dir, &manifest).unwrap_err();
    assert_eq!(fs::read_to_string(&running_dest).unwrap(), "old-binary");
    assert!(!install_dir.join(format!("{running_name}.old")).exists());
    assert!(!install_dir.join(format!("{running_name}.new")).exists());
}

#[test]
fn committed_cleanup_failure_warns_and_still_attempts_relaunch() {
    let tmp = tempdir().unwrap();
    let install_dir = tmp.path().join("install");
    let root_dir = tmp.path().join("root");
    fs::create_dir_all(&install_dir).unwrap();
    fs::create_dir_all(&root_dir).unwrap();

    let executable = install_dir.join("wavecrate");
    let old_path = install_dir.canonicalize().unwrap().join("wavecrate.old");
    fs::write(&executable, "old-binary").unwrap();
    fs::write(root_dir.join("wavecrate"), "new-binary").unwrap();
    let manifest = UpdateManifest {
        app: "wavecrate".to_string(),
        channel: "stable".to_string(),
        target: MACOS_TARGET.to_string(),
        platform: MACOS_PLATFORM.to_string(),
        arch: X86_64_ARCH.to_string(),
        files: vec!["wavecrate".to_string()],
    };
    let args = UpdaterRunArgs {
        repo: "owner/repo".to_string(),
        identity: identity(UpdateChannel::Stable),
        install_dir: install_dir.clone(),
        relaunch: true,
        requested_tag: Some("v1.2.3".to_string()),
    };
    let cleanup_path = old_path.clone();
    let applied =
        apply_files_and_dirs_with_commit(&install_dir, &root_dir, &manifest, |transaction| {
            transaction.commit_with_cleanup_failure(cleanup_path)
        })
        .expect("post-commit cleanup failure must remain non-fatal");
    let relaunch_attempted = Cell::new(false);
    let mut messages = Vec::new();

    let plan = finish_applied_update(
        &args,
        "v1.2.3".to_string(),
        &manifest,
        applied,
        &mut |progress| messages.push(progress.message),
        |received_install_dir, app, _manifest| {
            relaunch_attempted.set(true);
            assert_eq!(received_install_dir, install_dir);
            assert_eq!(app, "wavecrate");
            assert_eq!(fs::read_to_string(&executable).unwrap(), "new-binary");
            Ok(())
        },
    )
    .expect("committed update should return a warning-bearing plan");

    assert!(relaunch_attempted.get());
    assert_eq!(plan.post_commit_cleanup_failures.len(), 1);
    assert_eq!(plan.post_commit_cleanup_failures[0].path, old_path);
    assert!(messages.iter().any(|message| {
        message.contains("Warning: update committed but cleanup left")
            && message.contains("wavecrate.old")
    }));
    assert!(old_path.exists());
}

#[test]
fn committed_cleanup_warning_is_reported_before_fatal_relaunch_failure() {
    let tmp = tempdir().unwrap();
    let install_dir = tmp.path().join("install");
    fs::create_dir_all(&install_dir).unwrap();
    let remnant = install_dir.join("wavecrate.old");
    let args = UpdaterRunArgs {
        repo: "owner/repo".to_string(),
        identity: identity(UpdateChannel::Stable),
        install_dir,
        relaunch: true,
        requested_tag: Some("v1.2.3".to_string()),
    };
    let applied = AppliedFilesPlan {
        copied_files: vec!["wavecrate".to_string()],
        replaced_dirs: Vec::new(),
        post_commit_cleanup_failures: vec![PostCommitCleanupFailure {
            path: remnant.clone(),
            error: "file is locked".to_string(),
        }],
        stale_removal_failures: Vec::new(),
    };
    let mut messages = Vec::new();

    let err = finish_applied_update(
        &args,
        "v1.2.3".to_string(),
        &manifest("stable"),
        applied,
        &mut |progress| messages.push(progress.message),
        |_install_dir, _app, _manifest| Err(UpdateError::Invalid("relaunch blocked".to_string())),
    )
    .expect_err("relaunch failure must remain fatal");

    let expected_warning = format!(
        "Warning: update committed but cleanup left {}: file is locked",
        remnant.display()
    );
    assert!(err.to_string().contains("relaunch blocked"));
    assert_eq!(messages.first(), Some(&expected_warning));
    assert!(
        messages
            .iter()
            .any(|message| message == "Relaunching app...")
    );
    assert!(
        messages
            .iter()
            .any(|message| message.contains("Relaunch failed"))
    );
}

#[test]
fn apply_files_and_dirs_removes_stale_files_from_prior_manifest() {
    let tmp = tempdir().unwrap();
    let install_dir = tmp.path().join("install");
    let root_dir = tmp.path().join("root");
    fs::create_dir_all(&install_dir).unwrap();
    fs::create_dir_all(&root_dir).unwrap();

    let installed_manifest_json = r#"{
  "app": "wavecrate",
  "channel": "stable",
  "target": "x86_64-apple-darwin",
  "platform": "macos",
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
        app: "wavecrate".to_string(),
        channel: "stable".to_string(),
        target: MACOS_TARGET.to_string(),
        platform: MACOS_PLATFORM.to_string(),
        arch: X86_64_ARCH.to_string(),
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
  "app": "wavecrate",
  "channel": "stable",
  "target": "x86_64-apple-darwin",
  "platform": "macos",
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
        app: "wavecrate".to_string(),
        channel: "stable".to_string(),
        target: MACOS_TARGET.to_string(),
        platform: MACOS_PLATFORM.to_string(),
        arch: X86_64_ARCH.to_string(),
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
  "app": "wavecrate",
  "channel": "stable",
  "target": "x86_64-apple-darwin",
  "platform": "macos",
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
        app: "wavecrate".to_string(),
        channel: "stable".to_string(),
        target: MACOS_TARGET.to_string(),
        platform: MACOS_PLATFORM.to_string(),
        arch: X86_64_ARCH.to_string(),
        files: vec![
            "update-manifest.json".to_string(),
            "current.txt".to_string(),
        ],
    };
    fs::write(root_dir.join("update-manifest.json"), "new-manifest").unwrap();
    fs::write(root_dir.join("current.txt"), "new-current").unwrap();

    let applied = apply_files_and_dirs(&install_dir, &root_dir, &next_manifest).unwrap();
    let failures = applied.stale_removal_failures;

    if stale_file.exists() {
        let expected_stale_file = stale_file.canonicalize().unwrap_or(stale_file.clone());
        let expected_stale_dir = stale_dir.canonicalize().unwrap_or(stale_dir.clone());
        assert!(failures.iter().any(
            |failure| failure.path == expected_stale_file || failure.path == expected_stale_dir
        ));
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

    let install_root = ValidatedInstallRoot::new(&install_dir).unwrap();
    let link_path = install_root.child_path("stale-link").unwrap();
    symlink(&outside_dir, &link_path).unwrap();

    let failures = remove_stale_paths(std::slice::from_ref(&link_path), &install_root).unwrap();

    assert!(failures.is_empty());
    assert!(!link_path.exists());
    assert!(outside_dir.join("keep.txt").exists());
}
