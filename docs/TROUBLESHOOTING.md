# Troubleshooting and Guardrails

Use this document when local setup, validation, or invariant checks fail.

## macOS Cargo validation stops producing output

Supported Bash validation writes a diagnostic bundle and exits 124 instead of
waiting indefinitely when its complete owned process tree has made no progress
for the documented bound. Inspect the newest directory under
`target/validation-diagnostics/`; it contains the exact command, toolchain and
macOS versions, process tree, and samples of idle Cargo/compiler processes.

A sample ending in `rustc_metadata::creader`, `dyld`, and `fcntl` while
`target/debug/deps` has unusually large directory metadata indicates an
accumulated flat Cargo artifact directory rather than a dependency source failure.
Use the supported `scripts/agent.sh` or `scripts/ci.sh` entrypoint: on macOS it
selects a bounded toolchain/lockfile-specific target automatically. Do not use a
Cargo's dependency cache as a substitute for the parent workspace gate.

If validation reports that it quarantined a pathological target, retain it only
while investigating the bundle, then remove the exact printed `stale-*` path.
Do not broadly delete `target/` while another Cargo run is active.

## First diagnostics

- environment sanity:
  - Windows PowerShell:
    `powershell -ExecutionPolicy Bypass -File scripts/doctor.ps1`
  - macOS/Linux/WSL:
    `bash scripts/doctor.sh`
- latest logs:
  - Windows PowerShell:
    `powershell -ExecutionPolicy Bypass -File scripts/run.ps1 logs`
  - macOS/Linux/WSL:
    `bash scripts/run.sh logs`
- bug bundle:
  - Windows PowerShell:
    `powershell -ExecutionPolicy Bypass -File scripts/run.ps1 bug-bundle`
  - macOS/Linux/WSL:
    `bash scripts/run.sh bug-bundle`

## Debug run logs

Use this when you need a richer reconstruction of runtime actions or DB work.

- enable Wavecrate-owned debug diagnostics for one run:
  - Windows PowerShell:
    `cargo run --release -- --log`
  - macOS/Linux/WSL:
    `cargo run --release -- --log`
- keep using the normal latest-log helper afterward:
  - Windows PowerShell:
    `powershell -ExecutionPolicy Bypass -File scripts/run.ps1 logs`
  - macOS/Linux/WSL:
    `bash scripts/run.sh logs`
- for sandbox/manual QA runs created through `scripts/run.ps1 sandbox`, use the
  matching sandbox retrieval path so the helper resolves
  `<config-base>/.wavecrate/profiles/sandbox/logs` instead of the live profile:
  - Windows PowerShell:
    `powershell -ExecutionPolicy Bypass -File scripts/run.ps1 logs -Sandbox`
  - macOS/Linux/WSL:
    `bash scripts/run.sh logs --sandbox`
- when collecting a report bundle from that same sandbox profile, use:
  - Windows PowerShell:
    `powershell -ExecutionPolicy Bypass -File scripts/run.ps1 bug-bundle -Sandbox`
  - macOS/Linux/WSL:
    `bash scripts/run.sh bug-bundle --sandbox`
- the log file captures startup metadata plus structured debug events from the
  run you reproduced with `--log`; it does not replace the manual reproduction
  step for an incident that already happened in an earlier session
- if you also set `RUST_LOG`, remember that it overrides the default filter;
  leave it unset for the standard debug contract unless you intentionally need a
  custom filter
- use `WAVECRATE_DEBUG_LOGGING=1` only for non-interactive launch surfaces where
  adding `--log` is awkward
- do not log secrets, tokens, auth headers, raw credentials, or large
  unredacted user payloads while extending diagnostics coverage

## Common failures

### Windows ASIO build failures

Symptoms:

- `cpal` build errors mentioning missing ASIO headers

Fix:

- set `CPAL_ASIO_DIR` to the ASIO SDK root containing `host/` and `common/`

### Keyring failures

Symptoms:

- issue-report token storage fails because the OS keyring is unavailable

Fix:

- preferred: repair the local keyring backend
- fallback:
  - `WAVECRATE_DISABLE_KEYRING=1`
  - `WAVECRATE_ALLOW_FALLBACK_TOKEN_STORAGE=1`
  - `WAVECRATE_FALLBACK_KEY=<base64 32-byte key>`

### Native font fallback issues

Symptoms:

- text rendering fails or the runtime cannot resolve a usable system font

Fix:

- set `RADIANT_NATIVE_FONT_PATH` to a known-good `.ttf`

### SQLite extension load blocked

Symptoms:

- vector-acceleration extension is skipped or blocked

Fix:

- set both `WAVECRATE_SQLITE_EXT` and `WAVECRATE_SQLITE_EXT_ENABLE=1`
- keep the extension under the allowed path unless you are intentionally using
  the unsafe mode and matching cargo feature

### Updater path validation failures

Symptoms:

- updater refuses to write through symlinked paths

Fix:

- prefer a non-symlink install path
- for dev/test-only validation issues, use
  `WAVECRATE_UPDATER_ALLOW_SYMLINK_ERRORS=1`

## Guardrail policy

Most repository invariants are enforced through `scripts/check.{sh,ps1}`
subcommands backed by `scripts/internal/check/`.

When a check fails:

1. fix the underlying issue if possible
2. use an allowlist only as a last resort
3. keep the error message and owning doc in sync
4. wire invariant changes through both shell and PowerShell wrappers

## Changing an invariant safely

1. update the check implementation
2. update or add the matching allowlist file if exceptions are required
3. wire the check into the correct CI/local wrapper
4. update the canonical docs in this folder if the workflow or boundary changed
5. rerun the relevant validation lane until it is green

## When to add a new doc

Usually, do not.

Add a new long-lived doc only when the information cannot fit cleanly into one
of the canonical docs without making that file misleading. Active plans and
backlog structure belong in Linear. Temporary investigations or supporting
notes can go to `tmp/` when a canonical doc is not appropriate.
