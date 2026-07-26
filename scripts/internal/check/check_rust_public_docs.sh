#!/usr/bin/env bash

# Enforces a documentation-quality invariant:
# - newly added `pub` items (fn/struct/enum/trait/type/const/static) in non-test
#   Rust code must have nearby doc comments (`///` or `#[doc = ...]`)
#
# The check is diff-aware: it inspects only added lines in diffs, but it
# validates doc-comment presence using the corresponding "b-side" file content:
# - range diffs use `git show <head>:<path>`
# - staged diffs use `git show :<path>` (index)
# - unstaged diffs read the working tree
#
# Scope:
# - Rust files under `src/`
# - Skips obvious test/bench paths and allowlisted files

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
  cat <<'EOF'
Usage: scripts/internal/check/check_rust_public_docs.sh [--base <ref>] [--head <ref>]

Fails when added lines introduce public Rust items without doc comments nearby.

Allowlist file:
  scripts/internal/check/allowlists/rust_public_docs_allowlist.txt
EOF
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
      echo "[public_docs] Unknown argument: $1" >&2
      usage >&2
      exit 2 ;;
  esac
done

if ! command -v python3 >/dev/null 2>&1; then
  echo "[public_docs] ERROR: python3 is required for this check" >&2
  exit 2
fi

ALLOWLIST_PATH="$ROOT_DIR/scripts/internal/check/allowlists/rust_public_docs_allowlist.txt"

git_has_commit() {
  wavecrate_git rev-parse --verify --quiet "$1^{commit}" >/dev/null 2>&1
}

scan_diff() {
  local label="$1"
  local source="$2"   # worktree | index | commit
  local head_ref="$3" # only used when source=commit
  shift 3

  wavecrate_git diff --unified=0 --diff-filter=AMR "$@" -- src \
    | python3 scripts/internal/check/check_rust_public_docs_impl.py \
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

echo "[public_docs] OK"
exit 0
