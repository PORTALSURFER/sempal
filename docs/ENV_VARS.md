# Environment Variables

This document lists runtime and build-related environment variables used by
Wavecrate. Unless noted otherwise, variables are optional.

## Conventions

- Booleans: prefer setting `1` for “enabled” unless the variable explicitly
  documents additional accepted values.
- Paths: prefer absolute paths. Relative paths are interpreted by the code that
  consumes them (often relative to the current working directory).

## Build and release

- `CPAL_ASIO_DIR`
Path to the Steinberg ASIO SDK root directory (expected to contain `host/` and
`common/`). Required only when building with ASIO support on Windows.

- `WAVECRATE_CARGO_BIN`
Overrides the build tool used by `scripts/internal/release/build_release_zip.sh` (defaults to
`cargo`).

- `WAVECRATE_CHECKSUMS_ED25519_KEY`
CI secret used by the release workflows to sign release checksum files. This is
expected to be an Ed25519 private key in PEM form that OpenSSL can use for
`pkeyutl -sign`. The release workflows also accept the legacy
`SEMPAL_CHECKSUMS_ED25519_KEY` secret name as a compatibility fallback.

- `WAVECRATE_RELEASE_VERSION`
Full semver release identity embedded into a packaged binary. Examples:
`0.19.1-nightly.20260701+abc1234`, `0.19.1-rc.1`, `0.19.1`.

- `WAVECRATE_RELEASE_CHANNEL`
Embedded release channel: `nightly`, `rc`, or `stable`.

- `WAVECRATE_RELEASE_TARGET_VERSION`
Stable target version for the release train, such as `0.19.1`.

- `WAVECRATE_RELEASE_BUILD_DATE`
UTC build date embedded into the binary in `YYYY-MM-DD` form.

- `WAVECRATE_DISABLE_SCCACHE`
When set to `1`, repo Cargo helper scripts force direct `rustc` and clear any
inherited `RUSTC_WRAPPER=sccache` for the current script run. This is useful
when comparing uncached timings or debugging wrapper-related issues. Default:
unset.

- `WAVECRATE_ENABLE_SCCACHE`
When set to `1`, repo Cargo helper scripts may use `sccache` if it is installed
and its wrapper probe passes. Default: unset, so repo scripts use direct
`rustc` and do not probe `sccache`.

## macOS developer validation

These overrides are intended for watchdog regression tests and deliberate
diagnosis. Normal `scripts/agent.sh` and `scripts/ci.sh` runs should use their
defaults.

- `WAVECRATE_VALIDATION_IDLE_SECONDS`
Seconds without an owned process-tree change or aggregate CPU-time increase
before diagnostics are captured. Default: `300`.

- `WAVECRATE_VALIDATION_DIAGNOSTIC_GRACE_SECONDS`
Additional no-progress interval after diagnostics before the owned process
group is terminated with exit 124. Default: `120`.

- `WAVECRATE_VALIDATION_DIAGNOSTIC_COLLECTION_SECONDS`
Global budget shared by toolchain metadata and every compiler sample in one
diagnostic bundle. The recovery grace starts only after this budgeted collection
finishes. Default: `30`.

- `WAVECRATE_VALIDATION_TERM_GRACE_SECONDS`
Grace after `SIGTERM` before remaining owned processes receive `SIGKILL`.
Default: `10`.

- `WAVECRATE_VALIDATION_DIAGNOSTICS_DIR`
Overrides the diagnostic bundle root. Default:
`target/validation-diagnostics`.

- `WAVECRATE_VALIDATION_TARGET_ROOT`
Overrides the root containing toolchain/lockfile-specific Cargo targets.
Default: `target/agent-validation`. Supported macOS validation overrides an
inherited `CARGO_TARGET_DIR`; use this variable to customize its cache root.

- `WAVECRATE_VALIDATION_LEASE_WAIT_SECONDS`
Maximum time a supported macOS validation command waits for the keyed Cargo
target's live lease owner. Timeout exits 124 without terminating the owner or
removing its lease. Default: `480`.

- `WAVECRATE_VALIDATION_MAX_DEPS_METADATA_BYTES`
Maximum macOS directory metadata size allowed for a validation target's
`debug/deps` before that target is quarantined. Default: `8388608`.

### Release upload secrets

Wavecrate has three active public release workflows and one preserved inactive
stable workflow:

- `.github/workflows/release-train-prepare.yml`
  - manual release-train branch/version preparation for `release/X.Y`
- `.github/workflows/release-build.yml`
  - automatic and manual nightly builds from `main`
- `.github/workflows/release-rc.yml`
  - manual release-candidate builds from `release/X.Y`
- `.github/workflows/release-stable.yml.disabled`
  - preserved stable implementation; the non-YAML suffix keeps it unavailable
    to GitHub Actions while Wavecrate remains in RC stabilization

### Release lifecycle policy

Development is continuous on `main` through ordinary PRs. Publishing the first
RC for `X.Y.Z` signals that the project has entered stabilization for that
version; it does not freeze `main`, stop PRs, or authorize stable publication.
From that point, release-train work should focus on stability: bug fixes,
regressions, reliability, performance, packaging, and release blockers. New
features wait until stabilization ends unless the release scope is explicitly
changed.

The remote `release/X.Y` branch is the persistent coordination ref for the
candidate being tested. Preserve it when an RC preparation PR merges. If fixes
land on `main` after RC1, update `release/X.Y` from the current `main` commit at
the same target version and publish RC2 (and later RCs as needed). The stable
workflow deliberately requires the latest `vX.Y.Z-rc.N` tag and
`release/X.Y` to point at the same commit, so stable cannot silently include
changes that were never published as an RC.

Stable publication is temporarily disabled. The workflow implementation and
promotion checks remain in the repository, but GitHub does not register the
`.yml.disabled` file and `scripts/release.sh stable` fails closed before any
release resolution or dispatch. Re-enabling stable publication requires a
dedicated reviewed change; an RC approval or ordinary merge approval is not
sufficient.

Nightly runs publish rolling `nightly` builds for Windows x86_64 plus macOS
x86_64/aarch64 assets from the current `main` commit. The schedule is
`19:30 UTC` (evening in Europe/Amsterdam), and `workflow_dispatch` provides a
manual "force a nightly now" button with the same build/upload path.

Before the first RC for a train, run the release-train prep workflow or the
equivalent local command. The prep lane validates an `X.Y.Z` stable target
version, derives `release/X.Y`, checks out the requested source commit, updates
Wavecrate release-package manifests and `Cargo.lock`, rejects stale prerelease
package versions such as `alpha` or `beta`, and runs focused release contract
validation. The workflow defaults to dry-run mode; it only pushes or updates the
`release/X.Y` branch when `push_branch` is explicitly set to `true`.

The repo-root Bash orchestrator is the preferred operator entrypoint on
macOS/Linux. It must be run from a clean Wavecrate repo root, fetches/prunes
release refs before resolving commits, derives the target stable version from
the package version at the resolved source ref, and keeps public publishing
behind explicit operator intent:

```bash
scripts/release.sh prepare --bump minor --source-ref main --dry-run
scripts/release.sh prepare --bump minor --source-ref main --push
scripts/release.sh prepare --bump patch --source-ref release/0.19 --dry-run
scripts/release.sh rc --version 0.20.0 --rc-number 1 --branch release/0.20 --dispatch
```

`prepare --bump major` bumps `X.Y.Z` to `(X+1).0.0`; `prepare --bump minor`
bumps to `X.(Y+1).0`; `prepare --bump patch` bumps to `X.Y.(Z+1)` on the same
`release/X.Y` train. Without `--dispatch`, `rc` validates inputs and prints the
exact `gh workflow run ...` command instead of publishing. Stable commands fail
closed while publication is disabled. With `--dispatch`, the RC path requires
an authenticated GitHub CLI (`gh`) and invokes the active workflow with its
native input names. When `origin` is a GitHub
remote, it must match `WAVECRATE_GITHUB_REPO` so local ref resolution and
workflow dispatch target the same repository. `prepare --dispatch` dispatches
the workflow file from `--workflow-ref` (default `main`) while passing the
resolved source commit SHA as the workflow `source_ref` input.

Local equivalent:

```bash
scripts/internal/release/prepare_release_train.py \
  --version 0.19.1 \
  --source-ref main \
  --push
```

Use the workflow dry run first when preparing a new train, then re-run with
`push_branch=true` once the source commit and version are correct. After prep,
run the RC workflow against the prepared `release/X.Y` branch. After later
stability PRs merge to `main`, update the same release branch at the unchanged
target version with:

```bash
scripts/internal/release/prepare_release_train.py \
  --version X.Y.Z \
  --source-ref main \
  --push
```

Publish the next RC from that updated branch. Passing review makes the latest
RC eligible for promotion; it does not dispatch stable. Stable promotion runs
against the same release branch only after a separate explicit stable-release
instruction.

The release workflows keep channel policy in YAML but share operational helpers
under `scripts/internal/release/`:

- `provision_radiant_sibling.sh` provisions the pinned standalone Radiant sibling.
- `emit_rust_toolchain_channel.py` emits the pinned Rust toolchain channel for
  `dtolnay/rust-toolchain`.
- `setup_windows_asio_sdk.ps1` prepares the Windows ASIO SDK path.
- `build_release_artifact.sh` wraps package creation and per-platform checksum
  entry naming.
- `assemble_release_files.sh` copies built zips and assembles the channel
  checksum file.
- `prepare_github_release_body.py` writes the bounded body used for RC/stable
  GitHub releases. The canonical `release-log.md` remains attached to the
  release and is still used for the PortalSurfer per-release and full changelog.
- `sign_release_checksums.sh` signs and optionally verifies checksum files.
- `validate_promoted_rc_release.py` verifies that stable promotion is backed by
  a complete RC GitHub prerelease before stable builds start.

### Standalone Radiant checkout

Wavecrate resolves Radiant from the sibling path recorded in
`radiant-dependency.toml` (`../radiant` by default). Run
`scripts/bootstrap.sh` (or the PowerShell equivalent) after a fresh clone; it
provisions the recorded revision when the sibling is absent and never resets an
existing dirty checkout. Set `WAVECRATE_RADIANT_DIR` to use another local
checkout during development. CI/release helpers use
`RADIANT_REPOSITORY_DEPLOY_KEY` (mapped from the existing
`RADIANT_SUBMODULE_DEPLOY_KEY` secret) and fail closed unless the sibling's
remote, manifest, clean state, and HEAD match the recorded revision.
- `verify_published_release.py` downloads public GitHub or PortalSurfer release
  assets after publication, verifies checksum signatures and zip hashes, and
  inspects each `update-manifest.json` for the expected channel, version,
  commit, build date, target, platform, and architecture. PortalSurfer downloads
  authenticate with the release upload token so integrity verification is not
  counted as a user download.
- `publish_portalsurfer_release.sh` stages release files privately in
  PortalSurfer, assembles the full Wavecrate changelog, commits the staged
  files, generated release log, catalog entry, and full changelog in one
  PortalSurfer request, then verifies the public catalog, per-release
  changelog, and full changelog.
- `prune_github_release_assets.sh` removes stale assets from a rolling GitHub
  release during promotion.
- `write_release_step_summary.sh` writes secret-safe GitHub Actions summaries
  with public release metadata, artifact filenames, file hashes, checksum file
  names, PortalSurfer catalog/build ids, and verification status.

RC runs require `version`, `rc_number`, and `branch` inputs. The branch must be
`release/X.Y` for the requested `X.Y.Z` version, and the package manifest must
already carry that target version. RC releases are published as GitHub
pre-releases tagged `vX.Y.Z-rc.N` and as PortalSurfer catalog entries with build
ids like `wavecrate-X.Y.Z-rc.N`.

Stable runs require `version` and `branch` inputs. The branch must be
`release/X.Y`, the package manifest must match `X.Y.Z`, and the latest
`vX.Y.Z-rc.N` tag must point at the same commit being promoted. Stable
promotion also validates that the promoted RC exists as a GitHub prerelease with
the expected platform zips from `release_contract.toml`, a checksum file,
checksum signature, downloadable assets whose hashes match the checksum file,
and a release-bound `Wavecrate X.Y.Z-rc.N` log body. Stable releases are
published as normal GitHub releases tagged `vX.Y.Z` and as PortalSurfer catalog
entries with build ids like `wavecrate-X.Y.Z`. RC and stable workflows verify the
published GitHub release assets, then PortalSurfer publication stages the
generated release zips, checksum file, and checksum signature, commits those
files together with the release log and full Wavecrate changelog, verifies the
catalog entry, per-release changelog, and full changelog round trips, and
finally downloads the public PortalSurfer assets back to verify their checksums,
signature, and embedded manifests.

The nightly workflow stages the generated zips and checksum files through the
PortalSurfer Wavecrate release-upload API, then commits the complete release
files, Markdown release log, catalog entry, and full Wavecrate changelog before
moving the rolling GitHub `nightly` identity. Each PortalSurfer upload uses a path-safe
`wavecrate-nightly-b<build>-<short-sha>` build id so the website can show a
distinct nightly entry instead of stacking every nightly under one changelog
group, while the release log and package manifest keep the full SemVer nightly
version. The full changelog is assembled before commit by prepending the
generated release-bound log to the existing site-wide changelog. After commit,
the workflow verifies that the public per-release log and full changelog bodies
match the generated files.
Only after PortalSurfer publication has been verified does the workflow refresh
the rolling GitHub `nightly` release assets, publish the immutable nightly
version tag named like `0.19.1-nightly.20260701+abc1234`, move the rolling
`nightly` tag, and update the rolling release metadata. Scheduled and manual
nightly runs do not cancel an in-flight run because tag movement is the final
public identity transition. If a run fails before the PortalSurfer commit step,
only private staged files may remain and public readers keep seeing the previous
catalog/changelog state. If a run fails after the commit step but before GitHub
promotion, rerun the workflow for the same `main` commit; the same immutable
PortalSurfer build id is recommitted idempotently when already-published files
match by name, size, and SHA-256, then reverified before the rolling GitHub
identity moves.
The nightly workflow verifies both the PortalSurfer catalog downloads and the
rolling GitHub release assets after their respective publication points.
The maintenance step refuses to preserve a full changelog whose historical
sections are not already release-bound markdown logs.
GitHub Actions does not need SSH access or write access to the PortalSurfer
frontend repository.
This staged commit flow requires PortalSurfer server support for
`/wavecrate/api/v1/release-uploads/<build-id>/staging/files/<filename>` and
`/wavecrate/api/v1/release-uploads/<build-id>/commit`; older servers fail closed
instead of falling back to direct public mutation.

Release jobs use explicit GitHub Actions timeout budgets so stalled tests,
packaging, upload, verification, or release-train preparation fail with bounded
operator feedback instead of consuming the platform default timeout. Build jobs
are intentionally the most generous because macOS signing and notarization can
be slow; publish jobs are shorter because they should only assemble, upload,
commit, and verify already-built artifacts. Each publish path appends a
`GITHUB_STEP_SUMMARY` section with the resolved commit, version, build id,
GitHub release URL or tag, PortalSurfer catalog URL/build id, artifact names,
checksums, and final verification status. The summary helper only receives
public release metadata and local artifact paths; upload tokens, checksum
signing keys, Apple credentials, and private key material must never be passed
to it.

PortalSurfer upload and catalog fetches are bounded with curl connection and
total transfer timeouts. By default,
`PORTALSURFER_RELEASE_CONNECT_TIMEOUT_SECONDS=10` and
`PORTALSURFER_RELEASE_MAX_TIME_SECONDS=300` apply to each staged file upload,
commit request, and verification fetch. Override those only for an explicit
operator recovery run where large artifacts or a known network incident need a
larger transfer window.

To rerun the GitHub verifier for a published stable release, use the resolved
release metadata from the workflow run:

```bash
scripts/internal/release/verify_published_release.py \
  --surface github \
  --channel stable \
  --version 0.19.1 \
  --target-version 0.19.1 \
  --commit <target-sha> \
  --build-date <yyyy-mm-dd> \
  --tag v0.19.1 \
  --repo PORTALSURFER/wavecrate \
  --checksum-public-key 8Z7dQJBRMbxCFkFMeBYa1FMSWOUm6nePFgoK5c43jT4=
```

To rerun the PortalSurfer verifier for the same release:

```bash
export PORTALSURFER_RELEASE_UPLOAD_TOKEN=<release-upload-token>
scripts/internal/release/verify_published_release.py \
  --surface portalsurfer \
  --channel stable \
  --version 0.19.1 \
  --target-version 0.19.1 \
  --commit <target-sha> \
  --build-date <yyyy-mm-dd> \
  --portal-catalog-url https://portalsurfer.org/wavecrate/api/v1/releases \
  --portal-build-id wavecrate-0.19.1 \
  --build-number <build-number> \
  --checksum-public-key 8Z7dQJBRMbxCFkFMeBYa1FMSWOUm6nePFgoK5c43jT4=
```

- `PORTALSURFER_RELEASE_UPLOAD_TOKEN`
Bearer token sent by the workflow to the PortalSurfer upload endpoint. The
post-publish verifier sends only a SHA-256 proof derived from this token, so its
downloads still stream and validate the public artifact bytes without exposing
upload credentials or incrementing the public user-download total. Store the
matching `WAVECRATE_RELEASE_UPLOAD_TOKEN_SHA256` on the PortalSurfer server when
possible, so the server does not keep the raw token.

- `PORTALSURFER_RELEASE_UPLOAD_URL`
Optional upload endpoint. Defaults to
`https://portalsurfer.org/wavecrate/api/v1/release-uploads` and must end with
`/release-uploads`.

### macOS release signing secrets

macOS zips are packaged as `Wavecrate.app` bundles. On GitHub Actions, the
release workflows sign them with Developer ID, submit them to Apple
notarization, staples the ticket, and only then uploads the zip to GitHub and
PortalSurfer.

- `APPLE_DEVELOPER_ID_APPLICATION_CERT_BASE64`
Base64 text for the exported Developer ID Application `.p12` certificate.

- `APPLE_DEVELOPER_ID_APPLICATION_CERT_PASSWORD`
Password used when exporting the Developer ID Application `.p12`.

- `APPLE_NOTARY_KEY_BASE64`
Base64 text for the App Store Connect API key file, usually
`AuthKey_<key-id>.p8`.

- `APPLE_NOTARY_KEY_ID`
Key ID shown for the App Store Connect API key.

- `APPLE_NOTARY_ISSUER_ID`
Issuer ID shown in App Store Connect API key settings.

- `APPLE_CODESIGN_IDENTITY`
Optional exact signing identity. When unset, the signing script uses the first
imported `Developer ID Application:` identity from the temporary CI keychain.

Local `build_release_zip.sh` runs remain unsigned unless
`WAVECRATE_MACOS_SIGNING=1` is set explicitly, so development builds do not need
Apple credentials.

## Agent workflow

- `WAVECRATE_SKIP_AGENT_PREFLIGHT_HOOK_INSTALL`
Set to `1` to skip automatic preflight hook installation in bootstrap.
Hook execution for existing hook setups is separately controlled by
`WAVECRATE_SKIP_AGENT_PREFLIGHT_HOOK=1`.

- `WAVECRATE_DEAD_SWEEP_STRICT`
When set to `1`/`true`/`yes`/`on`, `scripts/check.sh dead-deps`
exits non-zero when it detects unused dependency/dead-code findings. Default:
advisory mode (`0`).

- `WAVECRATE_DEAD_SWEEP_RUN_UDEPS`
When set to `1`/`true`/`yes`/`on`,
`scripts/check.sh dead-deps` also runs `cargo udeps` using
`WAVECRATE_DEAD_SWEEP_UDEPS_TOOLCHAIN` (or its default).

- `WAVECRATE_DEAD_SWEEP_INSTALL_MISSING`
When set to `1`/`true`/`yes`/`on`,
`scripts/check.sh dead-deps` attempts to install missing
`cargo-machete`/`cargo-udeps` tools before running checks.

- `WAVECRATE_DEAD_SWEEP_UDEPS_TOOLCHAIN`
Rust toolchain string passed to `cargo +<toolchain> udeps` by
`scripts/check.sh dead-deps`. Default: `nightly`.

- `WAVECRATE_DEAD_SWEEP_REPORT_PATH`
Optional output path for `scripts/check.sh dead-deps` to write
the full advisory sweep report.

## Paths and directories

- `WAVECRATE_CONFIG_HOME`
Overrides the OS config base directory used for app files. Wavecrate still creates
and uses a `.wavecrate` directory under this base.

Example: `WAVECRATE_CONFIG_HOME=/tmp` causes logs to be written under
`/tmp/.wavecrate/logs` on Unix-like platforms.

- `WAVECRATE_CONFIG_PROFILE`
Selects one named non-live persistence profile under the resolved app root.
`WAVECRATE_CONFIG_PROFILE=gui-test` writes config, logs, and `library.db` under
`<config-base>/.wavecrate/profiles/gui-test/`. Set `WAVECRATE_CONFIG_PROFILE=live`
to force the real live app root explicitly. Test executables default to the
named `automated-tests` profile when neither `WAVECRATE_CONFIG_PROFILE` nor
`WAVECRATE_CONFIG_HOME` is set.
Use `sandbox` for manual QA/sandboxed runs and `automated-tests` for automated
validation. `scripts/run.* sandbox` now sets `WAVECRATE_CONFIG_PROFILE=sandbox`
explicitly so sandbox runs are first-class non-live profiles, not just
alternate config-home directories.

### Persistence profile quick reference

- `live`
  - app root: `<config-base>/.wavecrate/`
  - use for: normal user-facing startup and release/manual runs
- `sandbox`
  - app root: `<config-base>/.wavecrate/profiles/sandbox/`
  - use for: deliberate manual QA runs that must not touch the live profile
- `automated-tests`
  - app root: `<config-base>/.wavecrate/profiles/automated-tests/`
  - use for: tests, GUI contract runs, and scripted validation

The practical rule is simple: automated validation should prove behavior
through isolated fixture roots and the `automated-tests` profile, while a plain
`cargo run --release` with no overrides should still open the intended live
source registry afterward.

### What writes where (important for safe local runs)

- App config + logs:
  - Live runs write under `<config-base>/.wavecrate/`.
  - Named non-live profiles write under `<config-base>/.wavecrate/profiles/<name>/`.
  - `scripts/run.* sandbox` sets both `WAVECRATE_CONFIG_HOME` and `WAVECRATE_CONFIG_PROFILE=sandbox` so the run does **not** touch your real user profile config/log directories.
- Per-source-folder database side effect:
  - If you point Wavecrate at a folder, it may create or update a `.wavecrate.db` in that source folder.
  - `WAVECRATE_CONFIG_HOME` does **not** relocate these per-folder DB files.
  - `scripts/run.sh sandbox` and `scripts/run.ps1 sandbox` set
    `WAVECRATE_SOURCE_DB_READ_ONLY=1` by default, so source DB writes are blocked
    unless `--write-db` is explicitly passed.

### Source database behavior

- `WAVECRATE_SOURCE_DB_READ_ONLY`
  - When `1`/`true`/`yes`/`on`, `SourceDatabase::open` uses read-only access by
    default and requires `<source>/.wavecrate.db` to already exist. Legacy `<source>/.wavecrate_samples.db` files are still readable for compatibility.
  - When unset/default, normal write-capable behavior applies.

- `RADIANT_NATIVE_FONT_PATH`
Optional path to a `.ttf` font used by the UI projection text renderer when
system font discovery fails.

- `RADIANT_AUTOMATION_TARGET_EXPORT`
Optional path where Radiant's native runtime writes the latest flattened
automation target snapshot as JSON after surface refreshes.

- `RADIANT_AUTOMATION_TARGET_EXPORT_PRETTY`
When set to `1`/`true`/`yes`/`on`, Radiant writes the target snapshot as
pretty-printed JSON.

- `WAVECRATE_GUI_TEST_MODE`
Enables deterministic GUI test mode for the main app binary.

- `WAVECRATE_GUI_TEST_FIXTURE`
Selects the GUI fixture used in GUI test mode. The default is
`isolated-startup`, which captures the production native app composition on a
temporary config base using the dedicated non-live `automated-tests` profile.
Use `live` only when you explicitly want GUI validation to exercise the real
persisted startup profile. The legacy `default` tag is accepted only as an alias
for `isolated-startup`. The versioned `empty`, `small-multi-source`, and
`large-source` tags use production-native startup with deterministic disposable
source libraries; `small-multi-source` is the routine source-system QA fixture
and `large-source` is explicit/opt-in. Older names such as `browser`,
`waveform`, and `map` remain retained-controller compatibility tools and are not
product-startup certification.

## Logging

- `--log` / `-log`
Preferred Wavecrate-owned opt-in switch for richer per-launch debug diagnostics in
release or manual debugging runs.

Semantics:
- when absent, launch logs keep the normal always-on baseline (`info` level plus
  warnings/errors) and richer action/DB reconstruction events stay disabled
- when present and `RUST_LOG` is unset, Wavecrate installs the default filter
  `wavecrate=debug,perf=info,info` so Wavecrate-owned debug events and opt-in
  performance events are visible without turning third-party crates up to `debug`
- when present and `RUST_LOG` is also set, `RUST_LOG` still owns the final
  filter expression, but Wavecrate still treats the run as debug mode for
  diagnostics that explicitly gate on the Wavecrate-owned debug feature
- on Windows release builds, the same flag also preserves the existing console
  attach behavior for interactive debugging

Structured debug event contract for later instrumentation:
- action events must use stable `action`, `pane`, `source`, `outcome`,
  `elapsed_ms`, and `error` fields
- DB events must use stable `operation`, `source`, `outcome`, `elapsed_ms`, and
  `error` fields
- use concise, sanitized identifiers; prefer stable action/operation names over
  free-form prose

Never log:
- secrets, tokens, auth headers, credentials, encryption keys, or session data
- raw SQL with interpolated private values
- large unredacted user-authored payloads or other sensitive content that is not
  required to diagnose the seam

- `WAVECRATE_DEBUG_LOGGING`
Optional compatibility env override for non-interactive launch environments.
Accepted truthy values: `1`, `true`, `yes`, `on`. Accepted falsy values: `0`,
`false`, `no`, `off`.

- `RUST_LOG`
Controls logging verbosity via `tracing_subscriber::EnvFilter`. When unset,
Wavecrate defaults to `info`, or to `wavecrate=debug,perf=info,info` when the
Wavecrate-owned debug feature is enabled via `--log`, `-log`, or
`WAVECRATE_DEBUG_LOGGING`.
`RUST_LOG` overrides that default filter when it is set.

### Debug log workflow

- local release debugging:
  - live profile:
    `cargo run --release -- --log`
  - sandbox/manual QA profile:
    `powershell -ExecutionPolicy Bypass -File scripts/run.ps1 sandbox -- --log`
- newest log retrieval:
  - live profile:
    `powershell -ExecutionPolicy Bypass -File scripts/run.ps1 logs`
  - sandbox/manual QA profile:
    `powershell -ExecutionPolicy Bypass -File scripts/run.ps1 logs -Sandbox`
- bug-report collection:
  - live profile:
    `powershell -ExecutionPolicy Bypass -File scripts/run.ps1 bug-bundle`
  - sandbox/manual QA profile:
    `powershell -ExecutionPolicy Bypass -File scripts/run.ps1 bug-bundle -Sandbox`
- what the log captures automatically:
  - startup metadata, warnings/errors, and the structured Wavecrate-owned debug
    action/DB events emitted during the reproduced run
- what still requires manual reproduction:
  - you still need to rerun the failing scenario with `--log`; the file records
    what happened during that run, but it does not retroactively reconstruct an
    earlier session that was not logged

## Headless audio

- `ALSA_CONFIG_PATH`
When set, ALSA uses the provided config file path instead of system defaults.
On headless Linux runs, `scripts/ci.sh local` and `scripts/perf.sh guard`
set this automatically to `scripts/internal/alsa_headless.conf` unless already defined.
This routes default playback probing to a dummy sink to reduce `ALSA lib`
warning noise in test/bench logs. This is developer validation tooling for
headless Linux CI/agent/perf hosts only and does not describe shipped Linux app
support.

## UI and runtime profiling

- `WAVECRATE_HOTPATH_TELEMETRY`
Enables opt-in structured hot-path telemetry logs for latency-sensitive paths.
Current coverage includes browser search workers, audio loader stages, cached
playback startup, persistent waveform-cache reads, Reson decoder open/probe/seek,
async decode prefill, audio stream append handoff, and starmap drag-audition
hit testing/queue/playback routing. Use with `--log` or an equivalent
`RUST_LOG` filter so `perf::audio_start`, `perf::starmap_drag`, and
`perf::hotpath` events are written to the launch log. Normal `--log` runs also
emit warning-level `starmap_audition.slow_event` and
`starmap_audition.slow_duration` records when a starmap audition hot phase
crosses the slow-path threshold, so obvious drag stalls are still visible
without full event telemetry. For starmap playback
investigations, inspect `stage`, `outcome`, `hits_queued`, `hits_started`,
`ready_started`, `ready_unavailable`, `validation_queued`,
`runtime_started`, `runtime_failed`, `runtime_cancelled`,
`avg_widget_hit_test_ms`, `avg_ready_source_ms`, `avg_runtime_start_ms`, and
`avg_start_total_ms`. `perf::audio_start` runtime events also include
`origin` and `source_kind`, which distinguish starmap drag handoffs from
ordinary browser or waveform playback. Accepted values: `1`, `true`, `on`,
`yes`.
After a dense starmap/list/keyboard smoke run, summarize the latest log with:
`bash scripts/run.sh logs > /tmp/wavecrate-latest.log && python3 scripts/internal/perf/summarize_hotpath_telemetry.py /tmp/wavecrate-latest.log`.
The summary groups fast-audition decisions, starmap queue/hit/playback
counters, preview warm batches, slow UI frame dispatches, and missing telemetry
diagnostics so remaining missed plays can be tied to a specific stage.

- `RESON_PLAYBACK_TELEMETRY`
Enables only Reson playback-engine hot-path logs, including decoder setup,
seeking, async-source prefill, and output-stream append. `WAVECRATE_HOTPATH_TELEMETRY`
also enables these Reson logs. Accepted values: `1`, `true`, `on`, `yes`.

- `WAVECRATE_BROWSER_SEARCH_OFFLOAD_THRESHOLD`
Wav-entry threshold used by browser search to switch from synchronous layout
path recompute to async search jobs. Default: `5000`.

- `RADIANT_NATIVE_RENDER_PROFILE`
Enables Radiant native render profiling logs when the build enables the
`gui-performance` cargo feature. Accepted values include `1`, `true`, `on`,
and `yes` (case variants are accepted in the current implementation).

- `RADIANT_NATIVE_STARTUP_PROFILE`
Enables one-line Radiant native startup timing summaries after first present
plus deferred model refresh. Accepted values: `1`, `true`, `on`, `yes`
(case-insensitive). Intended for startup-focused profiling workflows.

- `RADIANT_NATIVE_INCREMENTAL_FRAME_PIPELINE`
Enables bridge-driven dirty-segment gating for native static frame rebuilds in
the Radiant `winit + vello` runtime. Accepted values: `1`, `true`, `on`, `yes`
(case-insensitive).

- `WAVECRATE_NATIVE_BRIDGE_PROFILE`
Enables UI bridge profiling logs when the build enables the
`native-bridge-metrics` cargo feature. Accepted values: `1`, `true`, `on`, `yes`
(case-insensitive).

- `WAVECRATE_NATIVE_BRIDGE_ASSERT_PROJECTION_SNAPSHOT`
Enables runtime validation that the cached UI bridge projection-key
snapshot still matches a freshly rebuilt key before projection. This is only
available when the build enables the `native-bridge-metrics` cargo feature.
Accepted values: `1`, `true`, `on`, `yes` (case-insensitive).

- `WAVECRATE_NATIVE_BRIDGE_IMMEDIATE_WAVEFORM_PREVIEW`
Controls whether waveform overlay preview actions (cursor + selection-range
updates) apply immediately in the UI bridge instead of waiting for queue
flush boundaries. Accepted values: `1`, `true`, `on`, `yes`
(case-insensitive). Default: enabled (`true`).

## Performance guard benchmark overrides

- `WAVECRATE_PERF_GUARD_OUT`
Output path used by `scripts/perf.* guard` for the benchmark JSON report.
Default: `target/perf/bench.json`.

- `WAVECRATE_PERF_GUARD_GUI_ROWS`
Synthetic GUI row count passed to the `wavecrate-bench` benchmark CLI for the
guard run.
Default: `2500`.

- `WAVECRATE_PERF_GUARD_GUI_INTERACTION_ROWS`
Synthetic row count used for focused interaction benchmark scenarios.
Default: `1500`.

- `WAVECRATE_PERF_GUARD_GUI_INTERACTION_ITERS`
Measured iteration count per focused interaction scenario.
Default: `24`.

- `WAVECRATE_PERF_GUARD_WARMUP_ITERS`
Warmup iteration count used during guard benchmark runs.
Default: `3`.

- `WAVECRATE_PERF_GUARD_MEASURE_ITERS`
Measured iteration count used for each guard benchmark action.
Default: `16`.

- `WAVECRATE_PERF_GUARD_RUNS`
Number of full `wavecrate-bench` benchmark CLI runs executed by
`scripts/perf.* guard`. When greater than `1`, the guard reports median
percentiles across runs and the p95 spread across runs. Default: `1`.

- `WAVECRATE_PERF_GUARD_STARTUP_PROFILE`
When set to `1`/`true`/`on`/`yes`, `scripts/perf.* guard` also captures
native startup timing logs by launching `wavecrate` under timeout and parsing
`RADIANT_NATIVE_STARTUP_PROFILE` output. Default: disabled (`0`).
For release-risk startup evidence, run this guard on supported app platforms.
The Bash `scripts/perf.sh calibrate-startup` command is Linux developer-only
threshold-refresh tooling and is not product Linux support.

- `WAVECRATE_PERF_GUARD_STARTUP_HIDDEN`
Optional developer-only startup-profile flag. When set to `1`/`true`/`on`/`yes`,
Wavecrate keeps its native window hidden after surface setup while preserving
first-present timing diagnostics. Leave this unset for release-risk startup
evidence because hidden-window captures do not measure user-visible first paint.

- `WAVECRATE_PERF_GUARD_STARTUP_TIMEOUT_SECS`
Timeout (seconds) used for each startup-profile capture run when startup
profiling is enabled. Default: `6`.

- `WAVECRATE_PERF_GUARD_STARTUP_MIN_VALID_RUNS`
Minimum number of startup profile captures that must include valid
`[native-vello-startup]` lines for startup summary confidence checks.
Default: `3` when `WAVECRATE_PERF_GUARD_RUNS >= 3`, otherwise `1`.

- `WAVECRATE_PERF_GUARD_STARTUP_REQUIRE_VALID_RUNS`
When set to `1`/`true`/`on`/`yes`, startup summary parsing fails if valid runs
fall below `WAVECRATE_PERF_GUARD_STARTUP_MIN_VALID_RUNS`. Default: disabled (`0`).

- `WAVECRATE_PERF_GUARD_STARTUP_SUMMARY_OUT`
Output path for the startup timing summary JSON generated by
`scripts/internal/perf/perf_startup_summary.py` when startup profiling is enabled.
Default: `<WAVECRATE_PERF_GUARD_OUT>.startup_summary.json`.

- `WAVECRATE_PERF_GUARD_STARTUP_LOCK_ENV_OUT`
Optional output env file path for startup threshold locking. When set and
startup profiling succeeds, `scripts/perf.* guard` writes startup threshold
assignments to this file using `scripts/internal/perf/perf_startup_lock_thresholds.py`.

- `WAVECRATE_PERF_GUARD_STARTUP_LOCK_ENV_IN`
Optional startup threshold lock-file input path sourced by
`scripts/perf.* guard` before threshold parsing. Defaults to the tracked
lock file at `scripts/internal/perf/locks/startup_thresholds.env`. Set to an empty value
to disable auto-loading.

- `WAVECRATE_PERF_GUARD_STARTUP_LOCK_MIN_VALID_RUNS`
Minimum valid startup-profile run count required before writing
`WAVECRATE_PERF_GUARD_STARTUP_LOCK_ENV_OUT`. Defaults to
`WAVECRATE_PERF_GUARD_STARTUP_MIN_VALID_RUNS`.

- `WAVECRATE_PERF_GUARD_FRAME_QUALITY_LOCK_ENV_OUT`
Optional output env file path for frame-quality threshold locking. When set,
`scripts/perf.* guard` writes calibrated frame-jank and missed-present
threshold assignments to this path using
`scripts/internal/perf/perf_frame_quality_lock_thresholds.py`.

- `WAVECRATE_PERF_GUARD_FRAME_QUALITY_LOCK_MIN_RUNS`
Minimum benchmark-report run count required before writing
`WAVECRATE_PERF_GUARD_FRAME_QUALITY_LOCK_ENV_OUT`. Defaults to `3` when
`WAVECRATE_PERF_GUARD_RUNS >= 3`, otherwise `1`.

- `WAVECRATE_PERF_WARN_STARTUP_FIRST_PRESENT_MS`
Warning threshold (milliseconds) for median startup `first_present_ms` in
startup profile summaries. Default: `800`.

- `WAVECRATE_PERF_FAIL_STARTUP_FIRST_PRESENT_MS`
Optional hard-fail threshold (milliseconds) for median startup
`first_present_ms` in startup profile summaries. Unset by default.

- `WAVECRATE_PERF_WARN_STARTUP_FIRST_PRESENT_SPREAD_MS`
Optional warning threshold (milliseconds) for startup `first_present_ms` spread
across captured runs. Unset by default.

- `WAVECRATE_PERF_FAIL_STARTUP_FIRST_PRESENT_SPREAD_MS`
Optional hard-fail threshold (milliseconds) for startup
`first_present_ms` spread across captured runs. Unset by default.

- `WAVECRATE_PERF_WARN_P95_US_HOVER`
Warning-only p95 latency threshold (microseconds) for hover interaction
benchmark results. Default: `8000`.

- `WAVECRATE_PERF_WARN_P95_US_FILTER_CHURN`
Warning-only p95 latency threshold (microseconds) for filter-only browser churn
benchmark results. Default: `10000`.

- `WAVECRATE_PERF_WARN_P95_US_QUERY_CHURN`
Warning-only p95 latency threshold (microseconds) for query-only browser churn
benchmark results. Default: `12000`.

- `WAVECRATE_PERF_WARN_P95_US_SORT_CHURN`
Warning-only p95 latency threshold (microseconds) for sort-only browser churn
benchmark results. Default: `10000`.

- `WAVECRATE_PERF_WARN_P95_US_WHEEL`
Warning-only p95 latency threshold (microseconds) for wheel interaction
benchmark results. Default: `10000`.

- `WAVECRATE_PERF_WARN_P95_US_MAP_PAN_PROXY`
Warning-only p95 latency threshold (microseconds) for map-pan-proxy interaction
benchmark results. Default: `12000`.

- `WAVECRATE_PERF_WARN_P95_US_WAVEFORM`
Warning-only p95 latency threshold (microseconds) for waveform interaction
benchmark results. Default: `10000`.

- `WAVECRATE_PERF_WARN_P95_US_WAVEFORM_ADJACENT`
Warning-only p95 latency threshold (microseconds) for adjacent waveform
pan/zoom interaction benchmark results. Default: `12000`.

- `WAVECRATE_PERF_WARN_P95_US_VOLUME`
Warning-only p95 latency threshold (microseconds) for continuous volume-drag
interaction benchmark results. Default: `8000`.

- `WAVECRATE_PERF_WARN_FRAME_JANK_RATIO`
Warning threshold (ratio `0.0..=1.0`) for per-scenario frame-jank proxy share
reported by `scripts/perf.* guard` from benchmark latency samples above
`frame_budget_us` (`16667us`). Default: `0.10`.

- `WAVECRATE_PERF_FAIL_FRAME_JANK_RATIO`
Optional hard-fail threshold (ratio `0.0..=1.0`) for per-scenario frame-jank
proxy share. Unset by default.

- `WAVECRATE_PERF_WARN_MISSED_PRESENT_PROXY_RATIO`
Warning threshold (ratio `0.0..=1.0`) for per-scenario missed-present proxy
share reported by `scripts/perf.* guard` from benchmark latency samples
above `2 * frame_budget_us` (`33334us`). Default: `0.05`.

- `WAVECRATE_PERF_FAIL_MISSED_PRESENT_PROXY_RATIO`
Optional hard-fail threshold (ratio `0.0..=1.0`) for per-scenario
missed-present proxy share. Unset by default.

- `WAVECRATE_PERF_FAIL_P95_US_HOVER`
Optional hard-fail p95 latency threshold (microseconds) for hover interaction
benchmark results. Unset by default.

- `WAVECRATE_PERF_FAIL_P95_US_FILTER_CHURN`
Optional hard-fail p95 latency threshold (microseconds) for filter-only browser
churn benchmark results. Unset by default.

- `WAVECRATE_PERF_FAIL_P95_US_QUERY_CHURN`
Optional hard-fail p95 latency threshold (microseconds) for query-only browser
churn benchmark results. Unset by default.

- `WAVECRATE_PERF_FAIL_P95_US_SORT_CHURN`
Optional hard-fail p95 latency threshold (microseconds) for sort-only browser
churn benchmark results. Unset by default.

- `WAVECRATE_PERF_FAIL_P95_US_WHEEL`
Hard-fail p95 latency threshold (microseconds) for wheel interaction benchmark
results. Default: `30000` (can be overridden; set to a very high value to
effectively disable).

- `WAVECRATE_PERF_FAIL_P95_US_FOCUS_PREVIEW`
Optional hard-fail p95 latency threshold (microseconds) for browser-focus
preview interaction benchmark results. Unset by default.

- `WAVECRATE_PERF_FAIL_P95_US_FOCUS_COMMIT`
Hard-fail p95 latency threshold (microseconds) for browser-focus commit
interaction benchmark results. Default: `100000` (can be overridden; set to a
very high value to effectively disable).

- `WAVECRATE_PERF_FAIL_P95_US_WAVEFORM`
Optional hard-fail p95 latency threshold (microseconds) for waveform
interaction benchmark results. Unset by default.

- `WAVECRATE_PERF_FAIL_P95_US_WAVEFORM_ADJACENT`
Optional hard-fail p95 latency threshold (microseconds) for adjacent waveform
pan/zoom interaction benchmark results. Unset by default.

- `WAVECRATE_PERF_FAIL_P95_US_VOLUME`
Optional hard-fail p95 latency threshold (microseconds) for continuous
volume-drag interaction benchmark results. Unset by default.

- `WAVECRATE_PERF_FAIL_P95_US_MAP_PAN_PROXY`
Hard-fail p95 latency threshold (microseconds) for map-pan-proxy interaction
benchmark results. Default: `4000` (can be overridden; set to a very high value
to effectively disable).

## Analysis job tuning and debugging

- `WAVECRATE_ANALYSIS_WORKERS`
Overrides the analysis worker count (integer >= 1).

- `WAVECRATE_DECODE_WORKERS`
Overrides the decode worker count (integer >= 1).

- `WAVECRATE_ANALYSIS_CLAIM_BATCH`
Overrides the analysis job claim batch size (integer >= 1). Default: `64`.

- `WAVECRATE_DECODE_QUEUE_TARGET`
Overrides the decode queue target size (integer >= 1).

- `WAVECRATE_ANALYSIS_STALE_SECS`
Overrides the “stale running job” timeout in seconds (integer >= 60). Default:
`120`.

- `WAVECRATE_ANALYSIS_LOG_JOBS`
Enables additional analysis worker logging when set to `1`.

- `WAVECRATE_ANALYSIS_LOG_QUEUE`
Enables additional analysis queue logging when set to `1`.

## SQLite extension loading (optional)

- `WAVECRATE_SQLITE_EXT`
Path to a loadable SQLite extension (`.so`, `.dll`, `.dylib`).

- `WAVECRATE_SQLITE_EXT_ENABLE`
Must be set to a truthy value (for example `1`) to opt in to loading
`WAVECRATE_SQLITE_EXT`.

- `WAVECRATE_SQLITE_EXT_UNSAFE`
Requests unsafe loading mode (bypasses allowlist and relaxes safety checks).
This is ignored unless the build enables the `sqlite-ext-unsafe` cargo feature.

## Updater safety overrides

- `WAVECRATE_UPDATER_ALLOW_SYMLINK_ERRORS`
Dev/test override that allows the updater to proceed when symlink validation
fails due to I/O or permission errors. Accepted values: `1`, `true`, `yes`
(case-insensitive).

## Tests and ad-hoc debugging

- `WAVECRATE_TEST_AUDIO_OUTPUT`
Test-only audio-device override. When running tests, set
`WAVECRATE_TEST_AUDIO_OUTPUT=1` (or `true`/`yes`/`on`) to exercise real audio
output devices. By default this is disabled so tests avoid opening host audio
devices, which keeps headless CI runs deterministic and reduces ALSA/driver
noise.

- `WAVECRATE_TRANSIENT_DEBUG`
When set (to any value), enables transient detection debug logs.

- `WAVECRATE_PANNS_GOLDEN_PATH`
Path to the golden log-mel JSON used by the golden regression tests (typically
set by `scripts/check.sh golden-tests` / `scripts/check.ps1 golden-tests`).

- `WAVECRATE_PANNS_EMBED_GOLDEN_PATH`
Path to the golden embedding JSON used by the golden regression tests (typically
set by `scripts/check.sh golden-tests` / `scripts/check.ps1 golden-tests`).

## Wheel stability workflow overrides

- `WAVECRATE_PERF_WHEEL_STABILITY_ROOT`
Artifact root used by `scripts/perf.sh wheel-stability` for collected
wheel-latency evidence and summary outputs. Default:
`target/perf/wheel_stability`.

- `WAVECRATE_PERF_WHEEL_STABILITY_WINDOWS`
Number of evidence windows collected in `collect` mode. Default: `7`.

- `WAVECRATE_PERF_WHEEL_STABILITY_RUNS_PER_WINDOW`
Number of perf-guard runs executed per collected window. Default: `3`.

- `WAVECRATE_PERF_WHEEL_STABILITY_REQUIRED_WINDOWS`
Number of most-recent windows required by `evaluate` mode before promotion can
be marked ready. Default: `7`.

- `WAVECRATE_PERF_WHEEL_STABILITY_TARGET_P95_US`
Maximum median wheel p95 (microseconds) allowed per window for promotion
readiness. Default: `16000`.

- `WAVECRATE_PERF_WHEEL_STABILITY_MAX_P95_SPREAD_US`
Maximum in-window wheel p95 spread (microseconds) allowed for readiness.
Default: `7000`.

- `WAVECRATE_PERF_WHEEL_STABILITY_MAX_STDDEV_US`
Maximum median wheel standard deviation (microseconds) allowed per window.
Default: `6000`.

- `WAVECRATE_PERF_WHEEL_STABILITY_MAX_OUTLIER_RATIO`
Maximum median wheel high-outlier ratio allowed per window (0.0-1.0).
Default: `0.35`.

- `WAVECRATE_PERF_WHEEL_STABILITY_SUMMARY_OUT`
Output path for wheel-stability readiness summary JSON written by
`scripts/perf.sh wheel-stability`. Default:
`target/perf/wheel_stability/wheel_stability_summary.json`.

- `WAVECRATE_PERF_WHEEL_STABILITY_ENFORCE_READY`
When truthy (`1`, `true`, `yes`, `on`), `scripts/perf.sh wheel-stability
evaluate` exits non-zero if readiness criteria are not met. Default: `0`.

## Issue reporting token storage

These variables affect the GitHub issue token storage backend in
`src/issue_gateway/token_store.rs`.

- `WAVECRATE_DISABLE_KEYRING`
Disables OS keyring usage when set to a truthy value.

- `WAVECRATE_ALLOW_FALLBACK_TOKEN_STORAGE`
Opt-in switch that permits encrypted on-disk fallback token storage when keyring
operations fail or are unavailable.

- `WAVECRATE_FALLBACK_KEY`
Supplies the 32-byte fallback encryption key via an environment variable (useful
when keyring storage is unavailable). This is expected to be base64-encoded.
