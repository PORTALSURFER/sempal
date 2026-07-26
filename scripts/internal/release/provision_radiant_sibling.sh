#!/usr/bin/env bash
set -euo pipefail

# CI/release entrypoint. The helper provisions a clean sibling at the
# repository's committed Radiant revision and verifies its remote, manifest,
# cleanliness, and HEAD before Cargo runs.
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
export RADIANT_REPOSITORY_DEPLOY_KEY="${RADIANT_REPOSITORY_DEPLOY_KEY:-${RADIANT_SUBMODULE_DEPLOY_KEY:-}}"
if [[ -z "$RADIANT_REPOSITORY_DEPLOY_KEY" ]]; then
  echo "Missing RADIANT_REPOSITORY_DEPLOY_KEY (mapped from the repository deploy-key secret)." >&2
  exit 1
fi
exec "$ROOT_DIR/scripts/radiant.sh" provision --clean
