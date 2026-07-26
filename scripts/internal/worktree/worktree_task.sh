#!/usr/bin/env bash

# One-change-per-worktree harness.
#
# Creates a new git worktree, runs the golden-path setup/checks, and can
# optionally launch the app in the sandbox (blocking).

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT_DIR"

NAME=""
BASE_REF="HEAD"
WORKTREE_PATH=""
SKIP_CI=0
NO_RUN=0
RUN_TEMP=0

usage() {
  cat <<'EOF'
Usage: scripts/internal/worktree/worktree_task.sh --name <id> [--base <ref>] [--path <dir>] [--skip-ci] [--no-run] [--run-temp] [--] [app args...]

Creates a worktree and runs:
- scripts/bootstrap.sh --verify-only
- scripts/ci.sh local (unless --skip-ci)

Then (unless --no-run) launches:
- scripts/run.sh sandbox (persistent by default)
  Use --run-temp to run with `--temp` (ephemeral; deleted on exit).

Defaults:
- base ref: HEAD
- worktree path: <repo>/.worktrees/<id>
EOF
}

sanitize_branch() {
  # Conservative: lowercase, replace spaces with '-', strip weird chars.
  printf "%s" "$1" \
    | tr '[:upper:]' '[:lower:]' \
    | tr ' ' '-' \
    | sed -E 's#[^a-z0-9._/-]+#-#g; s#-+#-#g; s#^[-/]+##; s#[-/]+$##'
}

while (( $# > 0 )); do
  case "$1" in
    --name)
      NAME="${2:-}"; shift 2 ;;
    --base)
      BASE_REF="${2:-}"; shift 2 ;;
  --path)
      WORKTREE_PATH="${2:-}"; shift 2 ;;
    --skip-ci)
      SKIP_CI=1; shift ;;
    --no-run)
      NO_RUN=1; shift ;;
    --run-temp)
      RUN_TEMP=1; shift ;;
    --)
      shift; break ;;
    -h|--help)
      usage; exit 0 ;;
    *)
      echo "[worktree_task] Unknown argument: $1" >&2
      usage >&2
      exit 2 ;;
  esac
done

if [[ -z "$NAME" ]]; then
  echo "[worktree_task] ERROR: --name is required" >&2
  usage >&2
  exit 2
fi

if [[ -z "$WORKTREE_PATH" ]]; then
  WORKTREE_PATH="$ROOT_DIR/.worktrees/$NAME/wavecrate"
fi

PAIR_ROOT="$(dirname "$WORKTREE_PATH")"
RADIANT_PATH="$PAIR_ROOT/radiant"

if ! command -v git >/dev/null 2>&1; then
  echo "[worktree_task] ERROR: git not found on PATH" >&2
  exit 1
fi

branch_id="$(sanitize_branch "$NAME")"
if [[ -z "$branch_id" ]]; then
  echo "[worktree_task] ERROR: invalid --name: $NAME" >&2
  exit 2
fi
branch="task/${branch_id}"

if [[ -e "$WORKTREE_PATH" ]]; then
  echo "[worktree_task] ERROR: worktree path already exists: $WORKTREE_PATH" >&2
  exit 1
fi
if [[ -e "$RADIANT_PATH" ]]; then
  echo "[worktree_task] ERROR: paired Radiant path already exists: $RADIANT_PATH" >&2
  exit 1
fi

echo "[worktree_task] Creating worktree:"
echo "[worktree_task]   branch=$branch"
echo "[worktree_task]   base=$BASE_REF"
echo "[worktree_task]   path=$WORKTREE_PATH"
git worktree add -b "$branch" "$WORKTREE_PATH" "$BASE_REF"

echo "[worktree_task] Provisioning isolated Radiant sibling: $RADIANT_PATH"
(cd "$WORKTREE_PATH" && bash scripts/radiant.sh provision --clean --path "$RADIANT_PATH")

echo "[worktree_task] Running bootstrap verification..."
(cd "$WORKTREE_PATH" && bash scripts/bootstrap.sh --verify-only)

if (( SKIP_CI == 0 )); then
  echo "[worktree_task] Running CI parity checks..."
  (cd "$WORKTREE_PATH" && bash scripts/ci.sh local)
else
  echo "[worktree_task] Skipping CI parity checks (--skip-ci)."
fi

echo "[worktree_task] Worktree ready: $WORKTREE_PATH"
echo "[worktree_task] Tip: remove when done: git worktree remove \"$WORKTREE_PATH\""

if (( NO_RUN == 1 )); then
  echo "[worktree_task] Not launching app (--no-run)."
  exit 0
fi

echo "[worktree_task] Launching app in sandbox. Close the app to return."
if (( RUN_TEMP == 1 )); then
  (cd "$WORKTREE_PATH" && bash scripts/run.sh sandbox --temp -- "$@")
else
  (cd "$WORKTREE_PATH" && bash scripts/run.sh sandbox -- "$@")
fi
