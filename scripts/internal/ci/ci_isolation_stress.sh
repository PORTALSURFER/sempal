#!/usr/bin/env bash

# Repeat the Wavecrate library harness in fresh explicitly parallel processes.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT_DIR"
# shellcheck source=scripts/internal/use_cargo_cache.sh
source "$ROOT_DIR/scripts/internal/use_cargo_cache.sh"
wavecrate_enable_cargo_cache

usage() {
  cat <<'USAGE'
Usage: scripts/ci.sh isolation-stress [options]

Options:
  --iterations N       Fresh test processes to run (default: 5).
  --test-threads N     Parallel libtest threads per process (default: 2-8 by CPU).
  --timeout-seconds N  Per-process timeout (default: 900).
  --output PATH        JSONL report path (default: timestamped target artifact).
  -h, --help           Show this help.

This opt-in lane verifies its injected leak sentinels, then runs the Wavecrate
library suite repeatedly. It stops at the first unexpected failure and never
retries a failing iteration until green.
USAGE
}

for argument in "$@"; do
  case "$argument" in
    -h|--help)
      usage
      exit 0
      ;;
  esac
done

exec python3 "$ROOT_DIR/scripts/internal/ci/parallel_isolation_stress.py" "$@"
