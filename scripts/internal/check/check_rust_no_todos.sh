#!/usr/bin/env bash

# Enforces a lightweight structural invariant:
# - no new TODO/FIXME markers in non-test Rust code
#
# The check is diff-aware: it inspects only added lines in diffs. This prevents
# legacy debt from blocking the rule introduction.
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
Usage: scripts/internal/check/check_rust_no_todos.sh [--base <ref>] [--head <ref>]

Fails when added lines introduce TODO/FIXME markers in non-test Rust sources.

Allowlist file:
  scripts/internal/check/allowlists/rust_no_todos_allowlist.txt
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
      echo "[no_todos] Unknown argument: $1" >&2
      usage >&2
      exit 2 ;;
  esac
done

ALLOWLIST_PATH="$ROOT_DIR/scripts/internal/check/allowlists/rust_no_todos_allowlist.txt"

git_has_commit() {
  wavecrate_git rev-parse --verify --quiet "$1^{commit}" >/dev/null 2>&1
}

is_allowlisted() {
  local file="$1"
  local line
  [[ -f "$ALLOWLIST_PATH" ]] || return 1
  while IFS= read -r line || [[ -n "$line" ]]; do
    [[ -z "$line" ]] && continue
    [[ "$line" == \#* ]] && continue
    [[ "$line" == "$file" ]] && return 0
  done < "$ALLOWLIST_PATH"
  return 1
}

is_testish_path() {
  local file="$1"
  [[ "$file" == *"/tests/"* ]] && return 0
  [[ "$file" == tests/* ]] && return 0
  [[ "$file" == *"/benches/"* ]] && return 0
  [[ "$file" == benches/* ]] && return 0
  [[ "$file" == *"_test.rs" ]] && return 0
  [[ "$file" == *"_tests.rs" ]] && return 0
  return 1
}

should_check_file() {
  local file="$1"
  [[ "$file" == src/* ]] && [[ "$file" == *.rs ]] && return 0
  return 1
}

scan_diff_stream() {
  local label="$1"
  local violations=0
  local current=""

  while IFS= read -r line; do
    case "$line" in
      "+++ b/"*)
        current="${line#+++ b/}"
        ;;
      "+"*)
        [[ "$line" == "+++"* ]] && continue
        [[ -z "$current" ]] && continue
        should_check_file "$current" || continue
        is_allowlisted "$current" && continue
        is_testish_path "$current" && continue

        text="${line#+}"
        if [[ "$text" =~ \bTODO\b ]] || [[ "$text" =~ \bFIXME\b ]]; then
          if (( violations == 0 )); then
            echo "[no_todos] Violations detected ($label):" >&2
            echo "[no_todos] Avoid landing TODO/FIXME markers in non-test Rust." >&2
            echo "[no_todos] Preferred: file an issue, add context to docs/plans/, or implement the fix now." >&2
            echo "[no_todos] Allowlist (last resort): $ALLOWLIST_PATH" >&2
          fi
          echo " - $current: $text" >&2
          violations=$((violations + 1))
        fi
        ;;
    esac
  done

  if (( violations > 0 )); then
    return 1
  fi
  return 0
}

scan_git_diff() {
  local label="$1"
  shift
  wavecrate_git diff --unified=0 --diff-filter=AMR "$@" -- src \
    | scan_diff_stream "$label"
}

status=0

if [[ -n "$BASE_REF" ]] && git_has_commit "$BASE_REF" && git_has_commit "$HEAD_REF"; then
  scan_git_diff "range $BASE_REF...$HEAD_REF" "$BASE_REF...$HEAD_REF" || status=1
fi

scan_git_diff "staged" --cached || status=1
scan_git_diff "unstaged" || status=1

if (( status != 0 )); then
  exit 1
fi

echo "[no_todos] OK"
exit 0
