#!/usr/bin/env bash

# Bootstrap a known-good local dev environment (humans + agents).
#
# - Installs the pinned Rust toolchain from rust-toolchain.toml
# - Ensures rustfmt/clippy/cargo-nextest are available
# - Detects optional local `sccache` support for explicit opt-in Cargo caching
# - Checks git-lfs and python3
# - Prints next-step commands for smoke, quick, and full validation tiers

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

VERIFY_ONLY=0

usage() {
  cat <<'EOF'
Usage: scripts/bootstrap.sh [--verify-only]

Default: installs/ensures a known-good local environment:
- pinned Rust toolchain from rust-toolchain.toml
- rustfmt + clippy components
- cargo-nextest
- optional sccache detection for explicit opt-in Cargo caching
- checks git-lfs, python3, and rg

  --verify-only:
  - performs checks only (no installs / no machine mutation)
  - exits non-zero if required items are missing
  - skip agent preflight hook installation
EOF
}

while (( $# > 0 )); do
  case "$1" in
    --verify-only)
      VERIFY_ONLY=1; shift ;;
    -h|--help)
      usage; exit 0 ;;
    *)
      echo "[bootstrap] Unknown argument: $1" >&2
      usage >&2
      exit 2 ;;
  esac
done

install_agent_preflight_hooks() {
  local hook_script="$ROOT_DIR/scripts/agent.sh"

  if [[ "${WAVECRATE_SKIP_AGENT_PREFLIGHT_HOOK_INSTALL:-0}" == "1" ]]; then
    echo "[bootstrap] WAVECRATE_SKIP_AGENT_PREFLIGHT_HOOK_INSTALL=1: skipping hook install."
    return 0
  fi

  if [[ ! -x "$hook_script" ]]; then
    echo "[bootstrap] ERROR: missing hook installer at $hook_script" >&2
    return 1
  fi

  if "$hook_script" install-hooks --force; then
    echo "[bootstrap] agent preflight hooks: installed"
    return 0
  fi

  echo "[bootstrap] ERROR: failed to install agent preflight hooks." >&2
  return 1
}

failures=0

echo "[bootstrap] repo: $ROOT_DIR"

if ! command -v git >/dev/null 2>&1; then
  echo "[bootstrap] ERROR: git not found on PATH" >&2
  exit 1
fi

if command -v rg >/dev/null 2>&1; then
  echo "[bootstrap] rg: OK"
else
  echo "[bootstrap] rg: MISSING (recommended; several repo checks use it)" >&2
  echo "[bootstrap]   Install ripgrep (rg). Examples:" >&2
  echo "[bootstrap]     macOS: brew install ripgrep" >&2
  echo "[bootstrap]     Ubuntu/Debian: sudo apt-get install ripgrep" >&2
  echo "[bootstrap]     Windows: winget install BurntSushi.ripgrep.MSVC" >&2
  if (( VERIFY_ONLY == 1 )); then
    failures=$((failures + 1))
  fi
fi

if command -v git-lfs >/dev/null 2>&1 || (git lfs version >/dev/null 2>&1); then
  echo "[bootstrap] git-lfs: OK"
  # Ensure hooks are configured locally (best effort).
  git lfs install --local >/dev/null 2>&1 || true
else
  echo "[bootstrap] git-lfs: MISSING (recommended)"
  echo "[bootstrap]   Install git-lfs and run: git lfs install --local" >&2
fi

if command -v python3 >/dev/null 2>&1; then
  py_ver="$(python3 -c 'import sys; print(".".join(map(str, sys.version_info[:3])))' 2>/dev/null || echo "unknown")"
  echo "[bootstrap] python3: OK ($py_ver)"
else
  echo "[bootstrap] python3: MISSING (recommended; used by some tooling/scripts)" >&2
fi

if ! command -v rustup >/dev/null 2>&1; then
  echo "[bootstrap] ERROR: rustup not found on PATH" >&2
  echo "[bootstrap]   Install rustup from https://rustup.rs/ and re-run this script." >&2
  exit 1
fi

if [[ ! -f rust-toolchain.toml ]]; then
  echo "[bootstrap] ERROR: rust-toolchain.toml not found at repo root" >&2
  exit 1
fi

read_toolchain_channel() {
  # Prefer Python 3.11+ tomllib, but fall back to a simple regex for the single
  # `toolchain.channel = "..."` value this repo uses.
  if command -v python3 >/dev/null 2>&1; then
    if python3 - <<'PY' >/dev/null 2>&1
import sys
sys.exit(0 if sys.version_info >= (3, 11) else 1)
PY
    then
      python3 - <<'PY'
import tomllib
from pathlib import Path
data = tomllib.loads(Path("rust-toolchain.toml").read_text(encoding="utf-8"))
print(data.get("toolchain", {}).get("channel", "stable"))
PY
      return 0
    fi
  fi

  # Minimal fallback: extract first `channel = "..."` line.
  sed -n 's/^[[:space:]]*channel[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' rust-toolchain.toml | head -n 1
}

channel="$(read_toolchain_channel | tr -d '[:space:]')"
if [[ -z "$channel" ]]; then
  channel="stable"
fi

echo "[bootstrap] rust toolchain (pinned): $channel"

if rustup run "$channel" rustc -V >/dev/null 2>&1; then
  echo "[bootstrap] pinned toolchain installed: yes"
else
  echo "[bootstrap] pinned toolchain installed: no ($channel)" >&2
  if (( VERIFY_ONLY == 1 )); then
    failures=$((failures + 1))
  else
    echo "[bootstrap] rustup toolchain install $channel"
    rustup toolchain install "$channel" --profile minimal
  fi
fi

installed_components="$(rustup component list --toolchain "$channel" --installed 2>/dev/null || true)"
has_fmt=0
has_clippy=0
grep -q '^rustfmt' <<<"$installed_components" && has_fmt=1
grep -q '^clippy' <<<"$installed_components" && has_clippy=1

if (( has_fmt == 1 )); then
  echo "[bootstrap] rustfmt: installed (toolchain $channel)"
else
  echo "[bootstrap] rustfmt: missing (toolchain $channel)" >&2
  if (( VERIFY_ONLY == 1 )); then
    failures=$((failures + 1))
  else
    echo "[bootstrap] rustup component add rustfmt --toolchain $channel"
    rustup component add rustfmt --toolchain "$channel"
  fi
fi

if (( has_clippy == 1 )); then
  echo "[bootstrap] clippy: installed (toolchain $channel)"
else
  echo "[bootstrap] clippy: missing (toolchain $channel)" >&2
  if (( VERIFY_ONLY == 1 )); then
    failures=$((failures + 1))
  else
    echo "[bootstrap] rustup component add clippy --toolchain $channel"
    rustup component add clippy --toolchain "$channel"
  fi
fi

if cargo metadata --locked --format-version 1 >/dev/null 2>&1; then
  echo "[bootstrap] Cargo dependency resolution: OK"
else
  echo "[bootstrap] Cargo dependency resolution: FAILED" >&2
  if (( VERIFY_ONLY == 1 )); then
    failures=$((failures + 1))
  else
    exit 1
  fi
fi

if cargo nextest --version >/dev/null 2>&1; then
  echo "[bootstrap] cargo-nextest: installed"
else
  echo "[bootstrap] cargo-nextest: missing" >&2
  if (( VERIFY_ONLY == 1 )); then
    failures=$((failures + 1))
  else
    echo "[bootstrap] cargo install cargo-nextest --locked"
    cargo install cargo-nextest --locked
  fi
fi

if command -v sccache >/dev/null 2>&1; then
  echo "[bootstrap] sccache: installed (set WAVECRATE_ENABLE_SCCACHE=1 to use it)"
else
  echo "[bootstrap] sccache: optional but missing"
  echo "[bootstrap]   Install for faster repeated builds. Examples:" >&2
  echo "[bootstrap]     cargo install sccache --locked" >&2
  echo "[bootstrap]     Windows: winget install Mozilla.sccache" >&2
fi

echo
echo "[bootstrap] Next steps:"
echo "  - Environment sanity:   bash scripts/doctor.sh"
echo "  - App-only check:       bash scripts/ci.sh smoke --app-only"
echo "  - Workspace smoke:      bash scripts/ci.sh smoke --workspace"
echo "  - Smoke devcheck:       bash scripts/ci.sh smoke"
echo "  - Fast test checks:     bash scripts/ci.sh quick"
echo "  - CI parity checks:     bash scripts/ci.sh local"
echo "  - Safe local run:       bash scripts/run.sh sandbox --"

if (( VERIFY_ONLY == 1 )); then
  if (( failures > 0 )); then
    echo "[bootstrap] Result: FAIL ($failures missing requirements)" >&2
    echo "[bootstrap] Hint: run without --verify-only to install missing requirements." >&2
    exit 1
  fi
  echo "[bootstrap] Result: OK"
else
  if ! install_agent_preflight_hooks; then
    echo "[bootstrap] Agent workspace setup is incomplete without the preflight hooks."
    echo "[bootstrap]   Re-run: bash scripts/agent.sh install-hooks --force"
    exit 1
  fi
  echo "[bootstrap] Result: OK"
fi
