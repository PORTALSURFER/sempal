//! Checks for the repo-root release orchestration wrapper.
#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

#[test]
fn prepare_minor_bump_derives_target_version_branch_and_runs_prep_helper() {
    let repo = FixtureRepo::new();
    repo.write_workspace("0.19.1");
    repo.commit_all("seed workspace");
    repo.push_branch("main");

    let output = repo.run_release(&[
        "prepare",
        "--bump",
        "minor",
        "--source-ref",
        "main",
        "--dry-run",
    ]);

    assert_success(&output);
    let stdout = stdout(&output);
    assert!(stdout.contains("Resolved version: 0.20.0"));
    assert!(stdout.contains("Release branch: release/0.20"));
    assert!(stdout.contains("prepare-helper [--version] [0.20.0]"));
    assert!(stdout.contains("[--dry-run]"));
    assert!(stdout.contains("release-train-prepare.yml"));
}

#[test]
fn prepare_major_bump_derives_next_major_train() {
    let repo = FixtureRepo::new();
    repo.write_workspace("0.19.1");
    repo.commit_all("seed workspace");
    repo.push_branch("main");

    let output = repo.run_release(&["prepare", "--bump", "major", "--source-ref", "main"]);

    assert_success(&output);
    let stdout = stdout(&output);
    assert!(stdout.contains("Resolved version: 1.0.0"));
    assert!(stdout.contains("Release branch: release/1.0"));
}

#[test]
fn prepare_patch_bump_derives_next_patch_on_existing_release_train() {
    let repo = FixtureRepo::new();
    repo.write_workspace("0.19.3");
    repo.commit_all("seed release train workspace");
    repo.create_release_branch("release/0.19");
    let release_sha = repo.git_stdout(&["rev-parse", "origin/release/0.19"]);
    repo.write_workspace("20.5.0");
    repo.commit_all("advance local checkout after release branch");
    repo.push_branch("main");

    let output = repo.run_release(&[
        "prepare",
        "--bump",
        "patch",
        "--source-ref",
        "release/0.19",
        "--dry-run",
    ]);

    assert_success(&output);
    let stdout = stdout(&output);
    assert!(stdout.contains("Resolved version: 0.19.4"));
    assert!(stdout.contains("Release branch: release/0.19"));
    assert!(stdout.contains(&format!("Target SHA: {release_sha}")));
    assert!(stdout.contains("prepare-helper [--version] [0.19.4]"));
    assert!(stdout.contains("[cwd-version=0.19.3]"));
}

#[test]
fn prepare_derives_bump_from_resolved_source_ref_not_local_checkout() {
    let repo = FixtureRepo::new();
    repo.write_workspace("0.19.1");
    repo.commit_all("seed workspace");
    repo.push_branch("main");
    repo.git(&["switch", "-c", "stale-local"]);
    repo.git(&["switch", "main"]);
    repo.write_workspace("20.5.0");
    repo.commit_all("advance main release version");
    repo.push_branch("main");
    repo.git(&["switch", "stale-local"]);
    assert!(
        repo.read("Cargo.toml").contains("version = \"0.19.1\""),
        "fixture checkout should stay on the stale package version"
    );

    let output = repo
        .release_command(&[
            "prepare",
            "--bump",
            "minor",
            "--source-ref",
            "main",
            "--dry-run",
        ])
        .env("GIT_ALLOW_PROTOCOL", "file")
        .output()
        .expect("run release wrapper");

    assert_success(&output);
    let stdout = stdout(&output);
    assert!(stdout.contains("Resolved version: 20.6.0"));
    assert!(stdout.contains("Release branch: release/20.6"));
    assert!(stdout.contains("prepare-helper [--version] [20.6.0]"));
    assert!(stdout.contains("[cwd-version=20.5.0]"));
}

#[test]
fn prepare_dispatch_uses_workflow_ref_for_gh_ref_and_source_sha_input() {
    let repo = FixtureRepo::new();
    repo.write_workspace("0.19.1");
    repo.commit_all("seed workspace");
    repo.push_branch("main");
    let configured_repo = "PORTALSURFER/wavecrate-fork";
    let source_sha = repo.git_stdout(&["rev-parse", "HEAD"]);
    let fake_gh = repo.write_fake_gh();

    let output = repo
        .release_command(&[
            "prepare",
            "--bump",
            "minor",
            "--source-ref",
            &source_sha,
            "--dispatch",
        ])
        .env("WAVECRATE_RELEASE_GH_BIN", fake_gh)
        .env("WAVECRATE_GITHUB_REPO", configured_repo)
        .output()
        .expect("run release wrapper");

    assert_success(&output);
    let stdout = stdout(&output);
    assert!(stdout.contains(
        "fake-gh [workflow] [run] [release-train-prepare.yml] [--repo] [PORTALSURFER/wavecrate-fork] [--ref] [main]"
    ));
    assert!(stdout.contains(&format!("[-f] [source_ref={source_sha}]")));
    assert!(!stdout.contains(&format!("[--ref] [{source_sha}]")));
    assert!(stdout.contains("Workflow: https://github.com/PORTALSURFER/wavecrate-fork/actions/workflows/release-train-prepare.yml"));
    assert!(stdout.contains("Run lookup: gh run list --repo PORTALSURFER/wavecrate-fork"));
}

#[test]
fn prepare_dispatch_rejects_local_only_source_ref() {
    let repo = FixtureRepo::new();
    repo.write_workspace("0.19.1");
    repo.commit_all("seed workspace");
    repo.push_branch("main");
    repo.write_workspace("19.2.0");
    repo.commit_all("local release source only");
    let local_sha = repo.git_stdout(&["rev-parse", "HEAD"]);
    let fake_gh = repo.write_fake_gh();

    let output = repo
        .release_command(&[
            "prepare",
            "--bump",
            "minor",
            "--source-ref",
            "HEAD",
            "--dispatch",
        ])
        .env("WAVECRATE_RELEASE_GH_BIN", fake_gh)
        .output()
        .expect("run release wrapper");

    assert_failure(&output);
    assert!(stderr(&output).contains(&format!(
        "source ref 'HEAD' resolves to local-only commit {local_sha}"
    )));
    assert!(!stdout(&output).contains("fake-gh [workflow] [run]"));
}

#[test]
fn prepare_rejects_invalid_bump_argument() {
    let repo = FixtureRepo::new();
    repo.write_workspace("0.19.1");
    repo.commit_all("seed workspace");
    repo.push_branch("main");

    let output = repo.run_release(&["prepare", "--bump", "micro", "--dry-run"]);

    assert_failure(&output);
    assert!(stderr(&output).contains("bump must be major, minor, or patch"));
}

#[test]
fn release_wrapper_rejects_dirty_worktree_before_release_actions() {
    let repo = FixtureRepo::new();
    repo.write_workspace("0.19.1");
    repo.commit_all("seed workspace");
    repo.push_branch("main");
    repo.write("dirty.txt", "not committed\n");

    let output = repo.run_release(&["prepare", "--bump", "minor", "--dry-run"]);

    assert_failure(&output);
    assert!(stderr(&output).contains("clean working tree"));
}

#[test]
fn release_wrapper_rejects_github_origin_that_differs_from_target_repo() {
    let repo = FixtureRepo::new();
    repo.write_workspace("0.19.1");
    repo.commit_all("seed workspace");
    repo.push_branch("main");
    repo.git(&[
        "remote",
        "set-url",
        "origin",
        "https://github.com/PORTALSURFER/wavecrate.git",
    ]);

    let output = repo
        .release_command(&["prepare", "--bump", "minor", "--source-ref", "main"])
        .env("WAVECRATE_GITHUB_REPO", "PORTALSURFER/wavecrate-fork")
        .output()
        .expect("run release wrapper");

    assert_failure(&output);
    let stderr = stderr(&output);
    assert!(stderr.contains("origin remote resolves release refs from portalsurfer/wavecrate"));
    assert!(stderr.contains("workflows target portalsurfer/wavecrate-fork"));
}

#[test]
fn rc_rejects_wrong_branch_for_version() {
    let repo = FixtureRepo::new();
    repo.write_workspace("19.2.0");
    repo.commit_all("seed workspace");
    repo.push_branch("main");

    let output = repo.run_release(&[
        "rc",
        "--version",
        "19.2.0",
        "--rc-number",
        "1",
        "--branch",
        "release/19.3",
    ]);

    assert_failure(&output);
    assert!(stderr(&output).contains("branch must be release/19.2"));
}

#[test]
fn rc_rejects_local_only_release_branch_before_printing_workflow_command() {
    let repo = FixtureRepo::new();
    repo.write_workspace("19.2.0");
    repo.commit_all("seed workspace");
    repo.git(&["switch", "-c", "release/19.2"]);
    repo.git(&["switch", "main"]);

    let output = repo.run_release(&[
        "rc",
        "--version",
        "19.2.0",
        "--rc-number",
        "1",
        "--branch",
        "release/19.2",
    ]);

    assert_failure(&output);
    assert!(stderr(&output).contains("release branch 'release/19.2' is not present on origin"));
    assert!(!stdout(&output).contains("Dry command: gh workflow run"));
}

#[test]
fn rc_fetches_release_branch_even_when_origin_refspec_only_tracks_main() {
    let repo = FixtureRepo::new();
    repo.write_workspace("19.2.0");
    repo.commit_all("seed workspace");
    repo.create_release_branch("release/19.2");
    repo.git(&["config", "--unset-all", "remote.origin.fetch"]);
    repo.git(&[
        "config",
        "--add",
        "remote.origin.fetch",
        "+refs/heads/main:refs/remotes/origin/main",
    ]);
    repo.git(&["update-ref", "-d", "refs/remotes/origin/release/19.2"]);

    let output = repo.run_release(&[
        "rc",
        "--version",
        "19.2.0",
        "--rc-number",
        "1",
        "--branch",
        "release/19.2",
    ]);

    assert_success(&output);
    let stdout = stdout(&output);
    assert!(stdout.contains("RC tag: v19.2.0-rc.1"));
    assert!(stdout.contains("Dry command: gh workflow run release-rc.yml"));
}

#[test]
fn rc_rejects_existing_tag_that_points_at_different_commit() {
    let repo = FixtureRepo::new();
    repo.write_workspace("19.2.0");
    repo.commit_all("seed workspace");
    let release_sha = repo.git_stdout(&["rev-parse", "HEAD"]);
    repo.create_release_branch("release/19.2");
    repo.write("src/lib.rs", "// later commit with reused rc tag\n");
    repo.commit_all("advance main after release branch");
    let tag_sha = repo.git_stdout(&["rev-parse", "HEAD"]);
    repo.git(&["tag", "v19.2.0-rc.1"]);
    repo.git(&["push", "origin", "v19.2.0-rc.1"]);
    assert_ne!(release_sha, tag_sha);

    let output = repo.run_release(&[
        "rc",
        "--version",
        "19.2.0",
        "--rc-number",
        "1",
        "--branch",
        "release/19.2",
    ]);

    assert_failure(&output);
    assert!(stderr(&output).contains(&format!(
        "RC tag v19.2.0-rc.1 already points at {tag_sha}, not {release_sha}"
    )));
    assert!(!stdout(&output).contains("Dry command: gh workflow run"));
}

#[test]
fn rc_dispatch_requires_gh_cli_before_workflow_run() {
    let repo = FixtureRepo::new();
    repo.write_workspace("19.2.0");
    repo.commit_all("seed workspace");
    repo.create_release_branch("release/19.2");

    let output = repo
        .release_command(&[
            "rc",
            "--version",
            "19.2.0",
            "--rc-number",
            "1",
            "--branch",
            "release/19.2",
            "--dispatch",
        ])
        .env("WAVECRATE_RELEASE_GH_BIN", repo.path().join("missing-gh"))
        .output()
        .expect("run release wrapper");

    assert_failure(&output);
    assert!(stderr(&output).contains("gh CLI not found"));
}

#[test]
fn rc_dry_run_preserves_release_notes_input() {
    let repo = FixtureRepo::new();
    repo.write_workspace("19.2.0");
    repo.commit_all("seed workspace");
    repo.create_release_branch("release/19.2");

    let output = repo.run_release(&[
        "rc",
        "--version",
        "19.2.0",
        "--rc-number",
        "1",
        "--branch",
        "release/19.2",
        "--release-notes",
        "RC notes with spaces",
    ]);

    assert_success(&output);
    let stdout = stdout(&output);
    assert!(stdout.contains("Dry command: gh workflow run release-rc.yml"));
    assert!(stdout.contains("--repo PORTALSURFER/wavecrate"));
    assert!(stdout.contains("-f release_notes=RC\\ notes\\ with\\ spaces"));
}

#[test]
fn rc_rejects_release_branch_with_mismatched_manifest_version() {
    let repo = FixtureRepo::new();
    repo.write_workspace("0.19.1");
    repo.commit_all("seed workspace");
    repo.create_release_branch("release/19.2");

    let output = repo.run_release(&[
        "rc",
        "--version",
        "19.2.0",
        "--rc-number",
        "1",
        "--branch",
        "release/19.2",
    ]);

    assert_failure(&output);
    assert!(stderr(&output).contains("Cargo.toml version 0.19.1"));
}

#[test]
fn stable_command_fails_closed_before_resolving_or_dispatching() {
    let repo = FixtureRepo::new();
    repo.write_workspace("19.2.0");
    repo.commit_all("seed workspace");

    let output = repo.run_release(&[
        "stable",
        "--version",
        "19.2.0",
        "--branch",
        "release/19.2",
        "--dispatch",
    ]);

    assert_failure(&output);
    assert!(stderr(&output).contains(
        "stable releases are temporarily disabled; continue publishing release candidates"
    ));
    assert!(!stdout(&output).contains("git fetch"));
    assert!(!stdout(&output).contains("gh workflow run"));
}

struct FixtureRepo {
    _temp: TempDir,
    worktree: PathBuf,
    origin: PathBuf,
}

impl FixtureRepo {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("create release wrapper fixture repo");
        let worktree = temp.path().join("worktree");
        let origin = temp.path().join("origin.git");
        fs::create_dir_all(&worktree).expect("create fixture worktree");
        assert_success(&run_git(
            None,
            &["init", "--bare", origin.to_str().expect("origin path")],
        ));
        let repo = Self {
            _temp: temp,
            worktree,
            origin,
        };
        repo.git(&["init", "-b", "main"]);
        repo.git(&["config", "user.email", "release-tests@example.invalid"]);
        repo.git(&["config", "user.name", "Release Tests"]);
        repo.git(&[
            "remote",
            "add",
            "origin",
            repo.origin.to_str().expect("origin path"),
        ]);
        repo
    }

    fn path(&self) -> &Path {
        &self.worktree
    }

    fn write_workspace(&self, wavecrate_version: &str) {
        self.write(
            "Cargo.toml",
            &format!(
                r#"[workspace]
members = ["."]
resolver = "3"

[package]
name = "wavecrate"
version = "{wavecrate_version}"
edition = "2024"
"#
            ),
        );
        self.write("Cargo.lock", "# fixture lockfile\n");
        self.write("radiant-dependency.toml", "repository = \"https://github.com/PORTALSURFER/radiant.git\"\nrevision = \"82043f13510fb60c4bf0114159d38e43e7df8987\"\npath = \"../radiant\"\n");
        self.write("src/lib.rs", "");
        self.write(".github/workflows/release-train-prepare.yml", "");
        self.write(".github/workflows/release-rc.yml", "");
        self.write(
            "scripts/radiant.sh",
            "#!/usr/bin/env bash\nset -euo pipefail\nmkdir -p \"$(dirname \"$PWD\")/radiant\"\nprintf '[radiant] fixture sibling provisioned\\n'\n",
        );
        make_executable(self.path().join("scripts/radiant.sh"));
        self.write(
            "scripts/internal/release/prepare_release_train.py",
            "#!/usr/bin/env bash\nversion=\"$(awk '/^\\[package\\]$/ { in_package = 1; next } in_package && /^\\[/ { exit } in_package && /^[[:space:]]*version[[:space:]]*=/ { gsub(/\\\"/, \"\", $3); print $3; exit }' Cargo.toml)\"\nprintf 'prepare-helper'\nfor arg in \"$@\"; do printf ' [%s]' \"$arg\"; done\nprintf ' [cwd-version=%s]\\n' \"$version\"\n",
        );
        make_executable(
            self.path()
                .join("scripts/internal/release/prepare_release_train.py"),
        );
    }

    fn create_release_branch(&self, branch: &str) {
        self.git(&["switch", "-c", branch]);
        self.push_branch(branch);
        self.git(&["switch", "main"]);
    }

    fn push_branch(&self, branch: &str) {
        self.git(&["push", "-u", "origin", branch]);
    }

    fn commit_all(&self, message: &str) {
        self.git(&["add", "."]);
        self.git(&["commit", "-m", message]);
    }

    fn run_release(&self, args: &[&str]) -> Output {
        self.release_command(args)
            .output()
            .expect("run release wrapper")
    }

    fn release_command(&self, args: &[&str]) -> Command {
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/release.sh");
        let mut command = Command::new("bash");
        command.arg(script).args(args).current_dir(self.path());
        command
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.path().join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixture parent");
        }
        fs::write(path, contents).expect("write fixture file");
    }

    fn read(&self, relative: &str) -> String {
        fs::read_to_string(self.path().join(relative)).expect("read fixture file")
    }

    fn write_fake_gh(&self) -> PathBuf {
        let path = self
            .path()
            .parent()
            .expect("fixture temp dir")
            .join("fake-gh");
        fs::write(
            &path,
            "#!/usr/bin/env bash\nif [[ \"$1\" == \"auth\" && \"$2\" == \"status\" ]]; then exit 0; fi\nprintf 'fake-gh'\nfor arg in \"$@\"; do printf ' [%s]' \"$arg\"; done\nprintf '\\n'\n",
        )
        .expect("write fake gh");
        make_executable(path.clone());
        path
    }

    fn git(&self, args: &[&str]) {
        let output = run_git(Some(self.path()), args);
        assert_success(&output);
    }

    fn git_stdout(&self, args: &[&str]) -> String {
        let output = run_git(Some(self.path()), args);
        assert_success(&output);
        stdout(&output).trim().to_string()
    }
}

fn run_git(cwd: Option<&Path>, args: &[&str]) -> Output {
    let mut command = Command::new("git");
    command.args(args);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    command.output().expect("run git")
}

#[cfg(unix)]
fn make_executable(path: PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(&path).expect("helper metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("chmod helper");
}

#[cfg(not(unix))]
fn make_executable(_path: PathBuf) {
    // The Bash release wrapper and its fixture helper are supported on macOS/Linux.
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "command failed\nstdout:\n{}\nstderr:\n{}",
        stdout(output),
        stderr(output)
    );
}

fn assert_failure(output: &Output) {
    assert!(
        !output.status.success(),
        "command should have failed\nstdout:\n{}\nstderr:\n{}",
        stdout(output),
        stderr(output)
    );
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
