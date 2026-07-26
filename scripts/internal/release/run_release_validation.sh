#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
RADIANT_DIR="$("$ROOT_DIR/scripts/radiant.sh" locate | sed -n 's/^RADIANT_DIR=//p')"

echo "[release_validation] Build Wavecrate workspace test targets."
cargo test --workspace --locked --no-run

echo "[release_validation] Build standalone Radiant test targets."
cargo test --manifest-path "$RADIANT_DIR/Cargo.toml" --locked --lib --no-default-features --no-run
cargo test --manifest-path "$RADIANT_DIR/Cargo.toml" --locked --test app_runtime_api --no-default-features --no-run

echo "[release_validation] Run release workflow contract checks."
cargo test --test release_contract
cargo test --test release_workflow_helpers

echo "[release_validation] Run scanner tests used by release-time source validation."
cargo test -p wavecrate-scan --lib

echo "[release_validation] OK"
