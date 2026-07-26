#!/usr/bin/env bash

# Diff-aware guardrail for private + public Rust item docs.
#
# Fails when added Rust items (fn/struct/enum/trait/type/const/static/mod) in
# src/ files are missing nearby doc comments.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT_DIR"
if [[ -f "$ROOT_DIR/scripts/internal/git_diff_env.sh" ]]; then
  # shellcheck source=scripts/internal/git_diff_env.sh
  source "$ROOT_DIR/scripts/internal/git_diff_env.sh"
else
  wavecrate_git() {
    git "$@"
  }
fi

BASE_REF=""
HEAD_REF="HEAD"

usage() {
  cat <<'USAGE'
Usage: scripts/internal/check/check_rust_private_docs.sh [--base <ref>] [--head <ref>]

Fails when added Rust items introduce missing doc comments.

Allowlist file:
  scripts/internal/check/allowlists/rust_private_docs_allowlist.txt
USAGE
}

while (( $# > 0 )); do
  case "$1" in
    --base)
      BASE_REF="${2:-}"; shift 2 ;;
    --head)
      HEAD_REF="${2:-}"; shift 2 ;;
    -h|--help)
      usage; exit 0 ;;
    *)
      echo "[private_docs] Unknown argument: $1" >&2
      usage >&2
      exit 2 ;;
  esac
done

if ! command -v python3 >/dev/null 2>&1; then
  echo "[private_docs] ERROR: python3 is required for this check" >&2
  exit 2
fi

ALLOWLIST_PATH="$ROOT_DIR/scripts/internal/check/allowlists/rust_private_docs_allowlist.txt"

git_has_commit() {
  wavecrate_git rev-parse --verify --quiet "$1^{commit}" >/dev/null 2>&1
}

scan_diff() {
  local label="$1"
  local source="$2"
  local head_ref="$3"
  shift 3

  wavecrate_git diff --unified=0 --diff-filter=AMR "$@" -- src \
    | python3 scripts/internal/check/check_rust_private_docs_impl.py \
        --label "$label" \
        --source "$source" \
        --head-ref "$head_ref" \
        --allowlist "$ALLOWLIST_PATH"
}

status=0

if [[ -n "$BASE_REF" ]] && git_has_commit "$BASE_REF" && git_has_commit "$HEAD_REF"; then
  scan_diff "range $BASE_REF...$HEAD_REF" "commit" "$HEAD_REF" "$BASE_REF...$HEAD_REF" || status=1
fi

scan_diff "staged" "index" "" --cached || status=1
scan_diff "unstaged" "worktree" "" || status=1

if (( status != 0 )); then
  exit 1
fi

echo "[private_docs] OK"
exit 0
