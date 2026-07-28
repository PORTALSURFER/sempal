#!/usr/bin/env bash

# Install local git hooks that keep Wavecrate on its main-integration workflow
# and run bounded repository-state checks after
# branch/source updates.
#
# Hooks installed for wavecrate:
# - post-checkout: run bounded repository-state checks
# - pre-commit / pre-push: verify local `main` tracks `origin/main`; feature branches are allowed for PR work
#
# To temporarily disable hook execution, set WAVECRATE_SKIP_AGENT_PREFLIGHT_HOOK=1.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT_DIR"

FORCE=0

usage() {
  cat <<'USAGE'
Usage: scripts/internal/agent/install_agent_preflight_hooks.sh [--force]

Install local git hooks that keep Wavecrate aligned with its main-integration
workflow and run bounded repository-state checks after repo-level source updates.
Dependency updates are controlled by Cargo.toml/Cargo.lock. Full agent preflight
remains explicit.

Options:
  --force  Overwrite existing hooks (a backup is still created if possible).
  -h, --help
           Show this help text.
USAGE
}

while (( $# > 0 )); do
  case "$1" in
    --force)
      FORCE=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "[agent_hook_install] Unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

ensure_hook_dir() {
  local hook_dir="$1"
  if [[ ! -d "$hook_dir" ]]; then
    echo "[agent_hook_install] Missing hooks directory: $hook_dir" >&2
    exit 1
  fi
}

write_hook() {
  local hook_dir="$1"
  local hook_name="$2"
  local sentinel="$3"
  local target="$hook_dir/$hook_name"

  if [[ -f "$target" && ! -x "$target" ]]; then
    echo "[agent_hook_install] Existing non-executable hook found: $target" >&2
    exit 1
  fi

  if (( FORCE == 0 )) && [[ -f "$target" ]] && ! grep -q "$sentinel" "$target" 2>/dev/null; then
    echo "[agent_hook_install] Refusing to overwrite existing hook: $target" >&2
    echo "[agent_hook_install] Use --force to replace it." >&2
    exit 1
  fi

  if [[ -f "$target" && (( FORCE == 1 )) ]]; then
    cp "$target" "${target}.pre-agent-backup" 2>/dev/null || true
    echo "[agent_hook_install] Backed up existing hook to ${target}.pre-agent-backup"
  fi

  cat > "$target"
  chmod +x "$target"
}

remove_managed_hook() {
  local hook_dir="$1"
  local hook_name="$2"
  local sentinel="$3"
  local target="$hook_dir/$hook_name"

  if [[ -f "$target" ]] && grep -q "$sentinel" "$target" 2>/dev/null; then
    rm -f -- "$target"
    echo "[agent_hook_install] Removed deprecated managed hook: $target"
  fi
}

ROOT_HOOK_DIR="$(git rev-parse --git-common-dir)/hooks"
ensure_hook_dir "$ROOT_HOOK_DIR"
remove_managed_hook "$ROOT_HOOK_DIR" "post-merge" "run_agent_hook_checks.sh"

write_hook "$ROOT_HOOK_DIR" "post-checkout" "run_agent_hook_checks.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

if [[ "${WAVECRATE_SKIP_AGENT_PREFLIGHT_HOOK:-0}" == "1" ]]; then
  exit 0
fi

if [[ "${3:-0}" != "1" ]]; then
  exit 0
fi

repo_root="$(git rev-parse --show-toplevel 2>/dev/null || true)"
if [[ -z "$repo_root" ]]; then
  exit 0
fi

hook_checks="$repo_root/scripts/internal/agent/run_agent_hook_checks.sh"
if [[ -x "$hook_checks" ]]; then
  "$hook_checks" --event post-checkout
else
  echo "[agent_preflight_hook] ERROR: missing $hook_checks" >&2
  exit 1
fi
EOF

write_hook "$ROOT_HOOK_DIR" "pre-commit" "check_main_branch.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

if [[ "${WAVECRATE_SKIP_AGENT_PREFLIGHT_HOOK:-0}" == "1" ]]; then
  exit 0
fi

repo_root="$(git rev-parse --show-toplevel 2>/dev/null || true)"
if [[ -z "$repo_root" ]]; then
  exit 0
fi

branch_guard="$repo_root/scripts/internal/check/check_main_branch.sh"
if [[ -x "$branch_guard" ]]; then
  "$branch_guard"
else
  echo "[branch_guard] ERROR: missing $branch_guard" >&2
  exit 1
fi
EOF

write_hook "$ROOT_HOOK_DIR" "pre-push" "check_main_branch.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

if [[ "${WAVECRATE_SKIP_AGENT_PREFLIGHT_HOOK:-0}" == "1" ]]; then
  exit 0
fi

repo_root="$(git rev-parse --show-toplevel 2>/dev/null || true)"
if [[ -z "$repo_root" ]]; then
  exit 0
fi

branch_guard="$repo_root/scripts/internal/check/check_main_branch.sh"
if [[ -x "$branch_guard" ]]; then
  "$branch_guard"
else
  echo "[branch_guard] ERROR: missing $branch_guard" >&2
  exit 1
fi
EOF

echo "[agent_hook_install] Installed hooks:"
echo "[agent_hook_install]   - $ROOT_HOOK_DIR/post-checkout"
echo "[agent_hook_install]   - $ROOT_HOOK_DIR/pre-commit"
echo "[agent_hook_install]   - $ROOT_HOOK_DIR/pre-push"
echo "[agent_hook_install] Override with: export WAVECRATE_SKIP_AGENT_PREFLIGHT_HOOK=1"
