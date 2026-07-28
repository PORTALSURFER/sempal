#!/usr/bin/env bash

# Locate and provision the standalone Radiant checkout used by Wavecrate.
#
# `locate` is deliberately non-mutating and permits a dirty/feature-branch
# sibling for live development. `provision --clean` is for CI/release and only
# operates on a new or already-clean target; it never resets a dirty checkout.

set -euo pipefail

ROOT_DIR=""
ACTION="locate"
TARGET=""
CLEAN=0

usage() {
  cat <<'EOF'
Usage: scripts/radiant.sh [locate|provision] [options]

Options:
  --root <dir>       Wavecrate checkout (defaults to this repository)
  --path <dir>       paired sibling path (must match Cargo's ../radiant path)
  --clean            provision an exact, clean detached checkout (CI/release)
  --help             show this help

Environment:
  RADIANT_REPOSITORY_DEPLOY_KEY  SSH private key for private CI/release clones
  RADIANT_SUBMODULE_DEPLOY_KEY   legacy alias for the existing GitHub secret
EOF
}

die() { echo "[radiant] ERROR: $*" >&2; exit 1; }
info() { echo "[radiant] $*"; }

while (( $# > 0 )); do
  case "$1" in
    locate|provision) ACTION="$1"; shift ;;
    --root) ROOT_DIR="${2:-}"; shift 2 ;;
    --path) TARGET="${2:-}"; shift 2 ;;
    --clean) CLEAN=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown argument: $1" ;;
  esac
done

if [[ -z "$ROOT_DIR" ]]; then
  ROOT_DIR="$(git rev-parse --show-toplevel 2>/dev/null || true)"
fi
[[ -n "$ROOT_DIR" && -f "$ROOT_DIR/Cargo.toml" ]] || die "could not identify a Wavecrate checkout; pass --root"
ROOT_DIR="$(cd "$ROOT_DIR" && pwd -P)"

metadata_value() {
  local key="$1"
  awk -v key="$key" '
    $0 ~ "^[[:space:]]*" key "[[:space:]]*=" {
      sub(/^[^=]*=[[:space:]]*/, "", $0); gsub(/^[" ]+|[" ]+$/, "", $0); print; exit
    }
  ' "$ROOT_DIR/radiant-dependency.toml"
}

[[ -f "$ROOT_DIR/radiant-dependency.toml" ]] || die "missing radiant-dependency.toml"
REPOSITORY="$(metadata_value repository)"
REVISION="$(metadata_value revision)"
METADATA_PATH="$(metadata_value path)"
[[ "$REVISION" =~ ^[0-9a-f]{40}$ ]] || die "metadata revision is not a full SHA"
[[ -n "$REPOSITORY" && -n "$METADATA_PATH" ]] || die "metadata repository/path is incomplete"

[[ -z "${WAVECRATE_RADIANT_DIR:-}" ]] || die "WAVECRATE_RADIANT_DIR is unsupported because Cargo is pinned to the paired ../radiant sibling; unset it and use the paired path"

canonical_path() {
  local path="$1" parent name
  if [[ "$path" != /* ]]; then
    path="$ROOT_DIR/$path"
  fi
  parent="$(dirname "$path")"
  name="$(basename "$path")"
  parent="$(cd "$parent" 2>/dev/null && pwd -P)" \
    || die "cannot resolve sibling parent directory: $parent"
  printf '%s/%s' "$parent" "$name"
}

METADATA_TARGET="$(canonical_path "$METADATA_PATH")"
if [[ -z "$TARGET" ]]; then
  TARGET="$METADATA_TARGET"
else
  TARGET="$(canonical_path "$TARGET")"
  [[ "$TARGET" == "$METADATA_TARGET" ]] \
    || die "Radiant path '$TARGET' does not match Cargo's configured sibling '$METADATA_TARGET'; use the paired ../radiant path"
fi

expected_remote_matches() {
  local remote="$1" normalized
  normalized="${remote%.git}"
  case "$normalized" in
    "$REPOSITORY"|"${REPOSITORY#https://}"|"git@github.com:PORTALSURFER/radiant") return 0 ;;
    "https://github.com/PORTALSURFER/radiant"|"http://github.com/PORTALSURFER/radiant") return 0 ;;
    *) return 1 ;;
  esac
}

validate_checkout() {
  [[ -d "$TARGET" ]] || die "Radiant sibling is missing: $TARGET (run scripts/radiant.sh provision)"
  [[ -f "$TARGET/Cargo.toml" ]] || die "Radiant sibling has no Cargo.toml: $TARGET"
  grep -qE '^name[[:space:]]*=[[:space:]]*[\"'"'"']radiant[\"'"'"']' "$TARGET/Cargo.toml" \
    || die "Radiant sibling manifest is not package radiant: $TARGET/Cargo.toml"
  git -C "$TARGET" rev-parse --git-dir >/dev/null 2>&1 \
    || die "Radiant sibling is not a Git checkout: $TARGET"
  local remote
  remote="$(git -C "$TARGET" remote get-url origin 2>/dev/null || true)"
  [[ -n "$remote" ]] || die "Radiant sibling has no origin remote: $TARGET"
  expected_remote_matches "$remote" \
    || die "Radiant sibling origin '$remote' does not match $REPOSITORY"
}

print_state() {
  local head branch dirty match
  head="$(git -C "$TARGET" rev-parse HEAD)"
  branch="$(git -C "$TARGET" symbolic-ref --short -q HEAD || echo detached)"
  dirty="clean"
  [[ -z "$(git -C "$TARGET" status --porcelain --untracked-files=normal)" ]] || dirty="dirty"
  match="no"
  [[ "$head" == "$REVISION" ]] && match="yes"
  printf 'RADIANT_DIR=%s\nRADIANT_HEAD=%s\nRADIANT_BRANCH=%s\nRADIANT_STATE=%s\nRADIANT_REVISION_MATCH=%s\n' \
    "$TARGET" "$head" "$branch" "$dirty" "$match"
}

provision() {
  local parent key_file ssh_command clone_url cleanup_command
  if [[ -e "$TARGET" ]]; then
    validate_checkout
    if (( CLEAN == 0 )); then
      info "existing sibling preserved (no reset/clean/pull): $TARGET"
      print_state
      return 0
    fi
    [[ -z "$(git -C "$TARGET" status --porcelain --untracked-files=normal)" ]] \
      || die "refusing to mutate dirty Radiant sibling for clean provisioning: $TARGET"
    info "fetching exact Radiant revision into clean sibling"
    git -C "$TARGET" fetch --no-tags origin "$REVISION"
    git -C "$TARGET" checkout --detach "$REVISION"
    validate_checkout
    [[ "$(git -C "$TARGET" rev-parse HEAD)" == "$REVISION" ]] || die "Radiant HEAD mismatch after provisioning"
    print_state
    return 0
  fi

  parent="$(dirname "$TARGET")"
  mkdir -p "$parent"
  key_file=""
  ssh_command=""
  clone_url="$REPOSITORY"
  if [[ -n "${RADIANT_REPOSITORY_DEPLOY_KEY:-${RADIANT_SUBMODULE_DEPLOY_KEY:-}}" ]]; then
    key_file="$(mktemp "${TMPDIR:-/tmp}/radiant-key.XXXXXX")"
    printf '%s\n' "${RADIANT_REPOSITORY_DEPLOY_KEY:-${RADIANT_SUBMODULE_DEPLOY_KEY}}" > "$key_file"
    chmod 600 "$key_file"
    ssh_command="ssh -i $key_file -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new"
    clone_url="git@github.com:PORTALSURFER/radiant.git"
    printf -v cleanup_command 'rm -f -- %q' "$key_file"
    trap "$cleanup_command" EXIT
  fi
  info "cloning Radiant at $REVISION into $TARGET"
  if [[ -n "$ssh_command" ]]; then
    GIT_SSH_COMMAND="$ssh_command" git clone --no-checkout "$clone_url" "$TARGET"
    GIT_SSH_COMMAND="$ssh_command" git -C "$TARGET" fetch --no-tags origin "$REVISION"
    GIT_SSH_COMMAND="$ssh_command" git -C "$TARGET" checkout --detach "$REVISION"
  else
    git clone --no-checkout "$clone_url" "$TARGET"
    git -C "$TARGET" fetch --no-tags origin "$REVISION"
    git -C "$TARGET" checkout --detach "$REVISION"
  fi
  validate_checkout
  [[ "$(git -C "$TARGET" rev-parse HEAD)" == "$REVISION" ]] || die "Radiant HEAD mismatch after clone"
  print_state
}

if [[ "$ACTION" == provision ]]; then
  provision
else
  validate_checkout
  print_state
fi
