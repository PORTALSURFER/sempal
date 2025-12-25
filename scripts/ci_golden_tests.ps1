Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if (Test-Path "tools\\generate_panns_golden_mel.py") {
    python tools\generate_panns_golden_mel.py --out assets\ml\panns_cnn14_16k\golden_mel.json
}
if (Test-Path "tools\\generate_panns_golden_embedding.py") {
    python tools\generate_panns_golden_embedding.py --out assets\ml\panns_cnn14_16k\golden_embedding.json
}

if (Test-Path "assets\\ml\\panns_cnn14_16k\\golden_mel.json") {
    $env:SEMPAL_PANNS_GOLDEN_PATH = "assets\\ml\\panns_cnn14_16k\\golden_mel.json"
}
if (Test-Path "assets\\ml\\panns_cnn14_16k\\golden_embedding.json") {
    $env:SEMPAL_PANNS_EMBED_GOLDEN_PATH = "assets\\ml\\panns_cnn14_16k\\golden_embedding.json"
}

cargo test golden_log_mel_matches_python
cargo test golden_embedding_matches_python
