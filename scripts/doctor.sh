#!/usr/bin/env bash

# Environment sanity checks for local development and agent runs.
# Prints common pitfalls called out in README and developer diagnostic log paths.
# Linux/WSL paths are contributor/agent tooling hints, not shipped app support.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

failures=0
warnings=0

info() { printf "%s\n" "[doctor] $*"; }
warn() { printf "%s\n" "[doctor][warn] $*" >&2; warnings=$((warnings + 1)); }
err() { printf "%s\n" "[doctor][error] $*" >&2; failures=$((failures + 1)); }

bootstrap_hint() {
  warn "Run bootstrap to install pinned toolchain + tools:"
  warn "  - bash scripts/bootstrap.sh"
  warn "  - powershell -ExecutionPolicy Bypass -File scripts/bootstrap.ps1"
}

os_name="$(uname -s | tr '[:upper:]' '[:lower:]')"
is_wsl=0
if [[ -r /proc/version ]] && grep -qi "microsoft" /proc/version 2>/dev/null; then
  is_wsl=1
fi

info "Repo: $ROOT_DIR"
info "OS: ${os_name}${is_wsl:+ (wsl)}"

info "Checking locked Cargo dependency resolution..."
if cargo metadata --locked --format-version 1 >/dev/null 2>&1; then
  info "Cargo dependency resolution: available"
else
  err "Cargo dependency resolution failed; run cargo metadata --locked"
fi

info "Checking ripgrep (rg)..."
if command -v rg >/dev/null 2>&1; then
  info "rg: present"
else
  warn "rg: missing (recommended; used by several repo checks)"
  bootstrap_hint
fi

info "Checking Rust toolchain (pinned)..."
if [[ ! -f "$ROOT_DIR/rust-toolchain.toml" ]]; then
  warn "rust-toolchain.toml not found at repo root (can't verify toolchain pin)"
else
  if ! command -v rustup >/dev/null 2>&1; then
    err "rustup not found on PATH (can't verify/install pinned toolchain)"
    bootstrap_hint
  else
    channel="$(sed -n 's/^[[:space:]]*channel[[:space:]]*=[[:space:]]*\"\\([^\"]*\\)\".*/\\1/p' rust-toolchain.toml | head -n 1 | tr -d '[:space:]')"
    [[ -z "$channel" ]] && channel="stable"
    info "Pinned toolchain channel: $channel"

    # Verify toolchain exists by trying to run rustc under it.
    if rustup run "$channel" rustc -V >/dev/null 2>&1; then
      info "Pinned toolchain installed: yes"
    else
      err "Pinned toolchain is not installed: $channel"
      bootstrap_hint
    fi

    # Verify active toolchain matches pin (warn only; can be overridden locally).
    active="$(rustup show active-toolchain 2>/dev/null | awk '{print $1}' || true)"
    if [[ -n "$active" ]]; then
      if [[ "$active" == "$channel"* ]]; then
        info "Active toolchain: $active (matches pin)"
      else
        warn "Active toolchain: $active (does not match pin: $channel)"
        warn "Consider: rustup default $channel"
      fi
    else
      warn "Could not determine active toolchain via rustup"
    fi

    # Verify components exist for pinned toolchain.
    installed="$(rustup component list --toolchain "$channel" --installed 2>/dev/null || true)"
    if grep -q '^rustfmt' <<<"$installed"; then
      info "rustfmt: installed (toolchain $channel)"
    else
      err "rustfmt is not installed for toolchain $channel"
      bootstrap_hint
    fi
    if grep -q '^clippy' <<<"$installed"; then
      info "clippy: installed (toolchain $channel)"
    else
      err "clippy is not installed for toolchain $channel"
      bootstrap_hint
    fi
  fi
fi

info "Checking Git LFS..."
if command -v git >/dev/null 2>&1; then
  if git lfs version >/dev/null 2>&1; then
    info "Git LFS: present"
  else
    warn "Git LFS: missing (install git-lfs if you see checkout/build issues with large assets)"
  fi
else
  warn "git not found on PATH"
fi

info "Checking WAVECRATE_NATIVE_FONT_PATH..."
if [[ -n "${WAVECRATE_NATIVE_FONT_PATH:-}" ]]; then
  if [[ -f "$WAVECRATE_NATIVE_FONT_PATH" ]]; then
    info "WAVECRATE_NATIVE_FONT_PATH: OK ($WAVECRATE_NATIVE_FONT_PATH)"
  else
    err "WAVECRATE_NATIVE_FONT_PATH is set but not a file: $WAVECRATE_NATIVE_FONT_PATH"
  fi
else
  info "WAVECRATE_NATIVE_FONT_PATH: not set (OK)"
fi

info "Checking CPAL_ASIO_DIR (Windows ASIO builds)..."
if [[ -n "${CPAL_ASIO_DIR:-}" ]]; then
  if [[ -d "$CPAL_ASIO_DIR" ]]; then
    if [[ -d "$CPAL_ASIO_DIR/host" && -d "$CPAL_ASIO_DIR/common" ]]; then
      info "CPAL_ASIO_DIR: looks like an ASIO SDK root ($CPAL_ASIO_DIR)"
    else
      err "CPAL_ASIO_DIR exists but doesn't look like ASIO SDK root (expected host/ and common/): $CPAL_ASIO_DIR"
    fi
  else
    err "CPAL_ASIO_DIR is set but not a directory: $CPAL_ASIO_DIR"
  fi
else
  info "CPAL_ASIO_DIR: not set (OK unless building Windows ASIO support)"
fi

info "Expected developer diagnostic log locations:"
info "  Supported app installs:"
info "    macOS:   \$HOME/Library/Application Support/.wavecrate/logs"
info "    Windows: %APPDATA%\\.wavecrate\\logs"
info "  Developer/agent tooling only; Linux is not a shipped app install target:"
info "    Linux:   \$HOME/.config/.wavecrate/logs"
if (( is_wsl == 1 )); then
  info "    WSL hint: /mnt/c/Users/<you>/AppData/Roaming/.wavecrate/logs"
fi

if (( failures > 0 )); then
  info "Result: FAIL ($failures errors, $warnings warnings)"
  exit 1
fi

info "Result: OK ($warnings warnings)"
exit 0
