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

usage() {
  cat <<'EOF'
Usage: build_macos_pkg.sh --target <triple> --arch <label> --channel <stable|nightly> [--version <x.y.z>] [--out-dir <path>]
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

resolve_python() {
  if command -v python3 >/dev/null 2>&1; then
    echo "python3"
    return 0
  fi
  if command -v python >/dev/null 2>&1; then
    echo "python"
    return 0
  fi
  return 1
}

ensure_onnx() {
  if [[ -n "${SEMPAL_PANNS_ONNX_PATH:-}" ]]; then
    return 0
  fi
  local python_bin
  if ! python_bin="$(resolve_python)"; then
    echo "Python is required to export PANNs ONNX. Set SEMPAL_PANNS_ONNX_PATH instead." >&2
    exit 1
  fi
  local onnx_dir="${REPO_ROOT}/.tmp/panns_onnx"
  local onnx_path="${onnx_dir}/panns_cnn14_16k.onnx"
  if [[ ! -f "$onnx_path" ]]; then
    echo "Generating PANNs ONNX from checkpoint..."
    "$python_bin" "${REPO_ROOT}/tools/export_panns_onnx.py" --out-dir "$onnx_dir"
  fi
  export SEMPAL_PANNS_ONNX_PATH="$onnx_path"
}

ensure_onnx
"$BUILD_CARGO_BIN" build --release --bin "$APP_NAME" --target "$TARGET"

WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

ROOT_DIR="${WORK_DIR}/root/Applications/${DISPLAY_NAME}"
mkdir -p "$ROOT_DIR"
cp "target/${TARGET}/release/${APP_NAME}" "${ROOT_DIR}/${APP_NAME}"
MODEL_DIR="${ROOT_DIR}/models"
mkdir -p "$MODEL_DIR"
BURNPACK_PATH="$(find "target/${TARGET}/release/build" -name "panns_cnn14_16k.bpk" | head -n 1)"
if [[ -z "$BURNPACK_PATH" ]]; then
  echo "Burnpack not found in target/${TARGET}/release/build; ensure the ONNX model is available for the build." >&2
  exit 1
fi
cp "$BURNPACK_PATH" "${MODEL_DIR}/panns_cnn14_16k.bpk"

SCRIPTS_DIR="${WORK_DIR}/scripts"
mkdir -p "$SCRIPTS_DIR"
POSTINSTALL="${SCRIPTS_DIR}/postinstall"

cat > "$POSTINSTALL" <<EOF
#!/bin/sh
set -e
APP_PATH="/Applications/${DISPLAY_NAME}/${APP_NAME}"
MODEL_SOURCE="/Applications/${DISPLAY_NAME}/models/panns_cnn14_16k.bpk"
if [ ! -x "\$APP_PATH" ] || [ ! -f "\$MODEL_SOURCE" ]; then
  exit 0
fi
CONSOLE_USER=\$(stat -f%Su /dev/console 2>/dev/null || echo "")
copy_models() {
  local user="\$1"
  local home
  home=\$(dscl . -read "/Users/\$user" NFSHomeDirectory 2>/dev/null | awk '{print \$2}')
  if [ -z "\$home" ]; then
    home=\$(eval echo "~\$user")
  fi
  local models_dir="\${home}/Library/Application Support/.sempal/models"
  mkdir -p "\$models_dir"
  cp "\$MODEL_SOURCE" "\$models_dir/panns_cnn14_16k.bpk"
  chown "\$user" "\$models_dir" "\$models_dir/panns_cnn14_16k.bpk"
}
if [ -n "\$CONSOLE_USER" ] && [ "\$CONSOLE_USER" != "root" ]; then
  copy_models "\$CONSOLE_USER"
else
  copy_models "\$USER"
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
