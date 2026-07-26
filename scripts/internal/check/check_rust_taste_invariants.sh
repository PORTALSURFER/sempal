#!/usr/bin/env bash

# Enforces lightweight Rust “taste invariants” to keep changes agent-friendly:
# - no `dbg!(...)` or `println!(...)` in non-test code (use `tracing` instead)
# - no `.unwrap()` or `.expect(...)` in non-test code (propagate errors)
#
# The check is diff-aware: it inspects only added lines in diffs. This prevents
# legacy code from blocking the rule introduction.
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
Usage: scripts/internal/check/check_rust_taste_invariants.sh [--base <ref>] [--head <ref>]

Fails when added lines introduce forbidden patterns in non-test Rust sources:
- `dbg!(...)`
- `println!(...)`
- `.unwrap()`
- `.expect(...)`

Allowlist file:
  scripts/internal/check/allowlists/rust_taste_invariants_allowlist.txt
EOF
}

while (( $# > 0 )); do
  case "$1" in
    --base)
      if [[ $# -lt 2 || -z "${2:-}" ]]; then
        echo "[taste] --base requires a value." >&2
        usage >&2
        exit 2
      fi
      BASE_REF="${2:-}"; shift 2 ;;
    --head)
      if [[ $# -lt 2 || -z "${2:-}" ]]; then
        echo "[taste] --head requires a value." >&2
        usage >&2
        exit 2
      fi
      HEAD_REF="${2:-}"; shift 2 ;;
    -h|--help)
      usage; exit 0 ;;
    *)
      echo "[taste] Unknown argument: $1" >&2
      usage >&2
      exit 2 ;;
  esac
done

ALLOWLIST_PATH="$ROOT_DIR/scripts/internal/check/allowlists/rust_taste_invariants_allowlist.txt"

git_has_commit() {
  wavecrate_git rev-parse --verify --quiet "$1^{commit}" >/dev/null 2>&1
}

is_allowlisted() {
  local file="$1"
  [[ -f "$ALLOWLIST_PATH" ]] || return 1
  grep -Fxq -- "$file" <(
    grep -Ev '^[[:space:]]*($|#)' "$ALLOWLIST_PATH" || true
  )
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

        # Ignore pure comment lines to avoid false positives in docs/comments.
        if echo "$text" | grep -Eq '^[[:space:]]*//'; then
          continue
        fi

        if echo "$text" | grep -Fq 'dbg!('; then
          if (( violations == 0 )); then
            echo "[taste] Violations detected ($label):" >&2
            echo "[taste] Use tracing instead of dbg!/println! in non-test code." >&2
            echo "[taste] Avoid .unwrap()/.expect(...) in non-test code; propagate errors." >&2
            echo "[taste] Allowlist (last resort): $ALLOWLIST_PATH" >&2
          fi
          echo " - $current: dbg!: $text" >&2
          violations=$((violations + 1))
        fi
        if echo "$text" | grep -Fq 'println!('; then
          if (( violations == 0 )); then
            echo "[taste] Violations detected ($label):" >&2
            echo "[taste] Use tracing instead of dbg!/println! in non-test code." >&2
            echo "[taste] Avoid .unwrap()/.expect(...) in non-test code; propagate errors." >&2
            echo "[taste] Allowlist (last resort): $ALLOWLIST_PATH" >&2
          fi
          echo " - $current: println!: $text" >&2
          violations=$((violations + 1))
        fi
        if echo "$text" | grep -Fq '.unwrap('; then
          if (( violations == 0 )); then
            echo "[taste] Violations detected ($label):" >&2
            echo "[taste] Use tracing instead of dbg!/println! in non-test code." >&2
            echo "[taste] Avoid .unwrap()/.expect(...) in non-test code; propagate errors." >&2
            echo "[taste] Allowlist (last resort): $ALLOWLIST_PATH" >&2
          fi
          echo " - $current: unwrap(): $text" >&2
          violations=$((violations + 1))
        fi
        if echo "$text" | grep -Fq '.expect('; then
          if (( violations == 0 )); then
            echo "[taste] Violations detected ($label):" >&2
            echo "[taste] Use tracing instead of dbg!/println! in non-test code." >&2
            echo "[taste] Avoid .unwrap()/.expect(...) in non-test code; propagate errors." >&2
            echo "[taste] Allowlist (last resort): $ALLOWLIST_PATH" >&2
          fi
          echo " - $current: expect(): $text" >&2
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

echo "[taste] OK"
exit 0
