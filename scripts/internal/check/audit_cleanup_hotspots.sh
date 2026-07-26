#!/usr/bin/env bash

# Generates a deterministic cleanup-hotspot snapshot for planning.
#
# The report includes:
# - largest Rust files (line count)
# - largest function spans (heuristic)
# - files still over the file-size budget limit
# - dead-code and clippy::too_many_arguments suppression density
# - likely test-gap hotspots (large files without local test modules)
#   excluding dedicated test paths like `tests/**`, `*_test.rs`, `*_tests.rs`,
#   and `tests.rs`, plus sibling module tests declared via `mod.rs` + `tests.rs`
#
# Output defaults to `tmp/cleanup_audit_hotspots.md`.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT_DIR"

OUTPUT_PATH="tmp/cleanup_audit_hotspots.md"
TOP_FILES=20
TOP_SUPPRESSIONS=20
TEST_GAP_MIN_LINES=200
FILE_SIZE_LIMIT=400
TOP_FUNCTION_SPANS=20

usage() {
  cat <<'USAGE'
Usage: scripts/check.sh cleanup-hotspots [--output <path>] [--top-files <n>] [--top-suppressions <n>] [--top-function-spans <n>] [--test-gap-min-lines <n>] [--file-size-limit <n>]

Generate a deterministic markdown snapshot of cleanup hotspots.

Options:
  --output <path>            Output markdown path (default: tmp/cleanup_audit_hotspots.md)
  --top-files <n>            Number of largest files to show (default: 20)
  --top-suppressions <n>     Number of files to show for each suppression table (default: 20)
  --top-function-spans <n>   Number of largest function spans to show (default: 20)
  --test-gap-min-lines <n>   Minimum file size for test-gap heuristic (default: 200)
  --file-size-limit <n>      File-size budget threshold in lines (default: 400)
  -h, --help                 Show help
USAGE
}

is_non_negative_integer() {
  [[ "$1" =~ ^[0-9]+$ ]]
}

while (( $# > 0 )); do
  case "$1" in
    --output)
      OUTPUT_PATH="${2:-}"
      shift 2
      ;;
    --top-files)
      TOP_FILES="${2:-}"
      shift 2
      ;;
    --top-suppressions)
      TOP_SUPPRESSIONS="${2:-}"
      shift 2
      ;;
    --top-function-spans)
      TOP_FUNCTION_SPANS="${2:-}"
      shift 2
      ;;
    --test-gap-min-lines)
      TEST_GAP_MIN_LINES="${2:-}"
      shift 2
      ;;
    --file-size-limit)
      FILE_SIZE_LIMIT="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "[cleanup_audit] Unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

for value_name in TOP_FILES TOP_SUPPRESSIONS TOP_FUNCTION_SPANS TEST_GAP_MIN_LINES FILE_SIZE_LIMIT; do
  value="${!value_name}"
  if ! is_non_negative_integer "$value"; then
    echo "[cleanup_audit] $value_name must be a non-negative integer (got: $value)" >&2
    exit 2
  fi
done

if [[ -z "$OUTPUT_PATH" ]]; then
  echo "[cleanup_audit] --output requires a non-empty path" >&2
  exit 2
fi

mkdir -p "$(dirname "$OUTPUT_PATH")"

tmp_line_counts="$(mktemp)"
tmp_over_limit="$(mktemp)"
tmp_dead_hits="$(mktemp)"
tmp_tma_hits="$(mktemp)"
tmp_dead_counts="$(mktemp)"
tmp_tma_counts="$(mktemp)"
tmp_test_gaps="$(mktemp)"
tmp_function_spans="$(mktemp)"

cleanup() {
  rm -f \
    "$tmp_line_counts" \
    "$tmp_over_limit" \
    "$tmp_dead_hits" \
    "$tmp_tma_hits" \
    "$tmp_dead_counts" \
    "$tmp_tma_counts" \
    "$tmp_test_gaps" \
    "$tmp_function_spans"
}
trap cleanup EXIT

collect_rust_files() {
  git ls-files -- '*.rs'
}

is_dedicated_test_path() {
  local file="$1"
  [[ "$file" == */tests/* || "$file" == tests/* || "$file" == *_test.rs || "$file" == *_tests.rs || "$file" == */tests.rs || "$file" == tests.rs ]]
}

has_local_test_markers() {
  local file="$1"
  grep -Eq '^[[:space:]]*#[[:space:]]*\[cfg\(test\)\]|^[[:space:]]*mod[[:space:]]+tests\b' "$file"
}

has_sibling_module_tests() {
  local file="$1"
  local dir
  dir="$(dirname "$file")"
  [[ "$dir" != "." ]] || return 1

  local tests_file="$dir/tests.rs"
  local mod_file="$dir/mod.rs"
  [[ -f "$tests_file" && -f "$mod_file" ]] || return 1

  has_local_test_markers "$mod_file"
}

rust_files=()
while IFS= read -r rust_file; do
  rust_files+=("$rust_file")
done < <(collect_rust_files | LC_ALL=C sort -u)

for file in "${rust_files[@]}"; do
  if [[ ! -f "$file" ]]; then
    continue
  fi
  line_count="$(wc -l <"$file" | tr -d '[:space:]')"
  printf "%s\t%s\n" "$line_count" "$file" >>"$tmp_line_counts"
  if (( line_count > FILE_SIZE_LIMIT )); then
    printf "%s\t%s\n" "$line_count" "$file" >>"$tmp_over_limit"
  fi

  awk -v file_path="$file" '
    function flush_span(next_line) {
      if (!in_fn) {
        return
      }
      span = next_line - fn_start
      if (span < 1) {
        span = 1
      }
      printf "%d\t%s:%d\t%s\n", span, file_path, fn_start, fn_name
    }
    {
      line_no = NR
      if ($0 ~ /^[[:space:]]*(pub(\([^)]*\))?[[:space:]]+)?(async[[:space:]]+)?fn[[:space:]]+[A-Za-z_][A-Za-z0-9_]*/) {
        flush_span(line_no)
        in_fn = 1
        fn_start = line_no
        fn_name = $0
        sub(/^.*fn[[:space:]]+/, "", fn_name)
        sub(/[^A-Za-z0-9_].*$/, "", fn_name)
        if (fn_name != "") {
          fn_name = fn_name
        } else {
          fn_name = "<unknown>"
        }
      }
    }
    END {
      if (in_fn) {
        flush_span(NR + 1)
      }
    }
  ' "$file" >>"$tmp_function_spans"

done

rg -n --no-heading --glob '*.rs' '^\s*#\s*\[allow\([^]]*dead_code[^]]*\)\]' >"$tmp_dead_hits" || true
if [[ -s "$tmp_dead_hits" ]]; then
  awk -F: '{counts[$1]++} END {for (file in counts) printf "%d\t%s\n", counts[file], file}' "$tmp_dead_hits" \
    | LC_ALL=C sort -t$'\t' -k1,1nr -k2,2 >"$tmp_dead_counts"
fi

rg -n --no-heading --glob '*.rs' '^\s*#\s*\[allow\([^]]*clippy::too_many_arguments[^]]*\)\]' >"$tmp_tma_hits" || true
if [[ -s "$tmp_tma_hits" ]]; then
  awk -F: '{counts[$1]++} END {for (file in counts) printf "%d\t%s\n", counts[file], file}' "$tmp_tma_hits" \
    | LC_ALL=C sort -t$'\t' -k1,1nr -k2,2 >"$tmp_tma_counts"
fi

while IFS=$'\t' read -r line_count file; do
  if (( line_count < TEST_GAP_MIN_LINES )); then
    continue
  fi
  if is_dedicated_test_path "$file"; then
    continue
  fi
  if has_local_test_markers "$file"; then
    continue
  fi
  if has_sibling_module_tests "$file"; then
    continue
  fi
  printf "%s\t%s\n" "$line_count" "$file" >>"$tmp_test_gaps"
done < <(LC_ALL=C sort -t$'\t' -k1,1nr -k2,2 "$tmp_line_counts")

timestamp_utc="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
git_branch="$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo unknown)"
git_commit="$(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
total_rust_files="$(wc -l <"$tmp_line_counts" | tr -d '[:space:]')"
over_budget_count="$(wc -l <"$tmp_over_limit" | tr -d '[:space:]')"
dead_supp_files="$(wc -l <"$tmp_dead_counts" | tr -d '[:space:]')"
tma_supp_files="$(wc -l <"$tmp_tma_counts" | tr -d '[:space:]')"
test_gap_count="$(wc -l <"$tmp_test_gaps" | tr -d '[:space:]')"
function_span_count="$(wc -l <"$tmp_function_spans" | tr -d '[:space:]')"

count_rows_for_scope() {
  local path="$1"
  local scope="$2"
  local file_column="$3"
  awk -F'\t' -v scope="$scope" -v file_column="$file_column" '
    function in_scope(file) {
      if (scope == "vendor") {
        return file ~ /^vendor\/radiant\//
      }
      return file !~ /^vendor\/radiant\//
    }
    in_scope($file_column) { count++ }
    END { print count + 0 }
  ' "$path"
}

emit_line_table_for_scope() {
  local path="$1"
  local scope="$2"
  local limit="$3"
  awk -F'\t' -v scope="$scope" -v limit="$limit" '
    function in_scope(file) {
      if (scope == "vendor") {
        return file ~ /^vendor\/radiant\//
      }
      return file !~ /^vendor\/radiant\//
    }
    BEGIN {
      emitted = 0
      seen = 0
    }
    in_scope($2) {
      seen++
      if (limit == 0 || seen <= limit) {
        if (emitted == 0) {
          print "| Lines | File |"
          print "| ---: | --- |"
        }
        printf "| %s | `%s` |\n", $1, $2
        emitted++
      }
    }
    END {
      if (seen == 0) {
        print "None."
      }
    }
  ' "$path"
}

emit_count_table_for_scope() {
  local path="$1"
  local scope="$2"
  local limit="$3"
  awk -F'\t' -v scope="$scope" -v limit="$limit" '
    function in_scope(file) {
      if (scope == "vendor") {
        return file ~ /^vendor\/radiant\//
      }
      return file !~ /^vendor\/radiant\//
    }
    BEGIN {
      emitted = 0
      seen = 0
    }
    in_scope($2) {
      seen++
      if (limit == 0 || seen <= limit) {
        if (emitted == 0) {
          print "| Occurrences | File |"
          print "| ---: | --- |"
        }
        printf "| %s | `%s` |\n", $1, $2
        emitted++
      }
    }
    END {
      if (seen == 0) {
        print "None."
      }
    }
  ' "$path"
}

emit_function_table_for_scope() {
  local path="$1"
  local scope="$2"
  local limit="$3"
  awk -F'\t' -v scope="$scope" -v limit="$limit" '
    function in_scope(file) {
      if (scope == "vendor") {
        return file ~ /^vendor\/radiant\//
      }
      return file !~ /^vendor\/radiant\//
    }
    BEGIN {
      emitted = 0
      seen = 0
    }
    in_scope($2) {
      seen++
      if (limit == 0 || seen <= limit) {
        if (emitted == 0) {
          print "| Span (lines) | Function |"
          print "| ---: | --- |"
        }
        printf "| %s | `%s` (`%s`) |\n", $1, $3, $2
        emitted++
      }
    }
    END {
      if (seen == 0) {
        print "None."
      }
    }
  ' "$path"
}

root_rust_files="$(count_rows_for_scope "$tmp_line_counts" root 2)"
vendor_rust_files="$(count_rows_for_scope "$tmp_line_counts" vendor 2)"
root_over_budget_count="$(count_rows_for_scope "$tmp_over_limit" root 2)"
vendor_over_budget_count="$(count_rows_for_scope "$tmp_over_limit" vendor 2)"

{
  echo "# Cleanup Hotspot Audit Snapshot"
  echo
  echo "- Generated (UTC): \`$timestamp_utc\`"
  echo "- Branch: \`$git_branch\`"
  echo "- Commit: \`$git_commit\`"
  echo "- Rust files scanned: $total_rust_files"
  echo "- File-size budget limit: \`$FILE_SIZE_LIMIT\`"
  echo
  echo "## Summary"
  echo
  echo "- Over file-size budget: $over_budget_count"
  echo "- Function spans captured: $function_span_count"
  echo "- Files with \`dead_code\` suppressions: $dead_supp_files"
  echo "- Files with \`clippy::too_many_arguments\` suppressions: $tma_supp_files"
  echo "- Likely large-file test-gap hotspots (heuristic): $test_gap_count"
  echo "- Wavecrate-root Rust files scanned: $root_rust_files"
  echo "- Radiant sibling Rust files scanned separately: $vendor_rust_files"
  echo "- Wavecrate-root files over budget: $root_over_budget_count"
  echo "- Radiant sibling files over budget (standalone gate): $vendor_over_budget_count"
  echo

  echo "## Wavecrate-root largest Rust files"
  echo
  LC_ALL=C sort -t$'\t' -k1,1nr -k2,2 "$tmp_line_counts" \
    | emit_line_table_for_scope /dev/stdin root "$TOP_FILES"
  echo

  echo "## Radiant sibling largest Rust files (standalone gate)"
  echo
  LC_ALL=C sort -t$'\t' -k1,1nr -k2,2 "$tmp_line_counts" \
    | emit_line_table_for_scope /dev/stdin vendor "$TOP_FILES"
  echo

  echo "## Wavecrate-root largest function spans (heuristic)"
  echo
  LC_ALL=C sort -t$'\t' -k1,1nr -k2,2 "$tmp_function_spans" \
    | emit_function_table_for_scope /dev/stdin root "$TOP_FUNCTION_SPANS"
  echo

  echo "## Radiant sibling largest function spans (standalone gate)"
  echo
  LC_ALL=C sort -t$'\t' -k1,1nr -k2,2 "$tmp_function_spans" \
    | emit_function_table_for_scope /dev/stdin vendor "$TOP_FUNCTION_SPANS"
  echo

  echo "## Wavecrate-root files over budget"
  echo
  LC_ALL=C sort -t$'\t' -k1,1nr -k2,2 "$tmp_over_limit" \
    | emit_line_table_for_scope /dev/stdin root 0
  echo

  echo "## Radiant sibling files over budget (standalone gate)"
  echo
  LC_ALL=C sort -t$'\t' -k1,1nr -k2,2 "$tmp_over_limit" \
    | emit_line_table_for_scope /dev/stdin vendor 0
  echo

  echo "## Wavecrate-root dead_code suppression density"
  echo
  emit_count_table_for_scope "$tmp_dead_counts" root "$TOP_SUPPRESSIONS"
  echo

  echo "## Radiant sibling dead_code suppression density (standalone gate)"
  echo
  emit_count_table_for_scope "$tmp_dead_counts" vendor "$TOP_SUPPRESSIONS"
  echo

  echo "## Wavecrate-root too_many_arguments suppression density"
  echo
  emit_count_table_for_scope "$tmp_tma_counts" root "$TOP_SUPPRESSIONS"
  echo

  echo "## Radiant sibling too_many_arguments suppression density (standalone gate)"
  echo
  emit_count_table_for_scope "$tmp_tma_counts" vendor "$TOP_SUPPRESSIONS"
  echo

  echo "## Wavecrate-root likely test-gap hotspots (heuristic)"
  echo
  echo "Files with at least \`$TEST_GAP_MIN_LINES\` lines and no local \`#[cfg(test)]\` or \`mod tests\` marker."
  echo "Skips dedicated test modules/paths (\`tests/**\`, \`tests.rs\`, \`*_test.rs\`, \`*_tests.rs\`) and sibling module tests declared through \`mod.rs\` + \`tests.rs\`."
  echo
  emit_line_table_for_scope "$tmp_test_gaps" root "$TOP_FILES"
  echo

  echo "## Radiant sibling likely test-gap hotspots (standalone gate)"
  echo
  echo "Files with at least \`$TEST_GAP_MIN_LINES\` lines and no local \`#[cfg(test)]\` or \`mod tests\` marker."
  echo "Skips dedicated test modules/paths (\`tests/**\`, \`tests.rs\`, \`*_test.rs\`, \`*_tests.rs\`) and sibling module tests declared through \`mod.rs\` + \`tests.rs\`."
  echo
  emit_line_table_for_scope "$tmp_test_gaps" vendor "$TOP_FILES"
  echo

  echo "## Suggested follow-up"
  echo
  echo "1. Triage Wavecrate-root candidates separately from the standalone Radiant repository."
  echo "2. Remove or test-gate high-density suppressions after each refactor slice."
  echo "3. Add focused tests for top heuristic gaps where behavior is non-trivial."
} >"$OUTPUT_PATH"

echo "[cleanup_audit] wrote $OUTPUT_PATH"
