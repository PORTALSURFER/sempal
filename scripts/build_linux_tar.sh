#!/usr/bin/env bash
set -euo pipefail

APP_NAME="sempal"
REPO_ROOT="$(pwd)"
BUILD_CARGO_BIN="${SEMPAL_CARGO_BIN:-cargo}"
OUT_DIR="dist/release"
TARGET=""
ARCH=""
CHANNEL=""
VERSION=""

usage() {
  cat <<'EOF'
Usage: build_linux_tar.sh --target <triple> --arch <label> --channel <stable|nightly> [--version <x.y.z>] [--out-dir <path>]
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
    TAR_NAME="${APP_NAME}-v${VERSION}-linux-${ARCH}.tar.gz"
    ;;
  nightly)
    TAR_NAME="${APP_NAME}-nightly-linux-${ARCH}.tar.gz"
    ;;
  *)
    echo "Unknown channel: $CHANNEL" >&2
    exit 1
    ;;
esac

"$BUILD_CARGO_BIN" build --release --bin "$APP_NAME" --target "$TARGET"

WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

ROOT_DIR="${WORK_DIR}/${APP_NAME}"
mkdir -p "$ROOT_DIR"
cp "target/${TARGET}/release/${APP_NAME}" "${ROOT_DIR}/${APP_NAME}"

cat > "${ROOT_DIR}/install.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

PREFIX="${1:-$HOME/.local}"
BIN_DIR="${PREFIX}/bin"
mkdir -p "$BIN_DIR"
cp sempal "${BIN_DIR}/sempal"
chmod +x "${BIN_DIR}/sempal"

if [ -n "${SEMPAL_PANNS_ONNX_URL:-}" ]; then
  "${BIN_DIR}/sempal" --prepare-models
else
  echo "SEMPAL_PANNS_ONNX_URL not set; run 'SEMPAL_PANNS_ONNX_URL=... sempal --prepare-models' later."
fi

echo "Installed to ${BIN_DIR}/sempal"
EOF
chmod +x "${ROOT_DIR}/install.sh"

mkdir -p "$OUT_DIR"
(cd "$WORK_DIR" && tar -czf "${REPO_ROOT}/${OUT_DIR}/${TAR_NAME}" "$APP_NAME")
