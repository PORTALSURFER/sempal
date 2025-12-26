#!/usr/bin/env bash
set -euo pipefail

APP_NAME="sempal"
DISPLAY_NAME="SemPal"
REPO_ROOT="$(pwd)"
BUILD_CARGO_BIN="${SEMPAL_CARGO_BIN:-cargo}"
OUT_DIR="dist/release"
TARGET=""
ARCH=""
CHANNEL=""
VERSION=""
ONNX_URL=""

usage() {
  cat <<'EOF'
Usage: build_macos_pkg.sh --target <triple> --arch <label> --channel <stable|nightly> [--version <x.y.z>] [--onnx-url <url>] [--out-dir <path>]
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --target)
      TARGET="$2"
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
    --onnx-url)
      ONNX_URL="$2"
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

if [[ -z "$TARGET" || -z "$ARCH" || -z "$CHANNEL" ]]; then
  usage >&2
  exit 1
fi

case "$CHANNEL" in
  stable)
    if [[ -z "$VERSION" ]]; then
      echo "Stable releases require --version." >&2
      exit 1
    fi
    PKG_NAME="${APP_NAME}-v${VERSION}-macos-${ARCH}.pkg"
    ;;
  nightly)
    PKG_NAME="${APP_NAME}-nightly-macos-${ARCH}.pkg"
    ;;
  *)
    echo "Unknown channel: $CHANNEL" >&2
    exit 1
    ;;
esac

if [[ -z "$ONNX_URL" ]]; then
  echo "--onnx-url is required for the macOS installer." >&2
  exit 1
fi

SEMPAL_PANNS_ONNX_URL="$ONNX_URL" "$BUILD_CARGO_BIN" build --release --bin "$APP_NAME" --target "$TARGET"

WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

ROOT_DIR="${WORK_DIR}/root/Applications/${DISPLAY_NAME}"
mkdir -p "$ROOT_DIR"
cp "target/${TARGET}/release/${APP_NAME}" "${ROOT_DIR}/${APP_NAME}"

SCRIPTS_DIR="${WORK_DIR}/scripts"
mkdir -p "$SCRIPTS_DIR"
POSTINSTALL="${SCRIPTS_DIR}/postinstall"

cat > "$POSTINSTALL" <<EOF
#!/bin/sh
set -e
APP_PATH="/Applications/${DISPLAY_NAME}/${APP_NAME}"
ONNX_URL="${ONNX_URL}"
if [ ! -x "\$APP_PATH" ]; then
  exit 0
fi
CONSOLE_USER=\$(stat -f%Su /dev/console 2>/dev/null || echo "")
if [ -n "\$CONSOLE_USER" ] && [ "\$CONSOLE_USER" != "root" ]; then
  su -l "\$CONSOLE_USER" -c "SEMPAL_PANNS_ONNX_URL=\$ONNX_URL \"\$APP_PATH\" --prepare-models"
else
  SEMPAL_PANNS_ONNX_URL="\$ONNX_URL" "\$APP_PATH" --prepare-models
fi
exit 0
EOF
chmod +x "$POSTINSTALL"

mkdir -p "$OUT_DIR"
pkgbuild \
  --root "$WORK_DIR/root" \
  --identifier "com.sempal.app" \
  --version "${VERSION:-0.0.0}" \
  --install-location "/" \
  --scripts "$SCRIPTS_DIR" \
  "${REPO_ROOT}/${OUT_DIR}/${PKG_NAME}"
