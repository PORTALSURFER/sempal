#!/usr/bin/env bash
set -euo pipefail

python3 tools/generate_clap_golden_mel.py --out assets/ml/clap_v1/golden_mel.json
python3 tools/generate_clap_golden_embedding.py --out assets/ml/clap_v1/golden_embedding.json
python3 tools/generate_logreg_golden_inference.py --out assets/ml/clap_v1/golden_infer.json

export SEMPAL_CLAP_GOLDEN_PATH="assets/ml/clap_v1/golden_mel.json"
export SEMPAL_CLAP_EMBED_GOLDEN_PATH="assets/ml/clap_v1/golden_embedding.json"
export SEMPAL_GOLDEN_INFER_PATH="assets/ml/clap_v1/golden_infer.json"

cargo test golden_log_mel_matches_python
cargo test golden_embedding_matches_python
cargo test golden_inference_matches_python
