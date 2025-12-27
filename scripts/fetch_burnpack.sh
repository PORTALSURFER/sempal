#!/usr/bin/env bash
set -euo pipefail

DEFAULT_URL="https://github.com/PORTALSURFER/sempal/releases/download/core-files/panns_cnn14_16k.bpk"
DEST="assets/ml/panns_cnn14_16k/panns_cnn14_16k.bpk"
URL="${SEMPAL_PANNS_BURNPACK_URL:-$DEFAULT_URL}"
FORCE=0

usage() {
  cat <<'EOF'
Usage: fetch_burnpack.sh [--dest <path>] [--url <url>] [--force]
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dest)
      DEST="$2"
      shift 2
      ;;
    --url)
      URL="$2"
      shift 2
      ;;
    --force)
      FORCE=1
      shift
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

if [[ -f "$DEST" && "$FORCE" -eq 0 ]]; then
  echo "Burnpack already present at $DEST"
  exit 0
fi

mkdir -p "$(dirname "$DEST")"

if command -v curl >/dev/null 2>&1; then
  curl -L --fail --output "$DEST" "$URL"
elif command -v wget >/dev/null 2>&1; then
  wget -O "$DEST" "$URL"
else
  echo "curl or wget is required to download the burnpack." >&2
  exit 1
fi

if [[ ! -f "$DEST" ]]; then
  echo "Burnpack download failed: $DEST not found." >&2
  exit 1
fi
