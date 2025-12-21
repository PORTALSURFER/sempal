$ErrorActionPreference = "Stop"

$dataset = "D:/music-production/samples/trainingset"
$outDir = "./dataset"

cargo run --bin sempal-dataset-export-curated -- --dataset $dataset --out $outDir --hybrid
