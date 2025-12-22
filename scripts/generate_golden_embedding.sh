#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out_path="${root_dir}/tests/golden_embedding.json"

mkdir -p "$(dirname "$out_path")"
python3 "${root_dir}/tools/generate_clap_golden_embedding.py" --out "$out_path"
echo "Wrote ${out_path}"
