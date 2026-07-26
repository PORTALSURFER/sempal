#!/usr/bin/env bash

# Regression coverage for hook/full-preflight ownership and single-flight
# coordination. Uses a temporary Git repository and instrumented check commands
# so it never starts the real Cargo-backed validation lane.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
FIXTURE_DIR="$(mktemp -d)"
trap 'rm -rf "$FIXTURE_DIR"' EXIT

failures=0

fail() {
  echo "[agent_preflight_test] FAIL: $*" >&2
  failures=$((failures + 1))
}

expect_file_count() {
  local path="$1"
  local expected="$2"
  local actual=0
  if [[ -f "$path" ]]; then
    actual="$(wc -l < "$path" | tr -d ' ')"
  fi
  if [[ "$actual" != "$expected" ]]; then
    fail "expected $expected invocation(s) in $path, got $actual"
  fi
}

REPO_DIR="$FIXTURE_DIR/repo"
REMOTE_DIR="$FIXTURE_DIR/remote.git"
mkdir -p "$REPO_DIR/scripts/internal/agent" "$REPO_DIR/scripts/internal/check"
cp "$ROOT_DIR/scripts/internal/agent/install_agent_preflight_hooks.sh" "$REPO_DIR/scripts/internal/agent/"
cp "$ROOT_DIR/scripts/internal/agent/run_agent_preflight.sh" "$REPO_DIR/scripts/internal/agent/"
cp "$ROOT_DIR/scripts/internal/agent/run_agent_hook_checks.sh" "$REPO_DIR/scripts/internal/agent/"

cat > "$REPO_DIR/scripts/internal/check/check_main_branch.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
"${WAVECRATE_AGENT_HOOK_CHECK_COMMAND:?missing fake hook check}"
EOF
chmod +x "$REPO_DIR/scripts/internal/agent/"*.sh "$REPO_DIR/scripts/internal/check/check_main_branch.sh"

git init --initial-branch=main "$REPO_DIR" >/dev/null
git -C "$REPO_DIR" config user.email agent-test@example.invalid
git -C "$REPO_DIR" config user.name agent-test
touch "$REPO_DIR/README.md"
git -C "$REPO_DIR" add README.md
WAVECRATE_SKIP_AGENT_PREFLIGHT_HOOK=1 git -C "$REPO_DIR" commit -m fixture >/dev/null
git init --bare "$REMOTE_DIR" >/dev/null
git -C "$REPO_DIR" remote add origin "$REMOTE_DIR"
WAVECRATE_SKIP_AGENT_PREFLIGHT_HOOK=1 git -C "$REPO_DIR" push -u origin main >/dev/null

cat > "$REPO_DIR/.git/hooks/post-merge" <<'EOF'
#!/usr/bin/env bash
# run_agent_hook_checks.sh
EOF
chmod +x "$REPO_DIR/.git/hooks/post-merge"
HOOK_COUNT="$FIXTURE_DIR/hook-count"
FULL_COUNT="$FIXTURE_DIR/full-count"
FAKE_HOOK_CHECK="$FIXTURE_DIR/fake-hook-check"
FAKE_FULL_CHECK="$FIXTURE_DIR/fake-full-check"

cat > "$FAKE_HOOK_CHECK" <<EOF
#!/usr/bin/env bash
set -euo pipefail
echo hook >> "$HOOK_COUNT"
EOF
cat > "$FAKE_FULL_CHECK" <<EOF
#!/usr/bin/env bash
set -euo pipefail
echo full >> "$FULL_COUNT"
if [[ -n "\${WAVECRATE_AGENT_TEST_READY_FILE:-}" ]]; then
  : > "\$WAVECRATE_AGENT_TEST_READY_FILE"
fi
sleep "\${WAVECRATE_AGENT_TEST_SLEEP_SECONDS:-0}"
EOF
chmod +x "$FAKE_HOOK_CHECK" "$FAKE_FULL_CHECK"

(cd "$REPO_DIR" && ./scripts/internal/agent/install_agent_preflight_hooks.sh --force) >/dev/null

for hook in post-checkout pre-commit pre-push; do
  if [[ ! -x "$REPO_DIR/.git/hooks/$hook" ]]; then
    fail "installer did not create executable $hook hook"
  fi
done
if [[ -e "$REPO_DIR/.git/hooks/post-merge" ]]; then
  fail "installer did not remove the deprecated managed root post-merge hook"
fi
# Automatic and explicit checkouts each run only the cheap hook state check.
# Neither may launch the full preflight command.
(cd "$REPO_DIR" && WAVECRATE_AGENT_HOOK_CHECK_COMMAND="$FAKE_HOOK_CHECK" \
  WAVECRATE_AGENT_CI_CHECKS_COMMAND="$FAKE_FULL_CHECK" \
  .git/hooks/post-checkout before after 1)
(cd "$REPO_DIR" && WAVECRATE_AGENT_HOOK_CHECK_COMMAND="$FAKE_HOOK_CHECK" \
  WAVECRATE_AGENT_CI_CHECKS_COMMAND="$FAKE_FULL_CHECK" \
  .git/hooks/post-checkout after after 1)
expect_file_count "$HOOK_COUNT" 2
expect_file_count "$FULL_COUNT" 0

# Pre-commit and pre-push retain their branch-policy ownership.
(cd "$REPO_DIR" && WAVECRATE_AGENT_HOOK_CHECK_COMMAND="$FAKE_HOOK_CHECK" .git/hooks/pre-commit)
(cd "$REPO_DIR" && WAVECRATE_AGENT_HOOK_CHECK_COMMAND="$FAKE_HOOK_CHECK" .git/hooks/pre-push)
expect_file_count "$HOOK_COUNT" 4

# Two concurrent explicit full-preflight requests coalesce to one owner.
STATE_DIR="$FIXTURE_DIR/preflight-state"
WAVECRATE_AGENT_PREFLIGHT_STATE_DIR="$STATE_DIR" \
  WAVECRATE_AGENT_CI_CHECKS_COMMAND="$FAKE_FULL_CHECK" \
  WAVECRATE_AGENT_TEST_SLEEP_SECONDS=1 \
  WAVECRATE_AGENT_PREFLIGHT_TEST_PAUSE_AFTER_LOCK_ACQUIRE_SECONDS=1 \
  "$REPO_DIR/scripts/internal/agent/run_agent_preflight.sh" >"$FIXTURE_DIR/owner.log" 2>&1 &
owner_pid=$!
for (( attempt = 0; attempt < 50; attempt++ )); do
  if [[ -f "$STATE_DIR/run.lock" ]]; then
    break
  fi
  sleep 0.1
done
if [[ ! -f "$STATE_DIR/run.lock" ]]; then
  fail "single-flight owner did not publish its lock"
fi
WAVECRATE_AGENT_PREFLIGHT_STATE_DIR="$STATE_DIR" \
  WAVECRATE_AGENT_CI_CHECKS_COMMAND="$FAKE_FULL_CHECK" \
  "$REPO_DIR/scripts/internal/agent/run_agent_preflight.sh" >"$FIXTURE_DIR/joiner.log" 2>&1 &
joiner_pid=$!
wait "$owner_pid" || fail "single-flight owner failed"
wait "$joiner_pid" || fail "single-flight joiner failed"
expect_file_count "$FULL_COUNT" 1
if ! rg -q 'coalesced with active full preflight' "$FIXTURE_DIR/joiner.log"; then
  fail "joiner did not report single-flight coalescing"
fi

# A dead owner lock cannot suppress the next explicit full preflight.
printf '999999\t%s\n' "$STATE_DIR/stale-result" > "$STATE_DIR/run.lock"
WAVECRATE_AGENT_PREFLIGHT_STATE_DIR="$STATE_DIR" \
  WAVECRATE_AGENT_CI_CHECKS_COMMAND="$FAKE_FULL_CHECK" \
  "$REPO_DIR/scripts/internal/agent/run_agent_preflight.sh" >/dev/null
expect_file_count "$FULL_COUNT" 2
if [[ -e "$STATE_DIR/run.lock" ]]; then
  fail "stale lock was not cleared"
fi

if (( failures > 0 )); then
  echo "[agent_preflight_test] FAILED: $failures assertion(s) failed." >&2
  exit 1
fi

echo "[agent_preflight_test] OK"
