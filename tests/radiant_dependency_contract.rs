//! Contract tests for the pinned public Radiant Cargo dependency.

use std::fs;
use std::path::Path;
use std::process::Command;

const RADIANT_REPOSITORY: &str = "https://github.com/PORTALSURFER/radiant.git";
const RADIANT_REVISION: &str = "0430556e927fbfd35491c617c8ca13156b5dbf9c";

#[test]
fn canonical_radiant_dependency_uses_the_exact_git_revision() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = fs::read_to_string(root.join("Cargo.toml")).expect("Cargo.toml");
    let lockfile = fs::read_to_string(root.join("Cargo.lock")).expect("Cargo.lock");

    assert!(manifest.contains(&format!(
        "radiant = {{ git = \"{RADIANT_REPOSITORY}\", rev = \"{RADIANT_REVISION}\" }}"
    )));
    assert!(lockfile.contains(&format!(
        "source = \"git+{RADIANT_REPOSITORY}?rev={RADIANT_REVISION}#{RADIANT_REVISION}\""
    )));

    for stale in [
        "radiant-dependency.toml",
        ".gitmodules",
        "vendor/radiant",
        "scripts/radiant.sh",
        "scripts/radiant.ps1",
        "scripts/internal/radiant",
        "scripts/internal/release/provision_radiant_sibling.sh",
        ".github/workflows/radiant-sibling-smoke.yml",
    ] {
        assert!(
            !root.join(stale).exists(),
            "stale sibling machinery remains: {stale}"
        );
    }
}

#[test]
fn source_tree_has_no_sibling_provisioning_contract() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for path in [
        "scripts/bootstrap.sh",
        "scripts/bootstrap.ps1",
        "scripts/doctor.sh",
        "scripts/doctor.ps1",
        "scripts/release.sh",
        "scripts/internal/ci/ci_agent.sh",
        "scripts/internal/ci/ci_agent.ps1",
        "scripts/internal/ci/devcheck.sh",
        "scripts/internal/ci/devcheck.ps1",
        "scripts/internal/gui/run_gui_contract.ps1",
        "scripts/internal/perf/run_perf_guard.sh",
        "scripts/internal/release/run_release_validation.sh",
        "scripts/internal/release/pull_and_run_release.ps1",
        "scripts/internal/worktree/worktree_task.sh",
        "scripts/internal/worktree/worktree_task.ps1",
        "run.sh",
    ] {
        let contents = fs::read_to_string(root.join(path)).expect(path);
        assert!(
            !contents.contains("scripts/radiant"),
            "{path} invokes sibling helper"
        );
        assert!(
            !contents.contains("RADIANT_SUBMODULE_DEPLOY_KEY"),
            "{path} provisions sibling"
        );
        assert!(
            !contents.contains("RADIANT_REPOSITORY_DEPLOY_KEY"),
            "{path} provisions sibling"
        );
        assert!(
            !contents.contains("manifest-path ../radiant"),
            "{path} targets sibling manifest"
        );
    }
}

#[test]
fn bootstrap_and_doctor_resolve_the_locked_dependency_graph() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for path in [
        "scripts/bootstrap.sh",
        "scripts/bootstrap.ps1",
        "scripts/doctor.sh",
        "scripts/doctor.ps1",
    ] {
        let contents = fs::read_to_string(root.join(path)).expect(path);
        assert!(
            contents.contains("cargo metadata --locked --format-version 1"),
            "{path} must resolve the locked dependency graph"
        );
        assert!(
            !contents.contains("cargo metadata --locked --no-deps"),
            "{path} must not skip dependency resolution"
        );
    }
}

#[cfg(unix)]
#[test]
fn bash_bootstrap_metadata_fixture_distinguishes_success_and_failure() {
    use std::os::unix::fs::PermissionsExt;

    fn write_executable(path: &Path, contents: &str) {
        fs::write(path, contents).expect("write fixture executable");
        let mut permissions = fs::metadata(path).expect("fixture metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("make fixture executable");
    }

    let temp = tempfile::tempdir().expect("create bootstrap fixture");
    let root = temp.path();
    let scripts = root.join("scripts");
    let bin = root.join("bin");
    fs::create_dir_all(&scripts).expect("create fixture scripts");
    fs::create_dir_all(&bin).expect("create fixture bin");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/bootstrap.sh"),
        scripts.join("bootstrap.sh"),
    )
    .expect("copy bootstrap script");
    write_executable(&scripts.join("agent.sh"), "#!/bin/sh\nexit 0\n");
    fs::write(
        root.join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"stable\"\n",
    )
    .expect("write toolchain fixture");
    let mode_file = root.join("cargo-mode");
    write_executable(
        &bin.join("cargo"),
        &format!(
            "#!/bin/sh\nif [ \"$1\" = metadata ] && [ \"$(cat \"{}\")\" = fail ]; then exit 1; fi\nexit 0\n",
            mode_file.display()
        ),
    );
    write_executable(
        &bin.join("rustup"),
        "#!/bin/sh\nif [ \"$1\" = component ]; then printf 'rustfmt\\nclippy\\n'; fi\nexit 0\n",
    );

    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").expect("PATH must be available")
    );
    let run = |mode: &str| {
        fs::write(&mode_file, mode).expect("write cargo mode");
        Command::new("bash")
            .arg(scripts.join("bootstrap.sh"))
            .current_dir(root)
            .env("PATH", &path)
            .env("WAVECRATE_SKIP_AGENT_PREFLIGHT_HOOK_INSTALL", "0")
            .output()
            .expect("run bootstrap fixture")
    };

    let success = run("success");
    assert!(
        success.status.success(),
        "success fixture failed: {success:?}"
    );
    let success_output = String::from_utf8_lossy(&success.stdout);
    assert!(success_output.contains("[bootstrap] Result: OK"));

    let failure = run("fail");
    assert!(
        !failure.status.success(),
        "failure fixture unexpectedly passed"
    );
    let failure_output = format!(
        "{}{}",
        String::from_utf8_lossy(&failure.stdout),
        String::from_utf8_lossy(&failure.stderr)
    );
    assert!(failure_output.contains("Cargo dependency resolution: FAILED"));
    assert!(!failure_output.contains("[bootstrap] Result: OK"));
}
