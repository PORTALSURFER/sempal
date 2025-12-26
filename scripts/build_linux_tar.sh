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
MODEL_DIR="${ROOT_DIR}/models"
mkdir -p "$MODEL_DIR"
BURNPACK_PATH="${REPO_ROOT}/assets/ml/panns_cnn14_16k/panns_cnn14_16k.bpk"
if [[ ! -f "$BURNPACK_PATH" ]]; then
  echo "Burnpack not found at ${BURNPACK_PATH}. Add the bundled model to assets/ml/panns_cnn14_16k." >&2
  exit 1
fi
cp "$BURNPACK_PATH" "${MODEL_DIR}/panns_cnn14_16k.bpk"

cat > "${ROOT_DIR}/install.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

PREFIX="${1:-$HOME/.local}"
BIN_DIR="${PREFIX}/bin"
mkdir -p "$BIN_DIR"
cp sempal "${BIN_DIR}/sempal"
chmod +x "${BIN_DIR}/sempal"

if [ -n "${SEMPAL_CONFIG_HOME:-}" ]; then
  CONFIG_ROOT="${SEMPAL_CONFIG_HOME}"
elif [ -n "${XDG_CONFIG_HOME:-}" ]; then
  CONFIG_ROOT="${XDG_CONFIG_HOME}"
else
  CONFIG_ROOT="${HOME}/.config"
fi

MODELS_DIR="${CONFIG_ROOT}/.sempal/models"
mkdir -p "${MODELS_DIR}"
cp "models/panns_cnn14_16k.bpk" "${MODELS_DIR}/panns_cnn14_16k.bpk"

echo "Installed to ${BIN_DIR}/sempal"
EOF
chmod +x "${ROOT_DIR}/install.sh"

mkdir -p "$OUT_DIR"
(cd "$WORK_DIR" && tar -czf "${REPO_ROOT}/${OUT_DIR}/${TAR_NAME}" "$APP_NAME")
