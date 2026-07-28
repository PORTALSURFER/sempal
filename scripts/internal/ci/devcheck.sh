#!/usr/bin/env bash

# Run the fastest local compile/smoke gate.
#
# This checks library, test, and binary targets without running the suite.
# Use this during the tight edit loop, then escalate to `ci_quick.sh` for
# filtered tests and `ci_local.sh` for full CI parity.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT_DIR"
# shellcheck source=scripts/internal/use_cargo_cache.sh
source "$ROOT_DIR/scripts/internal/use_cargo_cache.sh"
wavecrate_enable_cargo_cache

usage() {
  cat <<'EOF'
Usage: scripts/ci.sh smoke [--app-only] [--workspace]

Run the compile/smoke gate.
Use `--app-only` for the lightest app-only check.
Use `--workspace` for the broad workspace compile gate.
For fast test coverage, use `scripts/ci.sh quick`.
EOF
}

APP_ONLY=0
WORKSPACE=0

while (( $# > 0 )); do
  case "$1" in
    --app-only)
      APP_ONLY=1
      shift
      ;;
    --workspace)
      WORKSPACE=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "[devcheck] Unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if (( APP_ONLY == 1 && WORKSPACE == 1 )); then
  echo "[devcheck] --app-only and --workspace are mutually exclusive." >&2
  usage >&2
  exit 2
fi

echo "[devcheck] branch policy"
./scripts/internal/check/check_main_branch.sh

echo "[devcheck] cargo metadata --locked"
cargo metadata --locked --format-version 1 >/dev/null

if (( APP_ONLY == 1 )); then
  echo "[devcheck] cargo check -p wavecrate --lib --bin wavecrate"
  cargo check -p wavecrate --lib --bin wavecrate
elif (( WORKSPACE == 1 )); then
  echo "[devcheck] cargo check --workspace --tests --bins"
  cargo check --workspace --tests --bins
else
  echo "[devcheck] cargo check -p wavecrate --tests --bins"
  cargo check -p wavecrate --tests --bins
fi

echo "[devcheck] OK"
