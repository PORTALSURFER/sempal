use super::*;
use std::cell::RefCell;

fn sample_install_dir() -> PathBuf {
    PathBuf::from(r"C:\tmp\wavecrate")
}

fn sample_args() -> UpdaterRunArgs {
    UpdaterRunArgs {
        repo: REPO_SLUG.to_string(),
        identity: RuntimeIdentity {
            app: APP_NAME.to_string(),
            channel: UpdateChannel::Stable,
            target: default_target().expect("supported test target"),
            platform: default_platform().expect("supported test platform"),
            arch: default_arch().expect("supported test arch"),
        },
        install_dir: sample_install_dir(),
        relaunch: true,
        requested_tag: None,
    }
}

fn assert_defaults(args: &UpdaterRunArgs) {
    assert_eq!(args.repo, REPO_SLUG);
    assert_eq!(args.identity.app, APP_NAME);
    assert_eq!(args.identity.channel, UpdateChannel::Stable);
    assert_eq!(
        args.identity.target,
        default_target().expect("supported test target")
    );
    assert_eq!(
        args.identity.platform,
        default_platform().expect("supported test platform")
    );
    assert_eq!(
        args.identity.arch,
        default_arch().expect("supported test arch")
    );
    assert_eq!(args.install_dir, sample_install_dir());
    assert!(args.relaunch);
    assert_eq!(args.requested_tag, None);
}

#[test]
fn parse_args_returns_help_text() {
    let err = parse_args(vec!["--help".to_string()]).unwrap_err();
    assert_eq!(err, help_text());
}

#[test]
fn parse_args_requires_install_dir() {
    let err = parse_args(Vec::new()).unwrap_err();
    assert!(err.starts_with("Missing --install-dir"));
    assert!(err.contains("Usage:"));
}

#[test]
fn parse_args_requires_values_for_value_flags() {
    let err = parse_args(vec!["--repo".to_string()]).unwrap_err();
    assert_eq!(err, "Missing value for --repo");
}

#[test]
fn parse_args_uses_detected_defaults_for_headless_runs() {
    let (args, headless) = parse_args(vec![
        "--install-dir".to_string(),
        sample_install_dir().display().to_string(),
        "--headless".to_string(),
    ])
    .expect("parse args");

    assert_defaults(&args);
    assert!(headless);
}

#[test]
fn parse_args_applies_explicit_overrides() {
    let (args, headless) = parse_args(vec![
        "--repo".to_string(),
        "owner/repo".to_string(),
        "--channel".to_string(),
        "nightly".to_string(),
        "--install-dir".to_string(),
        sample_install_dir().display().to_string(),
        "--no-relaunch".to_string(),
        "--tag".to_string(),
        "nightly".to_string(),
        "--target".to_string(),
        "custom-target".to_string(),
        "--platform".to_string(),
        "custom-platform".to_string(),
        "--arch".to_string(),
        "custom-arch".to_string(),
        "--headless".to_string(),
    ])
    .expect("parse args");

    assert_eq!(args.repo, "owner/repo");
    assert_eq!(args.identity.channel, UpdateChannel::Nightly);
    assert_eq!(args.identity.target, "custom-target");
    assert_eq!(args.identity.platform, "custom-platform");
    assert_eq!(args.identity.arch, "custom-arch");
    assert_eq!(args.install_dir, sample_install_dir());
    assert!(!args.relaunch);
    assert_eq!(args.requested_tag.as_deref(), Some("nightly"));
    assert!(headless);
}

#[test]
fn parse_args_rejects_unknown_channel() {
    let err = parse_args(vec![
        "--channel".to_string(),
        "beta".to_string(),
        "--install-dir".to_string(),
        sample_install_dir().display().to_string(),
    ])
    .unwrap_err();

    assert_eq!(err, "Unknown channel 'beta'");
}

#[test]
fn parse_args_accepts_rc_channel() {
    let (args, _headless) = parse_args(vec![
        "--channel".to_string(),
        "rc".to_string(),
        "--install-dir".to_string(),
        sample_install_dir().display().to_string(),
    ])
    .expect("parse args");

    assert_eq!(args.identity.channel, UpdateChannel::Rc);
}

#[test]
fn parse_args_rejects_unknown_argument_with_help_text() {
    let err = parse_args(vec![
        "--install-dir".to_string(),
        sample_install_dir().display().to_string(),
        "--bogus".to_string(),
    ])
    .unwrap_err();

    assert!(err.starts_with("Unknown argument '--bogus'"));
    assert!(err.contains("Usage:"));
}

#[test]
fn run_headless_with_passes_args_through_to_apply() {
    let args = sample_args();
    let captured = RefCell::new(None);
    let plan = run_headless_with(
        args.clone(),
        |received, _progress| {
            captured.replace(Some(received));
            Ok(ApplyPlan {
                release_tag: "v1.2.3".to_string(),
                install_dir: sample_install_dir(),
                relaunch: true,
                copied_files: Vec::new(),
                replaced_dirs: Vec::new(),
                post_commit_cleanup_failures: Vec::new(),
                stale_removal_failures: Vec::new(),
            })
        },
        |_| {},
    )
    .expect("headless run");

    let received = captured
        .into_inner()
        .expect("apply closure should receive updater args");
    assert_eq!(received.repo, args.repo);
    assert_eq!(received.identity.app, args.identity.app);
    assert_eq!(received.identity.channel, args.identity.channel);
    assert_eq!(received.identity.target, args.identity.target);
    assert_eq!(received.identity.platform, args.identity.platform);
    assert_eq!(received.identity.arch, args.identity.arch);
    assert_eq!(received.install_dir, args.install_dir);
    assert_eq!(received.relaunch, args.relaunch);
    assert_eq!(received.requested_tag, args.requested_tag);
    assert_eq!(plan.release_tag, "v1.2.3");
    assert_eq!(plan.install_dir, sample_install_dir());
}

#[test]
fn run_headless_with_forwards_cleanup_warning_before_apply_error() {
    let remnant = sample_install_dir().join("wavecrate.exe.old");
    let expected_warning = format!(
        "Warning: update committed but cleanup left {}: file is locked",
        remnant.display()
    );
    let mut warnings = Vec::new();

    let err = run_headless_with(
        sample_args(),
        |_, progress| {
            progress(UpdateProgress::new(expected_warning.clone()));
            Err(UpdateError::Invalid("relaunch blocked".to_string()))
        },
        |warning| warnings.push(warning),
    )
    .expect_err("relaunch failure must remain fatal");

    assert!(err.contains("relaunch blocked"));
    assert_eq!(warnings, vec![expected_warning]);
}

#[test]
fn run_headless_with_returns_apply_errors_as_strings() {
    let err = run_headless_with(
        sample_args(),
        |_, _progress| Err(UpdateError::Invalid("test failure".to_string())),
        |_| {},
    )
    .unwrap_err();

    assert_eq!(err, "Invalid update: test failure");
}
