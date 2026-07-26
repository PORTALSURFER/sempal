#!/usr/bin/env bash

# Ensures `.github/CODEOWNERS` continues to mirror the high-level ownership
# buckets described in `docs/TARGET.md`.
#
# This is intentionally lightweight: it checks for coverage, not exact matches.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT_DIR"

CODEOWNERS_PATH=".github/CODEOWNERS"

if [[ ! -f "$CODEOWNERS_PATH" ]]; then
  echo "[codeowners_coverage] Missing $CODEOWNERS_PATH" >&2
  exit 1
fi

has_pattern_prefix() {
  local prefix="$1"
  awk -v p="$prefix" '
    /^[[:space:]]*#/ {next}
    /^[[:space:]]*$/ {next}
    {
      pat=$1
      if (pat==p) { found=1; exit }
      if (index(pat,p)==1) { found=1; exit }
    }
    END { exit(found?0:1) }
  ' "$CODEOWNERS_PATH"
}

required_prefixes=(
  "*"
  "/.github/"
  "/scripts/"
  "/docs/"
  "/manual/"
  "/tools/"
  "/src/app_core/"
  "/src/app/"
  "/src/analysis/"
  "/src/audio/"
  "/src/gui/"
  "/src/gui_runtime/"
  "/src/gui_test/"
  "/src/sample_sources/"
  "/src/selection/"
)

missing=0
for prefix in "${required_prefixes[@]}"; do
  if ! has_pattern_prefix "$prefix"; then
    if (( missing == 0 )); then
      echo "[codeowners_coverage] Missing required CODEOWNERS bucket entries:" >&2
      echo "[codeowners_coverage] (Update docs/TARGET.md and .github/CODEOWNERS together.)" >&2
    fi
    echo " - $prefix" >&2
    missing=$((missing + 1))
  fi
done

if (( missing > 0 )); then
  exit 1
fi

echo "[codeowners_coverage] OK"
exit 0
