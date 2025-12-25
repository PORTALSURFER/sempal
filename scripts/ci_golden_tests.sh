#!/usr/bin/env bash
set -euo pipefail

if [ -f tools/generate_panns_golden_mel.py ]; then
  python3 tools/generate_panns_golden_mel.py --out assets/ml/panns_cnn14_16k/golden_mel.json
fi
if [ -f tools/generate_panns_golden_embedding.py ]; then
  python3 tools/generate_panns_golden_embedding.py --out assets/ml/panns_cnn14_16k/golden_embedding.json
fi

if [ -f assets/ml/panns_cnn14_16k/golden_mel.json ]; then
  export SEMPAL_PANNS_GOLDEN_PATH="assets/ml/panns_cnn14_16k/golden_mel.json"
fi
if [ -f assets/ml/panns_cnn14_16k/golden_embedding.json ]; then
  export SEMPAL_PANNS_EMBED_GOLDEN_PATH="assets/ml/panns_cnn14_16k/golden_embedding.json"
fi

cargo test golden_log_mel_matches_python
cargo test golden_embedding_matches_python
