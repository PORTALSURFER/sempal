#!/usr/bin/env bash

set -euo pipefail

repo_slug="${WAVECRATE_GITHUB_REPO:-PORTALSURFER/wavecrate}"

usage() {
  cat <<'EOF'
Usage:
  scripts/release.sh prepare --bump <major|minor|patch> [--source-ref <ref>] [--workflow-ref <branch-or-tag>] [--dry-run|--push] [--dispatch]
  scripts/release.sh rc --version <X.Y.Z> --rc-number <N> --branch <release/X.Y> [--release-notes <text>] [--dispatch]
  scripts/release.sh stable [options]  # temporarily disabled

Prepare derives the target version from Cargo.toml at the resolved source ref.
Without --dispatch, prepare runs scripts/internal/release/prepare_release_train.py locally.
Without --dispatch, rc validates inputs and prints the exact gh workflow command.
Stable publication is temporarily disabled while release candidates continue.
Public publishing workflow dispatch requires --dispatch.
EOF
}

die() {
  echo "release: $*" >&2
  exit 2
}

run() {
  printf '+'
  printf ' %q' "$@"
  printf '\n'
  "$@"
}

print_dry_gh_command() {
  printf 'Dry command:'
  printf ' %q' gh "$@"
  printf '\n'
}

repo_root() {
  git rev-parse --show-toplevel 2>/dev/null || die "not inside a git repository"
}

ensure_repo_root() {
  local root
  root="$(repo_root)"
  [[ "$PWD" == "$root" ]] || die "run from the Wavecrate repo root: $root"
  [[ -f Cargo.toml && -d scripts/internal/release && -d .github/workflows ]] \
    || die "current directory does not look like the Wavecrate repo root"
}

ensure_clean_worktree() {
  local status
  status="$(git status --short)"
  [[ -z "$status" ]] || die "release orchestration requires a clean working tree"
}

normalize_repo_slug() {
  local slug="$1"
  slug="${slug%/}"
  slug="${slug%.git}"
  [[ "$slug" == */* ]] || return 1
  printf '%s\n' "$slug" | tr '[:upper:]' '[:lower:]'
}

origin_github_repo_slug() {
  local url slug
  url="$(git remote get-url origin)"
  case "$url" in
    https://github.com/*) slug="${url#https://github.com/}" ;;
    http://github.com/*) slug="${url#http://github.com/}" ;;
    git@github.com:*) slug="${url#git@github.com:}" ;;
    ssh://git@github.com/*) slug="${url#ssh://git@github.com/}" ;;
    *) return 1 ;;
  esac
  normalize_repo_slug "$slug"
}

ensure_origin_matches_repo_slug() {
  local target_slug origin_slug
  target_slug="$(normalize_repo_slug "$repo_slug")" \
    || die "WAVECRATE_GITHUB_REPO must be an owner/repo slug"
  if origin_slug="$(origin_github_repo_slug)"; then
    [[ "$origin_slug" == "$target_slug" ]] \
      || die "origin remote resolves release refs from $origin_slug, but workflows target $target_slug; set WAVECRATE_GITHUB_REPO to $origin_slug or update origin before release orchestration"
  fi
}

fetch_release_refs() {
  git remote get-url origin >/dev/null 2>&1 || die "git remote 'origin' is required"
  ensure_origin_matches_repo_slug
  run git fetch --prune origin '+refs/heads/*:refs/remotes/origin/*'
  run git fetch --prune --prune-tags --force origin '+refs/heads/*:refs/remotes/origin/*' '+refs/tags/*:refs/tags/*'
}

stable_version_re='^[0-9]+\.[0-9]+\.[0-9]+$'
rc_number_re='^[1-9][0-9]*$'

validate_version() {
  local version="$1"
  [[ "$version" =~ $stable_version_re ]] || die "version must be MAJOR.MINOR.PATCH"
}

validate_bump() {
  local bump="$1"
  [[ "$bump" == "major" || "$bump" == "minor" || "$bump" == "patch" ]] \
    || die "bump must be major, minor, or patch"
}

package_version_from_stdin() {
  awk '
    /^\[package\][[:space:]]*$/ { in_package = 1; next }
    in_package && /^\[/ { exit }
    in_package && /^[[:space:]]*version[[:space:]]*=/ {
      gsub(/"/, "", $3)
      print $3
      exit
    }
  '
}

current_package_version() {
  package_version_from_stdin < Cargo.toml
}

package_version_at_ref() {
  local sha="$1"
  git show "${sha}:Cargo.toml" 2>/dev/null | package_version_from_stdin
}

bump_version() {
  local version="$1"
  local bump="$2"
  validate_version "$version"
  validate_bump "$bump"
  IFS=. read -r major minor patch <<<"$version"
  case "$bump" in
    major) printf '%s.0.0\n' "$((major + 1))" ;;
    minor) printf '%s.%s.0\n' "$major" "$((minor + 1))" ;;
    patch) printf '%s.%s.%s\n' "$major" "$minor" "$((patch + 1))" ;;
  esac
}

release_branch_for_version() {
  local version="$1"
  validate_version "$version"
  IFS=. read -r major minor _patch <<<"$version"
  printf 'release/%s.%s\n' "$major" "$minor"
}

validate_branch_for_version() {
  local branch="$1"
  local version="$2"
  local expected
  expected="$(release_branch_for_version "$version")"
  [[ "$branch" == "$expected" ]] || die "branch must be $expected for version $version"
}

validate_manifest_version_at_ref() {
  local sha="$1"
  local version="$2"
  local actual
  actual="$(package_version_at_ref "$sha")"
  [[ "$actual" == "$version" ]] || die "Cargo.toml version $actual at $sha does not match $version"
}

validate_rc_tag_available() {
  local tag="$1"
  local target_sha="$2"
  local existing
  existing="$(git rev-parse -q --verify "refs/tags/${tag}^{commit}" 2>/dev/null || true)"
  [[ -z "$existing" || "$existing" == "$target_sha" ]] \
    || die "RC tag $tag already points at $existing, not $target_sha"
}

validate_stable_tag_available() {
  local tag="$1"
  local target_sha="$2"
  local existing
  existing="$(git rev-parse -q --verify "refs/tags/${tag}^{commit}" 2>/dev/null || true)"
  [[ -z "$existing" || "$existing" == "$target_sha" ]] \
    || die "stable tag $tag already points at $existing, not $target_sha"
}

resolve_ref_sha() {
  local ref="$1"
  local candidate="$ref"
  if git rev-parse --verify --quiet "origin/${ref}^{commit}" >/dev/null; then
    candidate="origin/${ref}"
  fi
  git rev-parse --verify "${candidate}^{commit}" 2>/dev/null \
    || die "could not resolve source ref '$ref' to a commit"
}

commit_reachable_from_origin() {
  local sha="$1"
  git branch -r --contains "$sha" --format='%(refname:short)' | grep -q '^origin/' \
    || git tag --contains "$sha" | grep -q .
}

ensure_ref_dispatchable_from_origin() {
  local ref="$1"
  local sha="$2"
  commit_reachable_from_origin "$sha" \
    || die "source ref '$ref' resolves to local-only commit $sha; push it to origin or use a ref from $repo_slug before workflow dispatch"
}

ensure_origin_branch_for_workflow() {
  local branch="$1"
  git rev-parse --verify --quiet "origin/${branch}^{commit}" >/dev/null \
    || die "release branch '$branch' is not present on origin; push it before dispatching or copying workflow commands"
}

workflow_url() {
  local workflow="$1"
  printf 'https://github.com/%s/actions/workflows/%s\n' "$repo_slug" "$workflow"
}

require_gh() {
  local gh_bin="${WAVECRATE_RELEASE_GH_BIN:-gh}"
  if [[ "$gh_bin" == */* ]]; then
    [[ -x "$gh_bin" ]] || die "gh CLI not found or not executable: $gh_bin"
  else
    command -v "$gh_bin" >/dev/null 2>&1 || die "gh CLI not found; install gh or set WAVECRATE_RELEASE_GH_BIN"
  fi
  "$gh_bin" auth status >/dev/null 2>&1 || die "gh CLI is not authenticated; run gh auth login"
  printf '%s\n' "$gh_bin"
}

print_resolved() {
  local version="$1"
  local branch="$2"
  local sha="$3"
  echo "Resolved version: $version"
  echo "Release branch: $branch"
  echo "Target SHA: $sha"
}

print_followups() {
  local workflow="$1"
  local branch="$2"
  echo "Workflow: $(workflow_url "$workflow")"
  printf 'Run lookup:'
  printf ' %q' gh run list --repo "$repo_slug" --workflow "$workflow" --branch "$branch" --limit 5
  printf '\n'
}

run_prepare_train_dry_run_at_ref() {
  local target_sha="$1"
  shift
  local temp_root worktree status
  temp_root="$(mktemp -d "${TMPDIR:-/tmp}/wavecrate-release-dry-run.XXXXXX")" \
    || die "failed to create temporary dry-run worktree"
  worktree="$temp_root/source"

  set +e
  run git worktree add --detach "$worktree" "$target_sha"
  status=$?
  set -e

  if (( status == 0 )); then
    set +e
    run bash "$worktree/scripts/radiant.sh" provision --clean --path "$temp_root/radiant"
    status=$?
    set -e
  fi

  if (( status == 0 )); then
    set +e
    (cd "$worktree" && run scripts/internal/release/prepare_release_train.py "$@")
    status=$?
    set -e
  fi

  git worktree remove --force "$worktree" >/dev/null 2>&1 || true
  rm -rf "$temp_root"
  return "$status"
}

prepare() {
  local bump=""
  local source_ref="main"
  local workflow_ref="${WAVECRATE_RELEASE_WORKFLOW_REF:-main}"
  local dry_run=1
  local explicit_dry_run=0
  local push=0
  local dispatch=0
  while (( $# > 0 )); do
    case "$1" in
      --bump) bump="${2:-}"; shift 2 ;;
      --source-ref) source_ref="${2:-}"; shift 2 ;;
      --workflow-ref) workflow_ref="${2:-}"; shift 2 ;;
      --dry-run) explicit_dry_run=1; dry_run=1; shift ;;
      --push) push=1; dry_run=0; shift ;;
      --dispatch) dispatch=1; shift ;;
      -h|--help) usage; return 0 ;;
      *) die "unknown prepare argument: $1" ;;
    esac
  done
  [[ -n "$bump" ]] || die "prepare requires --bump <major|minor|patch>"
  validate_bump "$bump"
  [[ -n "$workflow_ref" ]] || die "--workflow-ref must not be empty"
  (( push == 0 || explicit_dry_run == 0 )) || die "--dry-run and --push cannot be combined"

  ensure_repo_root
  ensure_clean_worktree
  fetch_release_refs

  local source_version target_version branch target_sha
  target_sha="$(resolve_ref_sha "$source_ref")"
  source_version="$(package_version_at_ref "$target_sha")"
  validate_version "$source_version"
  target_version="$(bump_version "$source_version" "$bump")"
  branch="$(release_branch_for_version "$target_version")"
  print_resolved "$target_version" "$branch" "$target_sha"

  if (( dispatch )); then
    local gh_bin push_branch
    ensure_ref_dispatchable_from_origin "$source_ref" "$target_sha"
    gh_bin="$(require_gh)"
    push_branch=false
    (( push )) && push_branch=true
    run "$gh_bin" workflow run release-train-prepare.yml \
      --repo "$repo_slug" \
      --ref "$workflow_ref" \
      -f "version=$target_version" \
      -f "source_ref=$target_sha" \
      -f "push_branch=$push_branch"
    print_followups "release-train-prepare.yml" "$branch"
    return 0
  fi

  local args=(--version "$target_version" --source-ref "$target_sha")
  if (( push )); then
    args+=(--push)
  else
    args+=(--dry-run)
  fi
  if (( dry_run )); then
    run_prepare_train_dry_run_at_ref "$target_sha" "${args[@]}"
  else
    run scripts/internal/release/prepare_release_train.py "${args[@]}"
  fi
  print_followups "release-train-prepare.yml" "$branch"
}

rc() {
  local version=""
  local rc_number=""
  local branch=""
  local release_notes=""
  local dispatch=0
  while (( $# > 0 )); do
    case "$1" in
      --version) version="${2:-}"; shift 2 ;;
      --rc-number) rc_number="${2:-}"; shift 2 ;;
      --branch) branch="${2:-}"; shift 2 ;;
      --release-notes) release_notes="${2:-}"; shift 2 ;;
      --dispatch) dispatch=1; shift ;;
      -h|--help) usage; return 0 ;;
      *) die "unknown rc argument: $1" ;;
    esac
  done
  [[ -n "$version" && -n "$rc_number" && -n "$branch" ]] \
    || die "rc requires --version, --rc-number, and --branch"
  validate_version "$version"
  [[ "$rc_number" =~ $rc_number_re ]] || die "rc_number must be a positive integer"
  validate_branch_for_version "$branch" "$version"

  ensure_repo_root
  ensure_clean_worktree
  fetch_release_refs

  local target_sha
  target_sha="$(resolve_ref_sha "$branch")"
  ensure_origin_branch_for_workflow "$branch"
  validate_manifest_version_at_ref "$target_sha" "$version"
  print_resolved "$version" "$branch" "$target_sha"
  local rc_tag="v${version}-rc.${rc_number}"
  validate_rc_tag_available "$rc_tag" "$target_sha"
  echo "RC tag: $rc_tag"

  local args
  args=(workflow run release-rc.yml --repo "$repo_slug" --ref "$branch" -f "version=$version" -f "rc_number=$rc_number" -f "branch=$branch")
  [[ -z "$release_notes" ]] || args+=(-f "release_notes=$release_notes")

  if (( dispatch )); then
    local gh_bin
    gh_bin="$(require_gh)"
    run "$gh_bin" "${args[@]}"
  else
    print_dry_gh_command "${args[@]}"
  fi
  print_followups "release-rc.yml" "$branch"
}

stable() {
  local version=""
  local branch=""
  local release_notes=""
  local dispatch=0
  while (( $# > 0 )); do
    case "$1" in
      --version) version="${2:-}"; shift 2 ;;
      --branch) branch="${2:-}"; shift 2 ;;
      --release-notes) release_notes="${2:-}"; shift 2 ;;
      --dispatch) dispatch=1; shift ;;
      -h|--help) usage; return 0 ;;
      *) die "unknown stable argument: $1" ;;
    esac
  done
  die "stable releases are temporarily disabled; continue publishing release candidates"

  # Preserve the promotion implementation below so it can be re-enabled once
  # Wavecrate is ready for stable publication.
  [[ -n "$version" && -n "$branch" ]] || die "stable requires --version and --branch"
  validate_version "$version"
  validate_branch_for_version "$branch" "$version"

  ensure_repo_root
  ensure_clean_worktree
  fetch_release_refs

  local target_sha latest_rc_tag rc_sha
  target_sha="$(resolve_ref_sha "$branch")"
  ensure_origin_branch_for_workflow "$branch"
  validate_manifest_version_at_ref "$target_sha" "$version"
  latest_rc_tag="$(git tag -l "v${version}-rc.*" --sort=-v:refname | head -n 1)"
  [[ -n "$latest_rc_tag" ]] || die "stable release $version requires at least one RC tag v${version}-rc.N"
  rc_sha="$(git rev-list -n 1 "$latest_rc_tag")"
  [[ "$rc_sha" == "$target_sha" ]] \
    || die "latest RC $latest_rc_tag points at $rc_sha; stable target is $target_sha"
  validate_stable_tag_available "v${version}" "$target_sha"

  print_resolved "$version" "$branch" "$target_sha"
  echo "Promoted RC tag: $latest_rc_tag"

  local args
  args=(workflow run release-stable.yml --repo "$repo_slug" --ref "$branch" -f "version=$version" -f "branch=$branch")
  [[ -z "$release_notes" ]] || args+=(-f "release_notes=$release_notes")

  if (( dispatch )); then
    local gh_bin
    gh_bin="$(require_gh)"
    run "$gh_bin" "${args[@]}"
  else
    print_dry_gh_command "${args[@]}"
  fi
  print_followups "release-stable.yml" "$branch"
}

main() {
  if (( $# == 0 )); then
    usage
    exit 0
  fi
  local command="$1"
  shift
  case "$command" in
    prepare) prepare "$@" ;;
    rc) rc "$@" ;;
    stable) stable "$@" ;;
    -h|--help) usage ;;
    *) die "unknown release command: $command" ;;
  esac
}

main "$@"
