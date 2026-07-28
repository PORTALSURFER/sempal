#!/usr/bin/env bash

# Run deterministic runtime interaction benchmarks and emit drift diagnostics.
#
# This guard may fail when explicit fail-threshold env overrides are configured
# (or for scenarios with default fail thresholds). Warning thresholds remain
# non-blocking.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT_DIR"
# shellcheck source=scripts/internal/setup_headless_audio.sh
source "$ROOT_DIR/scripts/internal/setup_headless_audio.sh"
# Tooling-only Linux fallback for headless perf hosts; not product support.
wavecrate_setup_headless_audio "perf_guard"

OUT_PATH="${WAVECRATE_PERF_GUARD_OUT:-target/perf/bench.json}"
GUI_ROWS="${WAVECRATE_PERF_GUARD_GUI_ROWS:-2500}"
GUI_INTERACTION_ROWS="${WAVECRATE_PERF_GUARD_GUI_INTERACTION_ROWS:-1500}"
GUI_INTERACTION_ITERS="${WAVECRATE_PERF_GUARD_GUI_INTERACTION_ITERS:-24}"
WARMUP_ITERS="${WAVECRATE_PERF_GUARD_WARMUP_ITERS:-3}"
MEASURE_ITERS="${WAVECRATE_PERF_GUARD_MEASURE_ITERS:-16}"
RUNS="${WAVECRATE_PERF_GUARD_RUNS:-1}"
PERF_STATE_ROOT="${WAVECRATE_PERF_GUARD_STATE_ROOT:-$ROOT_DIR/target/perf/runtime}"
STARTUP_PROFILE_RAW="${WAVECRATE_PERF_GUARD_STARTUP_PROFILE:-0}"
STARTUP_TIMEOUT_SECS="${WAVECRATE_PERF_GUARD_STARTUP_TIMEOUT_SECS:-6}"
STARTUP_REQUIRE_VALID_RAW="${WAVECRATE_PERF_GUARD_STARTUP_REQUIRE_VALID_RUNS:-0}"
STARTUP_LOCK_ENV_OUT="${WAVECRATE_PERF_GUARD_STARTUP_LOCK_ENV_OUT:-}"
STARTUP_LOCK_ENV_IN="${WAVECRATE_PERF_GUARD_STARTUP_LOCK_ENV_IN:-$ROOT_DIR/scripts/internal/perf/locks/startup_thresholds.env}"
FRAME_QUALITY_LOCK_ENV_OUT="${WAVECRATE_PERF_GUARD_FRAME_QUALITY_LOCK_ENV_OUT:-}"
CARGO_BIN="${CARGO_BIN:-cargo}"

lowercase() {
  printf '%s' "$1" | tr '[:upper:]' '[:lower:]'
}

startup_profile_enabled=0
case "$(lowercase "$STARTUP_PROFILE_RAW")" in
  1|true|yes|on)
    startup_profile_enabled=1
    ;;
esac

startup_require_valid_runs=0
case "$(lowercase "$STARTUP_REQUIRE_VALID_RAW")" in
  1|true|yes|on)
    startup_require_valid_runs=1
    ;;
esac

load_threshold_lock_env() {
  local lock_path="$1"
  local lock_label="$2"
  if [[ -z "$lock_path" ]]; then
    return
  fi
  if [[ ! -f "$lock_path" ]]; then
    return
  fi
  # shellcheck disable=SC1090
  source "$lock_path"
  echo "[perf_guard] loaded ${lock_label} threshold lock env: $lock_path"
}

load_threshold_lock_env "$STARTUP_LOCK_ENV_IN" "startup"

mkdir -p "$(dirname "$OUT_PATH")"
mkdir -p "$PERF_STATE_ROOT/config" "$PERF_STATE_ROOT/data" "$PERF_STATE_ROOT/state"

if ! [[ "$RUNS" =~ ^[0-9]+$ ]] || [ "$RUNS" -lt 1 ]; then
  echo "[perf_guard] ERROR: WAVECRATE_PERF_GUARD_RUNS must be an integer >= 1" >&2
  exit 1
fi

# Keep benchmark config/data writes inside the repo unless the caller overrides XDG dirs.
export XDG_CONFIG_HOME="${XDG_CONFIG_HOME:-$PERF_STATE_ROOT/config}"
export XDG_DATA_HOME="${XDG_DATA_HOME:-$PERF_STATE_ROOT/data}"
export XDG_STATE_HOME="${XDG_STATE_HOME:-$PERF_STATE_ROOT/state}"

declare -a REPORT_PATHS=()
declare -a STARTUP_LOG_PATHS=()

if (( startup_profile_enabled == 1 )) && ! command -v timeout >/dev/null 2>&1; then
  echo "[perf_guard] WARN: startup profiling requested but \`timeout\` is unavailable; skipping startup capture" >&2
  startup_profile_enabled=0
fi

if (( startup_profile_enabled == 1 )); then
  startup_binary="${ROOT_DIR}/target/debug/wavecrate"
  echo "[perf_guard] building wavecrate startup binary for profile capture"
  "$CARGO_BIN" build --bin wavecrate >/dev/null
  if [[ "$RUNS" -ge 3 ]]; then
    startup_min_valid_runs_default=3
  else
    startup_min_valid_runs_default=1
  fi
  STARTUP_MIN_VALID_RUNS="${WAVECRATE_PERF_GUARD_STARTUP_MIN_VALID_RUNS:-$startup_min_valid_runs_default}"
fi

if [[ "$RUNS" -ge 3 ]]; then
  frame_quality_min_runs_default=3
else
  frame_quality_min_runs_default=1
fi
FRAME_QUALITY_LOCK_MIN_RUNS="${WAVECRATE_PERF_GUARD_FRAME_QUALITY_LOCK_MIN_RUNS:-$frame_quality_min_runs_default}"

for run in $(seq 1 "$RUNS"); do
  run_out="$OUT_PATH"
  if [ "$RUNS" -gt 1 ]; then
    run_out="${OUT_PATH%.json}.run${run}.json"
  fi
  REPORT_PATHS+=("$run_out")
  echo "[perf_guard] running wavecrate-bench interaction profile (run ${run}/${RUNS})"
  "$CARGO_BIN" run -p wavecrate-bench-cli --bin wavecrate-bench -- \
    --out "$run_out" \
    --no-analysis \
    --no-similarity \
    --gui \
    --gui-rows "$GUI_ROWS" \
    --gui-interaction-rows "$GUI_INTERACTION_ROWS" \
    --gui-interaction-iters "$GUI_INTERACTION_ITERS" \
    --warmup-iters "$WARMUP_ITERS" \
    --measure-iters "$MEASURE_ITERS"
  if (( startup_profile_enabled == 1 )); then
    startup_log="${OUT_PATH%.json}.startup.run${run}.log"
    STARTUP_LOG_PATHS+=("$startup_log")
    echo "[perf_guard] capturing native startup profile (run ${run}/${RUNS})"
    set +e
    RADIANT_NATIVE_STARTUP_PROFILE=1 \
      timeout --signal=TERM --kill-after=1s "${STARTUP_TIMEOUT_SECS}s" \
      "$startup_binary" >"$startup_log" 2>&1
    startup_status=$?
    set -e
    if [[ "$startup_status" -ne 0 && "$startup_status" -ne 124 && "$startup_status" -ne 143 ]]; then
      echo "[perf_guard] WARN: startup profiling exited with status ${startup_status}; see ${startup_log}" >&2
    fi
  fi
done

if [ "$RUNS" -gt 1 ]; then
  last_index=$((${#REPORT_PATHS[@]} - 1))
  cp "${REPORT_PATHS[$last_index]}" "$OUT_PATH"
fi

echo "[perf_guard] parsing benchmark reports (${RUNS} run(s)); canonical report: $OUT_PATH"
python3 scripts/internal/perf/evaluate_perf_guard_report.py \
  --contract scripts/internal/data/validation_contract.json \
  "${REPORT_PATHS[@]}"

if [[ -n "$FRAME_QUALITY_LOCK_ENV_OUT" ]]; then
  python3 scripts/internal/perf/perf_frame_quality_lock_thresholds.py \
    --out "$FRAME_QUALITY_LOCK_ENV_OUT" \
    --min-runs "$FRAME_QUALITY_LOCK_MIN_RUNS" \
    "${REPORT_PATHS[@]}"
fi

if (( startup_profile_enabled == 1 )); then
  startup_summary_out="${WAVECRATE_PERF_GUARD_STARTUP_SUMMARY_OUT:-${OUT_PATH%.json}.startup_summary.json}"
  startup_summary_cmd=(
    python3
    scripts/internal/perf/perf_startup_summary.py
    --output
    "$startup_summary_out"
    --warn-first-present-ms
    "${WAVECRATE_PERF_WARN_STARTUP_FIRST_PRESENT_MS:-800}"
    --min-valid-runs
    "$STARTUP_MIN_VALID_RUNS"
  )
  if [[ -n "${WAVECRATE_PERF_FAIL_STARTUP_FIRST_PRESENT_MS:-}" ]]; then
    startup_summary_cmd+=(
      --fail-first-present-ms
      "$WAVECRATE_PERF_FAIL_STARTUP_FIRST_PRESENT_MS"
    )
  fi
  if [[ -n "${WAVECRATE_PERF_WARN_STARTUP_FIRST_PRESENT_SPREAD_MS:-}" ]]; then
    startup_summary_cmd+=(
      --warn-first-present-spread-ms
      "$WAVECRATE_PERF_WARN_STARTUP_FIRST_PRESENT_SPREAD_MS"
    )
  fi
  if [[ -n "${WAVECRATE_PERF_FAIL_STARTUP_FIRST_PRESENT_SPREAD_MS:-}" ]]; then
    startup_summary_cmd+=(
      --fail-first-present-spread-ms
      "$WAVECRATE_PERF_FAIL_STARTUP_FIRST_PRESENT_SPREAD_MS"
    )
  fi
  if (( startup_require_valid_runs == 1 )); then
    startup_summary_cmd+=(--require-min-valid-runs)
  fi
  startup_summary_cmd+=("${STARTUP_LOG_PATHS[@]}")
  "${startup_summary_cmd[@]}"
  if [[ -n "$STARTUP_LOCK_ENV_OUT" ]]; then
    STARTUP_LOCK_MIN_VALID_RUNS="${WAVECRATE_PERF_GUARD_STARTUP_LOCK_MIN_VALID_RUNS:-$STARTUP_MIN_VALID_RUNS}"
    python3 scripts/internal/perf/perf_startup_lock_thresholds.py \
      --summary "$startup_summary_out" \
      --out "$STARTUP_LOCK_ENV_OUT" \
      --min-valid-runs "$STARTUP_LOCK_MIN_VALID_RUNS"
  fi
fi
