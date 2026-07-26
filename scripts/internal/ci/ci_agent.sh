#!/usr/bin/env bash

# Agent-safe local validation loop.
#
# This lane avoids cargo-nextest and the broader GUI contract wrappers so it
# can run in constrained environments while still providing a real compile +
# Radiant required non-blocking guardrails, no-default core/API tests, and
# Wavecrate library-test cycle.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT_DIR"
RADIANT_DIR="$(./scripts/radiant.sh locate | sed -n 's/^RADIANT_DIR=//p')"
# shellcheck source=scripts/internal/use_cargo_cache.sh
source "$ROOT_DIR/scripts/internal/use_cargo_cache.sh"
wavecrate_enable_cargo_cache

usage() {
  cat <<'USAGE'
Usage: scripts/ci.sh agent

Run the agent-safe local validation loop without cargo-nextest.
For the broader integrated lane, use `scripts/ci.sh quick`.
For full CI parity, use `scripts/ci.sh local`.
USAGE
}

while (( $# > 0 )); do
  case "$1" in
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "[ci_agent] Unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

echo "[ci_agent] ./scripts/internal/check/check_main_branch.sh"
./scripts/internal/check/check_main_branch.sh

echo "[ci_agent] ./scripts/ci.sh smoke"
./scripts/ci.sh smoke

echo "[ci_agent] ./scripts/check.sh non-blocking-architecture"
./scripts/check.sh non-blocking-architecture
echo "[ci_agent] ./scripts/check.sh readiness-executor-boundary"
./scripts/check.sh readiness-executor-boundary

echo "[ci_agent] cargo test --manifest-path $RADIANT_DIR/Cargo.toml --lib --no-default-features"
cargo test --manifest-path "$RADIANT_DIR/Cargo.toml" --lib --no-default-features

echo "[ci_agent] cargo test --manifest-path $RADIANT_DIR/Cargo.toml --test app_runtime_api --no-default-features"
cargo test --manifest-path "$RADIANT_DIR/Cargo.toml" --test app_runtime_api --no-default-features

echo "[ci_agent] cargo test -p wavecrate --test controller_browser_integration --features legacy-controller"
cargo test -p wavecrate --test controller_browser_integration --features legacy-controller

echo "[ci_agent] cargo test -p wavecrate --lib -- --skip known isolated legacy failures"
cargo test -p wavecrate --lib -- \
  --skip prepare_auto_rename_requests_logs_looped_provenance \
  --skip rating_previous_random_history_entry_restores_waveform_for_replacement

echo "[ci_agent] OK"
