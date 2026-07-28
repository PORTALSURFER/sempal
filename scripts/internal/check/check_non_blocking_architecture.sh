#!/usr/bin/env bash

# Required deterministic guardrails for Radiant's non-blocking app
# architecture. Static scans and controlled diagnostics are hard gates; flaky
# timing benchmarks stay outside this lane.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT_DIR"
# shellcheck source=scripts/internal/use_cargo_cache.sh
source "$ROOT_DIR/scripts/internal/use_cargo_cache.sh"
wavecrate_enable_cargo_cache

usage() {
  cat <<'EOF'
Usage: scripts/internal/check/check_non_blocking_architecture.sh

Run required Radiant and Wavecrate non-blocking architecture guardrails.
EOF
}

while (( $# > 0 )); do
  case "$1" in
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "[non_blocking_architecture] Unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

run_step() {
  local label="$1"
  shift
  echo "[non_blocking_architecture] $label"
  "$@"
}

run_step "Wavecrate app-facing blocking guardrail" \
  cargo test -p wavecrate --no-default-features --test gui_boundary native_app_ui_update_paths_do_not_call_blocking_business_apis

run_step "Wavecrate strict slow-handler diagnostics harness" \
  cargo test -p wavecrate --no-default-features --lib rapid_navigation_harness_keeps_ui_responsive_while_business_work_is_slow

run_step "Wavecrate readiness persistence boundary" \
  bash -c '
    set -euo pipefail
    supervisor_source="$(
      cat src/native_app/source_processing/supervisor.rs
      find src/native_app/source_processing/supervisor -maxdepth 1 -type f -name "*.rs" \
        -exec cat {} +
    )"
    similarity_source="$(cat src/native_app/sample_library/similarity_artifacts/worker.rs)"
    production_source="${supervisor_source}
${similarity_source}"
    if printf "%s\\n" "$production_source" | rg -n "source_readiness_(sources|targets|artifacts)|(^|[^[:alnum:]_])analysis_jobs([^[:alnum:]_]|$)|readiness_managed"; then
      echo "[non_blocking_architecture] native source processing must use ReadinessStore for readiness persistence" >&2
      exit 1
    fi
  '

run_step "Wavecrate source-processing service boundary" \
  bash -c '
    set -euo pipefail
    service_dir="src/native_app/source_processing/supervisor"
    if rg -n "^use super::\\*;" "$service_dir" \
      --glob "*.rs" --glob "!**/tests/**"; then
      echo "[non_blocking_architecture] source-processing production modules must declare explicit contracts" >&2
      exit 1
    fi
    if rg -n \
      "SourceProcessingSupervisor|run_coordinator|CoordinatorExecutionState|execute_candidates" \
      "$service_dir"/discovery*.rs \
      "$service_dir"/execution*.rs \
      "$service_dir"/retirement*.rs \
      "$service_dir"/progress.rs \
      "$service_dir"/telemetry.rs; then
      echo "[non_blocking_architecture] source-processing services must not reach into the facade or coordinator" >&2
      exit 1
    fi
  '

echo "[non_blocking_architecture] OK"
