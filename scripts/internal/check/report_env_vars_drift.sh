#!/usr/bin/env bash

# Reports environment variable documentation drift:
# - env vars used in code but missing from `docs/ENV_VARS.md`
# - env vars documented in `docs/ENV_VARS.md` but not found in code (may be stale)
#
# The report focuses on Wavecrate and build-related env vars:
# - `WAVECRATE_*`
# - `CPAL_ASIO_DIR`
# - `RUST_LOG`

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT_DIR"

DOC_PATH="docs/ENV_VARS.md"

if [[ ! -f "$DOC_PATH" ]]; then
  echo "# Env var docs drift"
  echo
  echo "Missing: \`$DOC_PATH\`"
  exit 0
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

code_vars_path="$tmp_dir/code_vars.txt"
doc_vars_path="$tmp_dir/doc_vars.txt"

(
  rg -o --no-filename "WAVECRATE_[A-Z0-9_]+" src scripts .github 2>/dev/null || true
  rg -o --no-filename "\\bCPAL_ASIO_DIR\\b" README.md manual src scripts .github 2>/dev/null || true
  rg -o --no-filename "\\bRUST_LOG\\b" README.md manual src scripts .github 2>/dev/null || true
) | sort -u >"$code_vars_path"

python3 - "$DOC_PATH" >"$doc_vars_path" <<'PY'
import re
import sys
from pathlib import Path

text = Path(sys.argv[1]).read_text(encoding="utf-8")
names = set()
for m in re.finditer(r"`([A-Z][A-Z0-9_]+)`", text):
    names.add(m.group(1))

focus = {n for n in names if n.startswith("WAVECRATE_") or n in {"CPAL_ASIO_DIR", "RUST_LOG"}}
for n in sorted(focus):
    print(n)
PY

timestamp="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

python3 - "$code_vars_path" "$doc_vars_path" "$timestamp" <<'PY'
import sys
from pathlib import Path

code_vars = {line.strip() for line in Path(sys.argv[1]).read_text(encoding="utf-8").splitlines() if line.strip()}
doc_vars = {line.strip() for line in Path(sys.argv[2]).read_text(encoding="utf-8").splitlines() if line.strip()}
timestamp = sys.argv[3]

undocumented = sorted(code_vars - doc_vars)
stale = sorted(doc_vars - code_vars)

print("# Env var docs drift")
print()
print(f"- Timestamp (UTC): `{timestamp}`")
print(f"- Code vars found: `{len(code_vars)}`")
print(f"- Documented vars found: `{len(doc_vars)}`")
print()

print("## Used in code but missing from docs/ENV_VARS.md")
print()
if undocumented:
    for name in undocumented:
        print(f"- `{name}`")
else:
    print("None.")
print()

print("## Documented in docs/ENV_VARS.md but not found in code")
print()
if stale:
    for name in stale:
        print(f"- `{name}`")
else:
    print("None.")
PY
