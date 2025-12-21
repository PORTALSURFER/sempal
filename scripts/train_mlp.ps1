$ErrorActionPreference = "Stop"

$dataset = "./dataset"
$out = "./model.json"

cargo run --bin sempal-train-mlp -- --hybrid --dataset $dataset --out $out
