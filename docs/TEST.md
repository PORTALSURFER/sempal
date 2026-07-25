# Development Workflow and Test Map

This document is the canonical developer workflow guide for local iteration,
validation, and test selection.

## Default validation ladder

Use the lightest lane that still gives trustworthy coverage for the change.

1. Tight edit loop
   - Windows PowerShell:
     `powershell -ExecutionPolicy Bypass -File scripts/ci.ps1 smoke`
   - macOS/Linux/WSL:
     `bash scripts/ci.sh smoke`
2. Agent-safe validation
   - Windows PowerShell:
     `powershell -ExecutionPolicy Bypass -File scripts/ci.ps1 agent`
   - macOS/Linux/WSL:
     `bash scripts/ci.sh agent`
   - load-sensitive selection-export background-job tests run serially here
     after the default library test pass.
3. Broader integrated local checks
   - Windows PowerShell:
     `powershell -ExecutionPolicy Bypass -File scripts/ci.ps1 quick`
   - macOS/Linux/WSL:
     `bash scripts/ci.sh quick`
4. Full local CI parity
   - Windows PowerShell:
     `powershell -ExecutionPolicy Bypass -File scripts/ci.ps1 local`
   - macOS/Linux/WSL:
     `bash scripts/ci.sh local`

### Opt-in parallel-isolation stress lane

Use the normal `agent` lane for routine compile and broad library validation.
Use the parallel-isolation stress lane after changing test runtime guards,
process environment/current-directory handling, mutable global test controls,
worker cleanup, or when investigating a failure that disappears on retry:

- Windows PowerShell:
  `powershell -ExecutionPolicy Bypass -File scripts/ci.ps1 isolation-stress -Iterations 5 -TestThreads 8`
- macOS/Linux/WSL:
  `bash scripts/ci.sh isolation-stress --iterations 5 --test-threads 8`

This lane is deliberately not part of `scripts/ci.* agent`. It compiles the
Wavecrate library harness once, proves that deliberately injected
process-state and mutable-global-control leaks are detected, and then starts a
fresh test-binary process for every bounded iteration. Each process uses the
requested explicit libtest parallelism and a 15-minute default timeout. The
lane stops at the first unexpected failure instead of retrying until green.

Every process writes one compact JSONL record containing the test binary,
iteration, parallelism, status, test name, failure class, and disposition. The
metadata record separately identifies known quarantined coverage and its
exclusion reason. Reports default to a timestamped file under
`target/test-isolation-stress/`; pass `--output` on Bash or `-Output` on
PowerShell to select a stable artifact path.

Focused runner coverage, including report parsing, fresh-process invocation,
timeouts, exit behavior, and injected-leak classification, is available with:

`python3 -m unittest scripts/internal/ci/test_parallel_isolation_stress.py`

## Agent preflight and Git hooks

`scripts/agent.sh preflight` / `scripts/agent.sh request` and their PowerShell
equivalents own the full agent preflight, including Cargo-backed architecture
checks. Concurrent full-preflight invocations for the same repository coalesce
to one owner and report the shared result. Branch-changing `post-checkout`
hooks deliberately run only `run_agent_hook_checks.sh`: bounded
repository-state checks that preserve reviewed-head validation evidence during
merge cleanup.

On macOS, the supported Bash `scripts/agent.sh` and `scripts/ci.sh` entrypoints
use a Cargo target keyed by the Rust host/release and `Cargo.lock`. This keeps
validation independent from the unbounded general-purpose `target/debug/deps`
directory while preserving warm reuse across repeated runs. These entrypoints
override an inherited `CARGO_TARGET_DIR`; use `WAVECRATE_VALIDATION_TARGET_ROOT`
to customize validation cache placement. A target whose
`debug/deps` directory itself becomes pathological is atomically quarantined
before Cargo starts; the emitted path can be removed after any needed
diagnostics have been retained. A per-target lease serializes supported macOS
validation commands so quarantine cannot race an active Cargo user; stale
leases are recovered when their recorded PID and process-start identity no
longer identify a live, non-zombie owner. Waiting for a genuinely active owner
is bounded and exits 124 without disturbing that owner's processes or lease.

Those entrypoints also own a process-group watchdog. A changing owned process
tree or increasing aggregate CPU time counts as progress, so a quiet but active
clean compile is not a stall. After five minutes with neither signal, the
watchdog records the exact command, Cargo/Rust/macOS versions, owned process
tree, and macOS samples under `target/validation-diagnostics/`, with one
30-second budget across all collection. It then allows a full two minutes for
recovery before exiting 124 and terminating only the process group it created.
With the default five-second polling and ten-second termination grace, the
maximum scheduled bound is 7 minutes 50 seconds after last observed progress.
Cancellation uses the same owned cleanup path.

Run `bash scripts/internal/agent/test_agent_preflight_coordination.sh` to
exercise hook installation, checkout ownership, concurrent coalescing,
and stale-lock recovery without invoking Cargo.
Run `bash scripts/internal/validation/test_validation_watchdog.sh` to exercise
progress classification, diagnostics, cancellation/stall cleanup, unrelated
process isolation, and pathological-target rotation.
The fixture also covers reused-PID stale-lease recovery and bounded waits for a
genuinely live lease owner.
It also emulates several unresponsive compiler samples to prove the global
diagnostic budget and post-collection recovery interval.
Pre-spawn cancellation coverage proves a signal cannot start and detach a new
validation process group after cancellation has already been recorded.
Nested wrapper calls bypass duplicate supervision only after verifying the
enclosing watchdog's live PID/start identity, lease record, command, and process
ancestry. An inherited recursion marker without that proof starts a fresh
watchdog normally.

## Validation and release lane contract

GitHub Actions owns release packaging, upload, and the release workflow
validation gates. Local wrappers remain the primary validation contract for
development and release-risk checks:

### Release lifecycle and promotion policy

Wavecrate develops continuously on `main` through the normal PR pipeline. The
first RC for a version is a project decision to enter stabilization, not a
branch freeze or an automatic precursor to an immediate stable release.

1. Develop normally through PRs into `main`.
2. When the current product is ready for stabilization, prepare `release/X.Y`
   from `main` and publish `vX.Y.Z-rc.1`.
3. Continue normal PRs into `main`, limiting the release train to stability
   work: bug fixes, regression fixes, reliability, performance, packaging, and
   release blockers. Feature work waits until stabilization ends unless the
   release scope is explicitly changed.
4. When fixes have landed after an RC, move the release branch to the current
   `main` commit with the same target version and publish the next RC:

   ```bash
   scripts/internal/release/prepare_release_train.py \
     --version X.Y.Z \
     --source-ref main \
     --push
   scripts/release.sh rc \
     --version X.Y.Z \
     --rc-number N \
     --branch release/X.Y \
     --dispatch
   ```

5. Continue publishing RCs while stable publication is temporarily disabled.
   Ensure every change intended for the train is published as an RC.
   The preserved stable workflow promotes the exact latest RC commit, but it is
   stored outside GitHub's active workflow extensions and the local stable
   command fails closed. Re-enabling stable publication requires a dedicated
   reviewed change; PR approval, `approved`, sign-off, or RC acceptance does
   not authorize that change or a stable publication.

Keep the remote `release/X.Y` branch as the release coordination ref. It is not
a disposable feature branch and must remain available for subsequent RCs and
the eventual explicit stable promotion.

| Lane | GitHub job | Local command | Policy |
| --- | --- | --- | --- |
| Format and guardrails | none | `scripts/ci.* local` includes the required guardrails; use `scripts/check.* <name>` for focused reruns | Run locally before broad/risky changes |
| Clippy | none | `scripts/ci.* local` | Run locally before broad/risky changes |
| Docs | none | `scripts/ci.* local` | Run locally before broad/risky changes |
| Public mdBook docs | none | `scripts/check.* mdbook` | Run when changing `book.toml` or `docs/book/src/` |
| Tests | none | `scripts/ci.* local` runs `cargo nextest run --workspace --profile ci-required --all-targets --no-fail-fast` on the local platform | Run locally before broad/risky changes |
| Quarantined/slow tests | none | `cargo nextest run --workspace --profile ci-quarantine --all-targets --no-fail-fast` | Advisory until the tests are hardened |
| Dead dependency sweep and env-var nudge | none | `scripts/check.* dead-deps --advisory` and `scripts/check.* report-env-vars` | Advisory Linux-only hygiene |
| GUI semantic contracts | none | `scripts/gui.ps1 contract` or `scripts/gui.ps1 suite` | Local/manual or issue-specific |
| Perf guard | none | `scripts/perf.* guard` | Local/manual or release-risk validation |
| Nightly release build/sync | `Wavecrate nightly release` on the evening schedule or manual dispatch | release workflow dispatch | Resolves the exact `main` SHA, runs `scripts/internal/release/run_release_validation.sh` on Ubuntu before package builds or publication, builds Windows/macOS nightly assets from that SHA, embeds `X.Y.Z-nightly.DATE+SHA` metadata, verifies the checksum signing key against the pinned public key before public publication, uploads and verifies the immutable PortalSurfer release plus full changelog first, then refreshes the rolling GitHub `nightly` release assets and promotes the immutable and rolling GitHub tags as the final public identity step |
| RC release | `Wavecrate RC release` manual dispatch | release workflow dispatch | Builds Windows/macOS RC assets from `release/X.Y`, validates the requested package version and branch, runs `scripts/internal/release/run_release_validation.sh`, verifies the checksum signing key against the pinned public key before public publication, and publishes `vX.Y.Z-rc.N` as a GitHub pre-release |
| Stable release | Disabled | `scripts/release.sh stable` fails closed | The implementation is preserved in `.github/workflows/release-stable.yml.disabled`; restore it through a dedicated reviewed change only when Wavecrate is ready for stable publication |

Use `scripts/release.sh` from a clean repo root for macOS/Linux release
orchestration. It derives major, minor, and patch target versions from the
package version at the resolved source ref, delegates release-train prep to
`scripts/internal/release/prepare_release_train.py`, and dispatches the RC
GitHub workflow only when `--dispatch` is passed. Prepare dispatch uses
`--workflow-ref` (default `main`) for the workflow file ref and sends the
resolved source commit SHA through the `source_ref` workflow input. The script
requires `gh` only for explicit workflow dispatch. When `origin` is a GitHub
remote, it must match `WAVECRATE_GITHUB_REPO` before release refs are fetched or
resolved. Dry RC runs print the exact workflow command and follow-up run-list
command without publishing. Stable commands fail before resolving release refs
or invoking GitHub.

The nextest policy is:

- `ci-required` is the merge-blocking test subset and excludes known timing,
  platform, or permission-sensitive tests while they are being hardened.
- `ci-quarantine` owns that excluded coverage. Run it deliberately when working
  on source-DB contention, browser async controller integration, or updater
  stale-removal behavior.
- The default nextest profile stays unfiltered for maintainers who want the
  complete suite and are prepared to triage environment-specific failures.
- Local nextest runs remain the primary development evidence. GitHub release
  workflows add deterministic packaging-time checks for nightly, RC, and stable
  artifacts.
- Nightly, RC, and stable use `scripts/internal/release/run_release_validation.sh`
  on `ubuntu-latest` before any package build or public publication. The helper
  compiles Wavecrate-owned workspace test targets with `cargo test --workspace
  --locked --exclude radiant --no-run`, then runs release contract/helper tests
  and scanner tests. It intentionally avoids running the full native/UI default
  Cargo test lane in scheduled release workflows; local `ci-required` nextest,
  quarantined, GUI/manual, and performance checks remain explicit local or
  issue-specific release-risk evidence.
- The nightly, RC, and stable Ubuntu release-validation jobs install
  `pkg-config` and `libasound2-dev` before Cargo builds the workspace.
  Wavecrate still publishes only the Windows/macOS release targets declared in
  `release_contract.toml`, but the Linux-hosted validation lane keeps
  ALSA-backed compilation explicit ahead of planned Linux release support.

The non-blocking app architecture is enforced by
`scripts/check.* non-blocking-architecture`, and that check is required by
`scripts/ci.* agent` plus the agent preflight inside `scripts/ci.* local`. It
runs Radiant's reusable non-blocking guardrails, Wavecrate's app-facing blocking
scan, and the deterministic strict slow-handler diagnostics harness.
The agent lane intentionally runs the focused Radiant non-blocking guardrail
module instead of the full `generic_surface_guardrails` suite, because that
broader suite also contains unrelated cleanup/source-shape guardrails that
should be fixed deliberately rather than used as the non-blocking architecture
gate.

Windows note:

- use the PowerShell wrappers in this repository
- the wrappers use direct `rustc` by default and still fall back to repo-local
  temp space when the default temp directory is unusable
- set `WAVECRATE_ENABLE_SCCACHE=1` only when you explicitly want wrapper caching
- do not run multiple cargo test commands concurrently

Headless Linux audio note:

- `scripts/internal/setup_headless_audio.sh` is retained as developer
  validation tooling for Bash CI, agent, and perf lanes that may run on
  headless Linux hosts
- `scripts/ci.sh local` and `scripts/perf.sh guard` source it to set
  `ALSA_CONFIG_PATH=scripts/internal/alsa_headless.conf` only when the host is
  Linux, no display server is present, and the caller has not already set
  `ALSA_CONFIG_PATH`
- this dummy ALSA path reduces CPAL/ALSA missing-device warning noise in
  test/bench logs and does not represent shipped Linux app support

## Safe feature-change checklist

Before push:

1. Route the change to the right owner using `docs/TARGET.md`.
2. Add or update tests for non-trivial logic.
3. Update the canonical doc that owns the changed behavior.
4. Run the appropriate validation lane until green.
5. Keep the diff focused and avoid broad incidental cleanup.

## Test suite map

### Root crate tests

Use for most app/domain behavior under `src/`.

- all project tests:
  - `cargo nextest run --all-targets --no-fail-fast`
  - required CI subset:
    `cargo nextest run --workspace --profile ci-required --all-targets --no-fail-fast`
  - quarantined/slow subset:
    `cargo nextest run --workspace --profile ci-quarantine --all-targets --no-fail-fast`
  - `cargo test --doc`
- quick app-development subset:
  - `cargo nextest run --profile quick --lib --tests`
  - `scripts/ci.* quick` first lists the selected binaries with the same profile
    and target set, so stale inventory-dependent filter predicates fail before
    the test run starts
- agent-safe library suite:
  - `cargo test -p wavecrate --lib`
  - `scripts/ci.* agent` runs the library suite with two known legacy
    controller failures skipped so the required agent lane can stay focused on
    deterministic architecture, compile, and broad non-ignored library coverage:
    `prepare_auto_rename_requests_logs_looped_provenance` and
    `rating_previous_random_history_entry_restores_waveform_for_replacement`.

### Committed file-mutation and watcher reconciliation

Use these focused lanes when changing Wavecrate-owned create, copy/extract,
import/drop, edit, normalize, undo/redo, rename, move, trash/delete, source
manifest reconciliation, or watcher echo handling:

- mutation contract, source revision, identity, invalidation, partial failure,
  stale completion, and large-file background behavior:
  `cargo test -p wavecrate --bin wavecrate committed_file_mutations`
- watcher acknowledgement, deduplication, and external-change fallback:
  `cargo test -p wavecrate --bin wavecrate source_watcher`
- strict compile coverage for message routing and every native call site:
  `cargo check -p wavecrate --all-targets`

The mutation tests should prove that path-only moves retain content identity,
content changes invalidate file-derived readiness, deletes invalidate source
membership, and one operation ID covers committed and failed parts. The watcher
tests should prove that an acknowledged internal echo is removed without
starting duplicate work and that later external changes still reach the
authoritative reconciliation path.

### Source-processing liveness, churn, and performance

Use this explicit lane when changing source readiness, the background supervisor,
watcher-to-manifest reconciliation, committed mutations, retry/lease recovery,
source removal/re-add, playback scheduling, or exact similarity publication.
The end-to-end tests are ignored by the default suite so normal CI does not
inherit a real-time soak:

- deterministic liveness oracle:
  `cargo test -p wavecrate --lib liveness_oracle_rejects_actionable_deficits_without_observable_runtime_work`
- real temporary source through add, targeted and overflow reconciliation,
  same-size content change, rename, delete, playback pause, committed internal
  mutation, closed-app restart audit, root disappearance/reappearance, and
  source remove/re-add:
  `cargo test -p wavecrate --lib source_processing_liveness_harness_converges_restart_churn_and_root_recovery -- --ignored --nocapture`
- 10,000-file / 30,001-target calibrated source-processing profile:
  `cargo test -p wavecrate --lib profile_source_processing_churn_under_playback_and_browser_priority -- --ignored --nocapture`
- fully analyzed 10,000-file periodic-sweep and one-file readiness-delta profile:
  `cargo test -p wavecrate --lib profile_revision_gated_readiness_sweeps_10k -- --ignored --nocapture`
- full mutation and watcher family coverage:
  `cargo test -p wavecrate --lib committed_file_mutations`
  and
  `cargo test -p wavecrate --lib source_watcher`
- supervisor and durable-readiness failure injection (busy/resource contention,
  retry deadlines, expired leases, cancellation, worker failures, unsupported
  inputs, and stale completions):
  `cargo test -p wavecrate --lib native_app::source_processing::supervisor::tests`
  and
  `cargo test -p wavecrate-library sample_sources::readiness::tests`
- seeded, replayable source lifecycle and scan-order state machine (six retained
  regression seeds in the bounded normal-CI scanner lane):
  `cargo test -p wavecrate --lib source_processing_seeded_state_machine_normal_ci`
- integrated real-supervisor replay for the calibrated lifecycle seed:
  `cargo test -p wavecrate --lib source_processing_seeded_state_machine_integrated_supervisor -- --ignored --nocapture`
- extended 1,000-sequence state-machine stress lane:
  `cargo test -p wavecrate --lib source_processing_seeded_state_machine_stress_1000 -- --ignored --nocapture`
- replay a decimal seed or the minimized JSON artifact path printed by a
  failure:
  `WAVECRATE_SOURCE_STATE_MACHINE_REPLAY=<seed-or-artifact> cargo test -p wavecrate --lib source_processing_seeded_state_machine_replay -- --ignored --nocapture`
- UI frame and input budgets while exercising browser interactions:
  `bash scripts/perf.sh guard` on macOS/Linux/WSL or
  `powershell -ExecutionPolicy Bypass -File scripts/perf.ps1 guard` on Windows

Run the deterministic liveness test repeatedly when changing timing or wakeup
behavior. Its oracle reads committed manifest/readiness generations and
supervisor state directly; it does not infer completion from UI progress or
sleep for a fixed success delay. An active source with actionable deficits must
be dirty/scheduled, queued, in flight, resource-paused, waiting for a retry, or
waiting for an exact prerequisite. A stable state outside those categories
fails as silently idle.

The seeded state-machine lane runs only against the versioned
`small-multi-source` native fixture. It generates filesystem mutations,
coalesced watcher/audit/foreground causes, focus changes, cancellation,
restart, source removal/re-add, root loss/replacement, and deterministic
transaction/publication/watcher/hash/lifecycle failure boundaries. Every
observable commit checks the reference manifest and browser projection,
monotonic accepted revisions, at-most-once revision/cause publication, bounded
cause coalescing, and controlled-quiescence liveness. The integrated lane
submits duplicate deltas through the real supervisor queue and observes the
durable readiness publication/fallback output. Failures are greedily
minimized without removing lifecycle transitions and written below
`target/source-state-machine-failures/` (or the active `CARGO_TARGET_DIR`);
artifacts preserve scanner-only versus integrated execution mode and reject
fixture-version drift during replay.

Timeouts and invariant failures emit a JSON snapshot containing source and
readiness generations, availability, activity, per-stage deficits and
classifications, durable job status/lease/retry state, active runtime work,
watcher stimulus/root health, and supervisor resource counters. The calibrated
profile enforces these source-processing budgets in debug test builds:

- materialize 30,001 exact targets from 10,000 files within 180 seconds;
- sustain at least 200 materialized candidates per second;
- keep browser-priority update p99 at or below 10 ms while playback pauses
  processing;
- keep process-memory growth at or below 512 MiB; and
- keep average process CPU at or below two core-equivalents and process disk
  reads/writes at or below 1 GiB each for candidate materialization; and
- record zero supervisor database-contention events.

The calibrated source-processing profile prints wall time, completion
throughput, queue-priority latency, CPU time/core-equivalent, memory growth,
disk I/O, and database-contention events. The fully analyzed readiness profile
prints ten consecutive converged safety-probe costs and one-file
readiness-delta costs as JSON. Its steady-state and one-file sections include
wall time, CPU time, live heap growth, process disk reads/writes, and target-row
writes so their cost can be compared directly with an exact pre-change run.
Pair both with the maintained perf guard for the frame/input-latency budgets from
`scripts/internal/data/validation_contract.json`.

### Script and golden checks

Use for tooling, fixtures, and numerical-reference flows.

- `bash scripts/check.sh golden-tests`
- `bash scripts/check.sh mdbook`
- `cargo nextest run golden_log_mel_matches_python`
- `cargo nextest run golden_embedding_matches_python`

### Public mdBook documentation

Use this lane when changing `book.toml` or files under `docs/book/src/`.

- build:
  - `bash scripts/check.sh mdbook`
  - or `mdbook build`
- preview:
  - `mdbook serve --hostname 127.0.0.1 --port 3011`
- generated local output:
  - `target/mdbook/wavecrate-docs/`
- public publishing target:
  - `https://portalsurfer.org/wavecrate/docs/`

### Wavecrate UI projection compatibility tests

Use for Wavecrate-owned projection snapshot and behavior changes. Default GUI
work should prefer focused tests around `src/native_app.rs` and the relevant
Radiant public API surface.

- targeted snapshot suites:
  - `startup_shot_matches_fixture`
  - `browser_dense_shot_matches_fixture`
  - `waveform_selection_shot_matches_fixture`
  - update fixtures with
    `cargo test -p wavecrate --lib update_shot_fixtures -- --ignored`

### Sample navigation responsiveness

Use this focused harness when changing sample selection, deferred loading,
waveform cache promotion, decode scheduling, or stale sample-load completion
handling:

- `cargo test -p wavecrate rapid_navigation_harness --no-default-features`

The harness exercises rapid keyboard navigation while sample-load business work
is queued, verifies visible selection updates before completions, checks stale
deferred/sample-load completions cannot overwrite the current selection, and
asserts Radiant runtime diagnostics report no slow UI update handlers.

Manual cold-cache smoke for large folders:

1. Run Wavecrate against a source folder with many uncached audio files.
2. Navigate quickly through the sample browser with keyboard arrows and mouse
   selection while cache/decode work is still cold.
3. Confirm row selection and status feedback move immediately.
4. Confirm late loads from earlier selections do not steal playback, selection,
   or visible error state from the current row.

### Radiant compatibility tests

Use for generic Radiant behavior and compatibility coverage in the vendored
runtime dependency.

- `cargo nextest run --manifest-path vendor/radiant/Cargo.toml`

### GUI test platform

Use for semantic GUI contracts and CLI scenarios.

- Automated GUI runs default to the isolated `isolated-startup` fixture, which
  exercises the production `NativeAppState`/Radiant composition against a
  temporary config base using the dedicated `automated-tests` persistence
  profile.
- Product-native source-system runs may select `empty`, `small-multi-source`,
  or `large-source`. `small-multi-source` is the routine source scanning,
  readiness, watcher, progress, source-switching, and visual-QA baseline;
  `large-source` is opt-in for performance and high-cardinality coverage only.
- Provision and launch the routine native fixture with
  `bash scripts/run.sh sandbox --name source-qa --fixture small-multi-source`
  or `powershell -ExecutionPolicy Bypass -File scripts/run.ps1 sandbox -Name source-qa -Fixture small-multi-source`.
  Provisioning resets by default; pass `--fixture-preserve` or
  `-FixturePreserve` only when intentionally continuing the same mutation run.
- The shared `wavecrate-fixture` tool emits
  `.wavecrate/fixtures/<name>/fixture-manifest.json` with stable source ids,
  relative paths, hashes, file counts, and expected readiness-target counts.
  Its deterministic `create`, `same-size-change`, `move`, `delete`,
  `root-offline`, `root-online`, and `reset` scenarios replace ambient-folder
  mutations in source-system validation.
- Use the `live` GUI fixture only for deliberate manual validation against the
  real persisted startup profile. It uses the same native composition as the
  product executable and is the only GUI fixture allowed to resolve live state.
- Treat `default` only as a legacy alias for `isolated-startup`; it is not a
  separate live-profile mode.
- Named fixtures such as `browser`, `waveform`, and `map` remain explicitly
  legacy-controller compatibility coverage. They do not certify native startup
  or background-worker ownership.
- Native startup artifacts identify `fixture_runtime: native-app`, report native
  watcher/readiness-supervisor counts, report zero legacy analysis pools, and
  include the native shutdown artifact.

- Browser preview/commit flicker regressions:
  - targeted controller checks: `cargo test browser_focus_transition`
  - semantic GUI proof lane: `powershell -ExecutionPolicy Bypass -File scripts/gui.ps1 contract`
  - contract scenario anchor: `browser_focus_transition_stability`
- contract loop:
  - `powershell -ExecutionPolicy Bypass -File scripts/gui.ps1 contract`
  - this lane explicitly runs ignored fixture-backed GUI and bridge-runtime
    checks, including action parity and `contract_smoke_pack_runs_cleanly`; the
    default library test lane keeps only the cheaper runner and assertion
    checks.
- broader GUI suite:
  - `powershell -ExecutionPolicy Bypass -File scripts/gui.ps1 suite`

See `docs/TARGET.md` for the GUI artifact and automation contract details.

### Persistence profile boundary

Use these checks when the change could affect startup config loading/saving,
sample-source persistence, or GUI fixture profile selection.

- profile model:
  - live runs use `<config-base>/.wavecrate/`
  - sandbox/manual QA runs use `<config-base>/.wavecrate/profiles/sandbox/`
  - automated validation runs use `<config-base>/.wavecrate/profiles/automated-tests/`
- regression anchors:
  - `cargo test app_core::controller::tests::persistence_boundary::`
  - `cargo test gui_test::`
  - `cargo test native_app::automation::tests::startup_capture_uses_native_workers_and_shutdown_hook`
- wrapper coverage:
  - `powershell -ExecutionPolicy Bypass -File scripts/gui.ps1 contract`
    includes the native-startup regression that snapshots live configuration
    bytes before and after an isolated native-runtime run, plus the retained
    controller `library.db` boundary checks
  - `powershell -ExecutionPolicy Bypass -File scripts/ci.ps1 agent`
    still covers the same regression through `cargo test -p wavecrate --lib`

### Windows manual confirmation

Use this after boundary/profile work when you want one explicit PowerShell proof
that validation did not leak fixture sources into the real startup profile.

1. Run `powershell -ExecutionPolicy Bypass -File scripts/ci.ps1 agent`.
2. Run `powershell -ExecutionPolicy Bypass -File scripts/gui.ps1 contract`.
3. Start one normal live runtime without `WAVECRATE_CONFIG_HOME` or
   `WAVECRATE_CONFIG_PROFILE` overrides:
   `cargo run --release`
4. Confirm the app opens with the intended real source list instead of temp or
   fixture paths.
5. If you need a sandbox/manual QA run instead, use
   `powershell -ExecutionPolicy Bypass -File scripts/run.ps1 sandbox --`
   so the session writes to the dedicated `sandbox` profile instead of the live
   profile.

### Windows ZIP Distribution Lifecycle

Windows releases are distributed as downloadable ZIP artifacts. A supported
Windows release package contains the `wavecrate/` archive root, the
`wavecrate/wavecrate.exe` runtime binary, and `wavecrate/update-manifest.json`.
Wavecrate does not currently provide OS-managed app registration or automatic
system cleanup for the supported release path.

Windows users install Wavecrate by extracting the ZIP to a folder they manage.
To update a manual install, close Wavecrate, download a newer ZIP, and replace
the extracted `wavecrate/` folder or its files with the newer release contents.
To remove Wavecrate, delete the extracted folder. App settings, source metadata,
logs, and rebuildable caches live under the user's `.wavecrate` app data paths
and can be removed separately when the user deliberately wants to clear local
Wavecrate state.

Focused validation for Windows release distribution changes:

- `cargo test --test release_contract`
- `python3 -m py_compile scripts/internal/release/verify_published_release.py`
  when archive verification behavior changes
- Inspect the release ZIP and confirm it preserves the archive root and required
  runtime files declared by `release_contract.toml`

### Database migration and compatibility

Use this lane when changing `.wavecrate.db`, `library.db`, source metadata
queries, schema DDL, schema version stamps, or migration code.

- source DB contract and migration fixtures:
  - `cargo test -p wavecrate-library source_db_migration_tests`
- read-only source query compatibility:
  - `cargo test -p wavecrate-library sample_sources::db::read`
- global library DB contract and migrations:
  - `cargo test -p wavecrate-library sample_sources::library`
- compile check for the storage crate:
  - `cargo check -p wavecrate-library`

See `docs/DATABASE_MIGRATIONS.md` for the schema-change contract. Source DB
additive repairs must run even when `PRAGMA user_version` already matches the
current schema, and read-only source opens must tolerate pre-migration database
shapes by projecting safe defaults or avoiding missing optional tables.

### Debug log workflow

On macOS, build manual-test binaries with `bash scripts/build.sh --release`.
The helper emits a branch-specific, ad-hoc-signed app under
`target/app-bundles/` and prints both its Launch Services identity and launch
command. Use that app name for UI automation; launching `target/release/wavecrate`
directly does not register a distinct macOS application and can cause automation
to fall back to an installed Wavecrate bundle.

Use this when reproducing a release/manual bug that needs the richer
per-launch diagnostics trail.

1. Start a repro run with the Wavecrate-owned debug flag:
   `cargo run --release -- --log`
   For a live run that also shows debug layout overlays, use either spelling:
   `powershell -ExecutionPolicy Bypass -File scripts/run.ps1 logs debug-overlays`
   or `.\run.ps1 logs debug-layout`.
2. For isolated manual QA, use the sandbox profile instead:
   `powershell -ExecutionPolicy Bypass -File scripts/run.ps1 sandbox -- --log`
3. Retrieve the newest log quickly:
   `powershell -ExecutionPolicy Bypass -File scripts/run.ps1 logs`
4. Retrieve the sandbox run log the same way:
   `powershell -ExecutionPolicy Bypass -File scripts/run.ps1 logs -Sandbox`
5. Remember the boundary:
   the log captures startup metadata plus structured action/DB diagnostics for
   the current reproduced run only; earlier sessions still require a fresh repro
   with `--log` enabled.
6. Extra-verbose hot-path traces are separate:
   default `--log` keeps routine loop-cycle and similar hot-path chatter out of
   the file, but it still records warning-level slow starmap audition hot phases.
   Only enable `WAVECRATE_HOTPATH_TELEMETRY=1` when you deliberately need
   developer-level loop/hot-path tracing during a focused repro.

### Benchmarks and perf checks

- `cargo bench`
- `cargo bench --bench ann_index`
- `cargo bench --bench tagging`
- local perf guard:
  - `bash scripts/perf.sh guard`
  - `powershell -ExecutionPolicy Bypass -File scripts/perf.ps1 guard`

Startup threshold calibration is not a release lane by itself. The Bash
`scripts/perf.sh calibrate-startup` command is optional Linux developer tooling
for refreshing startup threshold lock files on compositor-backed hosts. Current
release-risk startup perf evidence should be collected with `scripts/perf.* guard`
on supported app platforms, using `WAVECRATE_PERF_GUARD_STARTUP_PROFILE=1` when
first-paint startup timing evidence is required.

## CI reference

GitHub CI and local wrappers together cover:

- formatting
- clippy
- required nextest
- doc tests
- selected guardrail scripts
- advisory quarantined tests and hygiene checks
- local-only perf and GUI contract lanes where appropriate

When in doubt, run the wrapper script instead of assembling the command list by
hand.
