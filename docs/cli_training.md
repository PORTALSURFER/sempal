# CLI Training Workflow

This page covers the developer CLI flow for exporting datasets, training models, and importing them into the app.

## Embedding pipeline (recommended)

1. Export embeddings + labels with stratified splits:

```bash
cargo run --bin sempal-embedding-export -- --out ./dataset
```

2. Train multinomial logistic regression with class balancing:

```bash
cargo run --bin sempal-train-logreg -- --dataset ./dataset --out ./model.json
```

3. Import the model into the library DB:

```bash
cargo run --bin sempal-model-import -- --model ./model.json --kind logreg
```

After importing, the app will enqueue inference on next startup or when you click “Re-run inference”.

Optional: Train an embedding MLP classifier:

```bash
cargo run --bin sempal-train-mlp -- --dataset ./dataset --out ./model.json
```

Then import with:

```bash
cargo run --bin sempal-model-import -- --model ./model.json --kind mlp
```

Notes:
- The embedding backend uses the CLAP audio ONNX model (`clap_audio.onnx`).
- Ensure the model is installed under the app models directory before exporting embeddings.

## Curated folder pipeline (app-style)

If you already have a curated dataset organized by class folders, you can train directly
without exporting JSONL datasets. The CLI tools accept either an export directory
(`manifest.json`) or a curated folder root:

```
./curated
  /kick
    kick_001.wav
  /snare
    snare_001.wav
```

Train LogReg on a curated folder:

```bash
cargo run --bin sempal-train-logreg -- --dataset ./curated --out ./model.json
```

Train MLP on a curated folder (optional hybrid features):

```bash
cargo run --bin sempal-train-mlp -- --dataset ./curated --hybrid --out ./model.json
```

Train baseline GBDT on a curated folder:

```bash
cargo run --bin sempal-train-baseline -- --dataset ./curated --out ./model.json
```

Notes:
- Use `--min-class-samples` to control class filtering (default: 30).
- Add `--augment` to enable default training-time augmentation.
- Curated embedding training still requires the CLAP model under the app models directory.

## Curated export pipeline (fast retrains)

If you want faster retrains from a curated folder, export an embedding dataset once:

```bash
cargo run --bin sempal-dataset-export-curated -- --dataset ./curated --out ./dataset
```

Then train from the exported dataset (no re-embedding step):

```bash
cargo run --bin sempal-train-logreg -- --dataset ./dataset --out ./model.json
```

Notes:
- The curated export uses stratified splits across classes.
- Add `--hybrid` to export embeddings plus light DSP features for MLP hybrid mode.
- Add `--augment` to bake augmentation copies into the export.

## Feature pipeline (legacy baseline)

1. Export features + labels:

```bash
cargo run --bin sempal-dataset-export -- --out ./dataset
```

2. Train the baseline GBDT stump model:

```bash
cargo run --bin sempal-train-baseline -- --dataset ./dataset --out ./model.json
```

3. Import the model:

```bash
cargo run --bin sempal-model-import -- --model ./model.json --kind gbdt
```

## Notes

- Use `--db <path-to-library.db>` with the export tools if your library is not in the default app data location.
- Adjust `--min-confidence` to include more weak labels (e.g. `0.70`) if export yields too few rows.
- For stratified splits, use `sempal-embedding-export` (the feature exporter keeps pack-level splits).

## What a good training set looks like

A strong dataset is balanced and diverse across categories and sources:

- Aim for ~300+ labeled samples per category (more is better).
- Avoid a single pack dominating a class; mix multiple packs/sources.
- Include variety: velocity layers, mic positions, and processing styles.
- Minimize label noise (fix obvious mislabels with user overrides before export).
- Keep a healthy test set (10–20%) so accuracy reflects real performance.
