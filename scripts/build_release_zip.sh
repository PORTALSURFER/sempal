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

"$BUILD_CARGO_BIN" build --release --bin "$APP_NAME" --bin "${APP_NAME}-model-setup" --target "$TARGET"

BIN_NAME="$APP_NAME"
if [[ "$TARGET" == *windows* ]]; then
  BIN_NAME="${APP_NAME}.exe"
fi
SETUP_BIN_NAME="${APP_NAME}-model-setup"
if [[ "$TARGET" == *windows* ]]; then
  SETUP_BIN_NAME="${APP_NAME}-model-setup.exe"
fi

WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

ROOT_DIR="${WORK_DIR}/${APP_NAME}"
mkdir -p "$ROOT_DIR"
cp "target/${TARGET}/release/${BIN_NAME}" "${ROOT_DIR}/${BIN_NAME}"
cp "target/${TARGET}/release/${SETUP_BIN_NAME}" "${ROOT_DIR}/${SETUP_BIN_NAME}"

cat > "${ROOT_DIR}/update-manifest.json" <<EOF
{
  "app": "${APP_NAME}",
  "channel": "${CHANNEL}",
  "target": "${TARGET}",
  "platform": "${PLATFORM}",
  "arch": "${ARCH}",
  "files": ["${BIN_NAME}", "${SETUP_BIN_NAME}", "update-manifest.json"]
}
EOF

mkdir -p "$OUT_DIR"
(cd "$WORK_DIR" && zip -r "${REPO_ROOT}/${OUT_DIR}/${ZIP_NAME}" "$APP_NAME" >/dev/null)

if command -v sha256sum >/dev/null 2>&1; then
  SHA=$(sha256sum "${OUT_DIR}/${ZIP_NAME}" | awk '{print $1}')
else
  SHA=$(shasum -a 256 "${OUT_DIR}/${ZIP_NAME}" | awk '{print $1}')
fi
printf "%s  %s\n" "$SHA" "$ZIP_NAME" > "${OUT_DIR}/checksums-entry.txt"
