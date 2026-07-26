#!/usr/bin/env bash

# Report-only line-count guardrail for scripts/internal. This intentionally exits
# successfully so existing debt is visible
# without blocking incremental cleanup work.

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

LIMIT=400
TOP_FILES=20

usage() {
  cat <<'EOF'
Usage: scripts/internal/check/report_size_hotspots.sh [--limit <n>] [--top-files <n>]

Prints a report-only line-count snapshot for scripts/internal.
EOF
}

while (( $# > 0 )); do
  case "$1" in
    --limit)
      [[ $# -ge 2 && "${2:-}" =~ ^[0-9]+$ ]] || {
        echo "[size_hotspots] --limit requires a positive integer." >&2
        exit 2
      }
      LIMIT="$2"; shift 2 ;;
    --top-files)
      [[ $# -ge 2 && "${2:-}" =~ ^[0-9]+$ ]] || {
        echo "[size_hotspots] --top-files requires a positive integer." >&2
        exit 2
      }
      TOP_FILES="$2"; shift 2 ;;
    -h|--help)
      usage; exit 0 ;;
    *)
      echo "[size_hotspots] Unknown argument: $1" >&2
      usage >&2
      exit 2 ;;
  esac
done

scope_for_file() {
  local file="$1"
  case "$file" in
    scripts/internal/*) echo "scripts/internal" ;;
    *) echo "other" ;;
  esac
}

collect_files() {
  wavecrate_git ls-files -- scripts/internal \
    | grep -E '\.(ps1|sh|py|json|cmd)$' || true

}

rows_file="$(mktemp)"
trap 'rm -f "$rows_file"' EXIT

while IFS= read -r file; do
  [[ -n "$file" && -f "$file" ]] || continue
  lines="$(wc -l <"$file" | tr -d '[:space:]')"
  scope="$(scope_for_file "$file")"
  printf "%s\t%s\t%s\n" "$lines" "$scope" "$file"
done < <(collect_files | sort -u) | sort -rn >"$rows_file"

total="$(wc -l <"$rows_file" | tr -d '[:space:]')"
over="$(awk -F '\t' -v limit="$LIMIT" '$1 > limit { count++ } END { print count + 0 }' "$rows_file")"
timestamp_utc="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"

echo "# Size Hotspot Report"
echo
echo "- Timestamp (UTC): \`$timestamp_utc\`"
echo "- Limit: \`$LIMIT\` lines"
echo "- Scopes: \`scripts/internal\`"
echo "- Entries: total=$total over=$over"
echo

echo "## Over Budget"
echo
if (( over > 0 )); then
  echo "| Lines | Scope | File |"
  echo "| ---: | --- | --- |"
  awk -F '\t' -v limit="$LIMIT" -v top="$TOP_FILES" '
    $1 > limit && count < top {
      printf("| %s | %s | `%s` |\n", $1, $2, $3)
      count++
    }
  ' "$rows_file"
else
  echo "None."
fi
echo

echo "## Largest Files"
echo
echo "| Lines | Scope | File |"
echo "| ---: | --- | --- |"
awk -F '\t' -v top="$TOP_FILES" '
  count < top {
    printf("| %s | %s | `%s` |\n", $1, $2, $3)
    count++
  }
' "$rows_file"

exit 0
