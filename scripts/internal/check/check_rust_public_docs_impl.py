#!/usr/bin/env python3

"""
Implementation for scripts/internal/check/check_rust_public_docs.sh.

Reads a `git diff` stream from stdin and fails when newly added `pub` Rust items
are missing nearby doc comments.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path


PUBLIC_ITEM_RE = re.compile(
    r"""
    ^\s*pub\s+(?!\()                      # pub, but not pub(...)
    (?:async\s+|unsafe\s+|extern\s+"[^"]+"\s+)*   # optional qualifiers
    \b(fn|struct|enum|trait|type|const|static)\b
    """,
    re.VERBOSE,
)

HUNK_RE = re.compile(r"@@ .* \+(\d+)(?:,(\d+))? @@")


def is_testish_path(path: str) -> bool:
    return (
        "/tests/" in path
        or path.startswith("tests/")
        or "/benches/" in path
        or path.startswith("benches/")
        or path.endswith("_test.rs")
        or path.endswith("_tests.rs")
    )


def should_check_file(path: str) -> bool:
    if not path.endswith(".rs"):
        return False
    return path.startswith("src/")


def load_allowlist(path: str) -> set[str]:
    p = Path(path)
    if not p.exists():
        return set()
    out: set[str] = set()
    for raw in p.read_text(encoding="utf-8").splitlines():
        t = raw.strip()
        if not t or t.startswith("#"):
            continue
        out.add(t)
    return out


def git_show(spec: str) -> str | None:
    try:
        res = subprocess.run(
            ["git", "show", spec],
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
        )
        return res.stdout
    except Exception:
        return None


def load_file_lines(source: str, head_ref: str, path: str) -> list[str] | None:
    if source == "worktree":
        try:
            return Path(path).read_text(encoding="utf-8").splitlines()
        except Exception:
            return None
    if source == "index":
        txt = git_show(f":{path}")
        return None if txt is None else txt.splitlines()
    if source == "commit":
        if not head_ref:
            return None
        txt = git_show(f"{head_ref}:{path}")
        return None if txt is None else txt.splitlines()
    return None


def has_doc_comment(lines: list[str], item_line_1: int) -> bool:
    idx = item_line_1 - 1
    if idx <= 0 or idx > len(lines):
        return False

    start = max(0, idx - 12)
    for i in range(idx - 1, start - 1, -1):
        s = lines[i].strip()
        if not s:
            continue

        if s.startswith("///"):
            return True
        if s.startswith("/**") or s.startswith("/*!"):
            return True
        if re.match(r"^\s*#\!?\[doc\s*(=|\()", s):
            return True

        if s.startswith("#[") or s.startswith("#!["):
            continue

        # Any other comment/code is a barrier; docs should be adjacent-ish.
        return False

    return False


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--label", required=True)
    ap.add_argument("--source", required=True, choices=["worktree", "index", "commit"])
    ap.add_argument("--head-ref", default="")
    ap.add_argument("--allowlist", required=True)
    args = ap.parse_args()

    allowlist = load_allowlist(args.allowlist)

    current = ""
    new_line = 0
    violations: list[str] = []
    file_cache: dict[str, list[str] | None] = {}

    for raw in sys.stdin.read().splitlines():
        if raw.startswith("+++ b/"):
            current = raw[6:]
            new_line = 0
            continue
        if raw.startswith("@@"):
            m = HUNK_RE.search(raw)
            new_line = int(m.group(1)) if m else 0
            continue
        if not raw.startswith("+") or raw.startswith("+++"):
            continue
        if not current:
            continue
        if not should_check_file(current):
            new_line += 1
            continue
        if current in allowlist:
            new_line += 1
            continue
        if is_testish_path(current):
            new_line += 1
            continue

        text = raw[1:]
        if "pub use" in text:
            new_line += 1
            continue
        if not PUBLIC_ITEM_RE.match(text):
            new_line += 1
            continue

        if current not in file_cache:
            file_cache[current] = load_file_lines(args.source, args.head_ref, current)

        lines = file_cache[current]
        if not lines:
            violations.append(f"{current}:{new_line}: missing file content for doc check")
            new_line += 1
            continue

        if not has_doc_comment(lines, new_line):
            violations.append(f"{current}:{new_line}: {text.strip()}")

        new_line += 1

    if violations:
        sys.stderr.write(f"[public_docs] Violations detected ({args.label}):\n")
        sys.stderr.write("[public_docs] Newly added public items must have doc comments (`///` or `#[doc = ...]`).\n")
        sys.stderr.write(f"[public_docs] Allowlist (last resort): {args.allowlist}\n")
        for v in sorted(violations):
            sys.stderr.write(f" - {v}\n")
        return 1

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
