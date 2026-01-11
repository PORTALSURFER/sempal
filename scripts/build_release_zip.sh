#!/usr/bin/env bash
set -euo pipefail

APP_NAME="sempal"
REPO_ROOT="$(pwd)"
BUILD_CARGO_BIN="${SEMPAL_CARGO_BIN:-cargo}"
OUT_DIR="dist/release"
TARGET=""
PLATFORM=""
ARCH=""
CHANNEL=""
VERSION=""

usage() {
  cat <<'EOF'
Usage: build_release_zip.sh --target <triple> --platform <label> --arch <label> --channel <stable|nightly> [--version <x.y.z>] [--out-dir <path>]
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --target)
      TARGET="$2"
      shift 2
      ;;
    --platform)
      PLATFORM="$2"
      shift 2
      ;;
    --arch)
      ARCH="$2"
      shift 2
      ;;
    --channel)
      CHANNEL="$2"
      shift 2
      ;;
    --version)
      VERSION="$2"
      shift 2
      ;;
    --out-dir)
      OUT_DIR="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

if [[ -z "$TARGET" || -z "$PLATFORM" || -z "$ARCH" || -z "$CHANNEL" ]]; then
  usage >&2
  exit 1
fi

case "$CHANNEL" in
  stable)
    if [[ -z "$VERSION" ]]; then
      echo "Stable releases require --version." >&2
      exit 1
    fi
    ZIP_NAME="${APP_NAME}-v${VERSION}-${PLATFORM}-${ARCH}.zip"
    ;;
  nightly)
    ZIP_NAME="${APP_NAME}-nightly-${PLATFORM}-${ARCH}.zip"
    ;;
  *)
    echo "Unknown channel: $CHANNEL" >&2
    exit 1
    ;;
esac

"$BUILD_CARGO_BIN" build --release --bin "$APP_NAME" --target "$TARGET"
if [[ "$TARGET" == *windows* ]]; then
  "$BUILD_CARGO_BIN" build --release --bin "${APP_NAME}-updater" --target "$TARGET"
fi

BIN_NAME="$APP_NAME"
UPDATER_NAME=""
if [[ "$TARGET" == *windows* ]]; then
  BIN_NAME="${APP_NAME}.exe"
  UPDATER_NAME="${APP_NAME}-updater.exe"
fi

WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

ROOT_DIR="${WORK_DIR}/${APP_NAME}"
mkdir -p "$ROOT_DIR"
cp "target/${TARGET}/release/${BIN_NAME}" "${ROOT_DIR}/${BIN_NAME}"
if [[ -n "$UPDATER_NAME" ]]; then
  cp "target/${TARGET}/release/${UPDATER_NAME}" "${ROOT_DIR}/${UPDATER_NAME}"
fi
MANIFEST_PATH="${ROOT_DIR}/update-manifest.json"
FILES=()
if [[ -n "$UPDATER_NAME" ]]; then
  FILES+=("$UPDATER_NAME")
fi
FILES+=("$BIN_NAME" "update-manifest.json")

PYTHON_BIN="python3"
if ! command -v "$PYTHON_BIN" >/dev/null 2>&1; then
  PYTHON_BIN="python"
fi
if ! command -v "$PYTHON_BIN" >/dev/null 2>&1; then
  echo "python is required to write update-manifest.json" >&2
  exit 1
fi

"$PYTHON_BIN" - "$MANIFEST_PATH" "$APP_NAME" "$CHANNEL" "$TARGET" "$PLATFORM" "$ARCH" "${FILES[@]}" <<'PY'
import json
import sys

manifest_path = sys.argv[1]
app, channel, target, platform, arch = sys.argv[2:7]
files = sys.argv[7:]

with open(manifest_path, "w", encoding="utf-8") as handle:
    json.dump(
        {
            "app": app,
            "channel": channel,
            "target": target,
            "platform": platform,
            "arch": arch,
            "files": files,
        },
        handle,
        indent=2,
        sort_keys=False,
    )
    handle.write("\n")
PY

mkdir -p "$OUT_DIR"
ZIP_PATH="${REPO_ROOT}/${OUT_DIR}/${ZIP_NAME}"
if command -v zip >/dev/null 2>&1; then
  (cd "$WORK_DIR" && zip -r "$ZIP_PATH" "$APP_NAME" >/dev/null)
elif command -v powershell.exe >/dev/null 2>&1; then
  mkdir -p "$OUT_DIR"
  if command -v cygpath >/dev/null 2>&1; then
    POWERSHELL_OUT_DIR=$(cygpath -w "$OUT_DIR")
    POWERSHELL_WORK_DIR=$(cygpath -w "$WORK_DIR")
  else
    POWERSHELL_OUT_DIR=$(powershell.exe -NoProfile -Command "[System.IO.Path]::GetFullPath('$OUT_DIR')")
    POWERSHELL_WORK_DIR=$(powershell.exe -NoProfile -Command "[System.IO.Path]::GetFullPath('$WORK_DIR')")
  fi
  POWERSHELL_ZIP_PATH="$POWERSHELL_OUT_DIR\\${ZIP_NAME}"
  powershell.exe -NoProfile -Command "Compress-Archive -Path \"$POWERSHELL_WORK_DIR\\$APP_NAME\\*\" -DestinationPath \"$POWERSHELL_ZIP_PATH\" -Force"
else
  echo "No zip tool found (zip or powershell Compress-Archive required)." >&2
  exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
  SHA=$(sha256sum "${OUT_DIR}/${ZIP_NAME}" | awk '{print $1}')
else
  SHA=$(shasum -a 256 "${OUT_DIR}/${ZIP_NAME}" | awk '{print $1}')
fi
printf "%s  %s\n" "$SHA" "$ZIP_NAME" > "${OUT_DIR}/checksums-entry.txt"
