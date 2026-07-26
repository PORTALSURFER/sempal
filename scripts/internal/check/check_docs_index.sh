#!/usr/bin/env bash

# Ensures `docs/README.md` remains a reliable system-of-record landing page.
#
# Checks:
# - Required docs are referenced by path in `docs/README.md`
# - Any `docs/*.md` path referenced in `docs/README.md` exists on disk
# - `docs/README.md` indicates that planning/backlog live in Linear

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT_DIR"

DOCS_README="docs/README.md"

required=(
  "docs/ENV_VARS.md"
  "docs/TEST.md"
  "docs/TARGET.md"
  "docs/TROUBLESHOOTING.md"
)
required_phrases=(
  'Linear project `Wavecrate` under team `PORTALSURFER`'
)

if [[ ! -f "$DOCS_README" ]]; then
  echo "[docs_index] Missing $DOCS_README" >&2
  exit 1
fi

missing_refs=0
for path in "${required[@]}" "${required_phrases[@]}"; do
  if command -v rg >/dev/null 2>&1; then
    found_path=0
    rg -F -q "$path" "$DOCS_README" || found_path=1
  else
    found_path=0
    grep -F -q "$path" "$DOCS_README" || found_path=1
  fi
  if (( found_path != 0 )); then
    if (( missing_refs == 0 )); then
      echo "[docs_index] docs/README.md is missing required references:" >&2
    fi
    echo " - $path" >&2
    missing_refs=$((missing_refs + 1))
  fi
done

for path in "${required[@]}"; do
  if [[ ! -f "$path" ]]; then
    echo "[docs_index] Required file missing on disk: $path" >&2
    exit 1
  fi
done

if (( missing_refs > 0 )); then
  exit 1
fi

python3 - "$DOCS_README" <<'PY'
import re
import sys
from pathlib import Path

readme = Path(sys.argv[1])
text = readme.read_text(encoding="utf-8")

paths = sorted(set(re.findall(r"\bdocs/[A-Za-z0-9._/-]+\.md\b", text)))
missing = [p for p in paths if not Path(p).is_file()]
if missing:
    print("[docs_index] docs/README.md references missing files:", file=sys.stderr)
    for p in missing:
        print(f" - {p}", file=sys.stderr)
    sys.exit(1)

legacy_plan_refs = [
    "docs/plans/index.md",
    "docs/plans/TEMPLATE_execution_plan.md",
    "docs/plans/TEMPLATE_investigation.md",
    "docs/plans/active/todo.md",
    "tmp/improvement_audit_plan.md",
]
legacy_hits = [p for p in legacy_plan_refs if p in text]
if legacy_hits:
    print(
        "[docs_index] docs/README.md should not present Markdown plan files as live workflow entrypoints:",
        file=sys.stderr,
    )
    for path in legacy_hits:
        print(f" - {path}", file=sys.stderr)
    sys.exit(1)

print(f"[docs_index] OK ({len(paths)} referenced docs files)")
PY
