//! Developer utility to train and export an MLP classifier on embeddings.

use std::io::Write;
use std::path::PathBuf;

use sempal::analysis::embedding::EMBEDDING_DIM;
use sempal::dataset::curated;
use sempal::dataset::loader::{LoadedDataset, load_dataset};
use sempal::ml::gbdt_stump::TrainDataset;
use sempal::ml::mlp::{MlpInputKind, TrainOptions, train_mlp};
use sempal::ml::metrics::{ConfusionMatrix, accuracy, precision_recall_by_class};
use sempal::sample_sources::config::TrainingAugmentation;

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let options = parse_args(std::env::args().skip(1).collect())?;
    if !options.dataset_dir.is_dir() {
        return Err(format!(
            "Dataset path is not a directory: {}",
            options.dataset_dir.display()
        ));
    }
    let manifest_path = options.dataset_dir.join("manifest.json");
    let (train, val, test, input_kind) = if manifest_path.is_file() {
        let loaded = load_dataset(&options.dataset_dir).map_err(|err| err.to_string())?;
        let (train, val, test, input_kind) = split_train_val_test(&loaded, options.use_hybrid)?;
        (train, val, test, input_kind)
    } else {
        println!("Scanning curated dataset...");
        let samples = curated::collect_training_samples(&options.dataset_dir)?;
        if samples.is_empty() {
            return Err("Training dataset folder is empty".to_string());
        }
        let samples = curated::filter_training_samples(samples, options.min_class_samples);
        if samples.is_empty() {
            return Err("Training dataset has no classes after hygiene filter".to_string());
        }
        let class_count = samples
            .iter()
            .map(|sample| sample.class_id.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        println!("Found {} samples across {} classes", samples.len(), class_count);
        let split_map =
            curated::stratified_split_map(&samples, "sempal-training-dataset-v1", 0.1, 0.1)?;
        println!("Embedding samples...");
        let mut last_print = 0usize;
        let mut progress = |update: curated::TrainingProgress| {
            if update.processed == update.total || update.processed.saturating_sub(last_print) >= 25 {
                last_print = update.processed;
                print!(
                    "\rEmbedding {}/{} (skipped {})",
                    update.processed, update.total, update.skipped
                );
                let _ = std::io::stdout().flush();
                if update.processed == update.total {
                    println!();
                }
            }
        };
        let (train, val, test) = curated::build_mlp_dataset_from_samples_with_progress(
            &samples,
            &split_map,
            options.use_hybrid,
            options.min_class_samples,
            &options.augmentation,
            options.seed,
            Some(&mut progress),
        )?;
        let input_kind = if options.use_hybrid {
            MlpInputKind::HybridV1
        } else {
            MlpInputKind::EmbeddingV1
        };
        (train, val, test, input_kind)
    };

    let mut train_options = TrainOptions::default();
    train_options.hidden_size = options.hidden_size;
    train_options.epochs = options.epochs;
    train_options.batch_size = options.batch_size;
    train_options.learning_rate = options.learning_rate;
    train_options.l2_penalty = options.l2_penalty;
    train_options.seed = options.seed;
    train_options.input_kind = input_kind;
    let model = train_mlp(&train, &train_options, Some(&val))?;
    save_model(&options.model_out, &model)?;

    let (acc, cm, per_class) = evaluate(&model, &test);
    println!("test accuracy: {:.4}", acc);
    for (idx, stats) in per_class.iter().enumerate() {
        println!(
            "class {:>2} {:<16}  precision={:.3}  recall={:.3}  support={}",
            idx,
            model.classes[idx],
            stats.precision,
            stats.recall,
            stats.support
        );
    }
    println!("confusion matrix (rows=true, cols=pred):");
    for truth in 0..cm.n_classes {
        let mut row = String::new();
        for pred in 0..cm.n_classes {
            row.push_str(&format!("{:6}", cm.get(truth, pred)));
        }
        println!("{row}");
    }

    Ok(())
}

#[derive(Debug, Clone)]
struct CliOptions {
    dataset_dir: PathBuf,
    model_out: PathBuf,
    hidden_size: usize,
    epochs: usize,
    batch_size: usize,
    learning_rate: f32,
    l2_penalty: f32,
    seed: u64,
    min_class_samples: usize,
    use_hybrid: bool,
    augmentation: TrainingAugmentation,
}

fn parse_args(args: Vec<String>) -> Result<CliOptions, String> {
    let mut dataset_dir: Option<PathBuf> = None;
    let mut model_out = PathBuf::from("model.json");
    let mut hidden_size = 128usize;
    let mut epochs = 20usize;
    let mut batch_size = 128usize;
    let mut learning_rate = 0.01f32;
    let mut l2_penalty = 1e-4f32;
    let mut seed = 42u64;
    let mut min_class_samples = 30usize;
    let mut use_hybrid = false;
    let mut augmentation = TrainingAugmentation::default();

    let mut idx = 0usize;
    while idx < args.len() {
        match args[idx].as_str() {
            "-h" | "--help" => return Err(help_text()),
            "--dataset" => {
                idx += 1;
                let value = args.get(idx).ok_or_else(|| "--dataset requires a value".to_string())?;
                dataset_dir = Some(PathBuf::from(value));
            }
            "--out" => {
                idx += 1;
                let value = args.get(idx).ok_or_else(|| "--out requires a value".to_string())?;
                model_out = PathBuf::from(value);
            }
            "--hidden" => {
                idx += 1;
                let value = args.get(idx).ok_or_else(|| "--hidden requires a value".to_string())?;
                hidden_size = value
                    .parse::<usize>()
                    .map_err(|_| format!("Invalid --hidden value: {value}"))?;
            }
            "--epochs" => {
                idx += 1;
                let value = args.get(idx).ok_or_else(|| "--epochs requires a value".to_string())?;
                epochs = value
                    .parse::<usize>()
                    .map_err(|_| format!("Invalid --epochs value: {value}"))?;
            }
            "--batch" => {
                idx += 1;
                let value = args.get(idx).ok_or_else(|| "--batch requires a value".to_string())?;
                batch_size = value
                    .parse::<usize>()
                    .map_err(|_| format!("Invalid --batch value: {value}"))?;
            }
            "--learning-rate" => {
                idx += 1;
                let value = args
                    .get(idx)
                    .ok_or_else(|| "--learning-rate requires a value".to_string())?;
                learning_rate = value
                    .parse::<f32>()
                    .map_err(|_| format!("Invalid --learning-rate value: {value}"))?;
            }
            "--l2" => {
                idx += 1;
                let value = args.get(idx).ok_or_else(|| "--l2 requires a value".to_string())?;
                l2_penalty = value
                    .parse::<f32>()
                    .map_err(|_| format!("Invalid --l2 value: {value}"))?;
            }
            "--seed" => {
                idx += 1;
                let value = args.get(idx).ok_or_else(|| "--seed requires a value".to_string())?;
                seed = value
                    .parse::<u64>()
                    .map_err(|_| format!("Invalid --seed value: {value}"))?;
            }
            "--min-class-samples" => {
                idx += 1;
                let value = args
                    .get(idx)
                    .ok_or_else(|| "--min-class-samples requires a value".to_string())?;
                min_class_samples = value
                    .parse::<usize>()
                    .map_err(|_| format!("Invalid --min-class-samples value: {value}"))?;
            }
            "--hybrid" => {
                use_hybrid = true;
            }
            "--augment" => {
                augmentation.enabled = true;
            }
            unknown => return Err(format!("Unknown argument: {unknown}\n\n{}", help_text())),
        }
        idx += 1;
    }

    let dataset_dir = dataset_dir.ok_or_else(|| help_text())?;
    Ok(CliOptions {
        dataset_dir,
        model_out,
        hidden_size,
        epochs,
        batch_size,
        learning_rate,
        l2_penalty,
        seed,
        min_class_samples,
        use_hybrid,
        augmentation,
    })
}

fn help_text() -> String {
    [
        "sempal-train-mlp",
        "",
        "Trains an MLP classifier on an embedding dataset and writes model.json.",
        "",
        "Usage:",
        "  sempal-train-mlp --dataset <dataset_dir> [--out model.json]",
        "",
        "Options:",
        "  --dataset <dir>       Dataset export (manifest.json) or curated class-folder root (required).",
        "  --hidden <n>          Hidden layer size (default 128)",
        "  --epochs <n>          Training epochs (default 20)",
        "  --batch <n>           Batch size (default 128)",
        "  --learning-rate <f>   Learning rate (default 0.01)",
        "  --l2 <f>              L2 penalty (default 1e-4)",
        "  --seed <n>            RNG seed (default 42)",
        "  --min-class-samples <n> Minimum samples per class for curated folders (default: 30).",
        "  --hybrid             Use embeddings + light DSP features (requires hybrid export).",
        "  --augment            Enable default augmentation for curated folders.",
    ]
    .join("\n")
}

fn split_train_val_test(
    loaded: &LoadedDataset,
    use_hybrid: bool,
) -> Result<(TrainDataset, TrainDataset, TrainDataset, MlpInputKind), String> {
    let manifest_len = loaded.manifest.feature_len_f32;
    let hybrid_len = EMBEDDING_DIM + sempal::analysis::LIGHT_DSP_VECTOR_LEN;
    if manifest_len != EMBEDDING_DIM && manifest_len != hybrid_len {
        return Err(format!(
            "Unsupported embedding_len {} (expected {} or {})",
            manifest_len, EMBEDDING_DIM, hybrid_len
        ));
    }
    if use_hybrid && manifest_len != hybrid_len {
        return Err(format!(
            "Dataset feature_len {} does not match hybrid length {}",
            manifest_len, hybrid_len
        ));
    }
    if !use_hybrid && manifest_len != EMBEDDING_DIM {
        return Err(format!(
            "Dataset feature_len {} requires --hybrid",
            manifest_len
        ));
    }
    let expected_len = manifest_len;
    let input_kind = if use_hybrid {
        MlpInputKind::HybridV1
    } else {
        MlpInputKind::EmbeddingV1
    };

    let class_map = loaded.class_index_map();
    let classes: Vec<String> = class_map.iter().map(|(name, _)| name.clone()).collect();

    let mut train_rows = Vec::new();
    let mut val_rows = Vec::new();
    let mut test_rows = Vec::new();

    for sample in &loaded.samples {
        let Some(row) = loaded.feature_row(sample) else {
            continue;
        };
        let Some(&class_idx) = class_map.get(&sample.label.class_id) else {
            continue;
        };
        let labeled = LabeledRow {
            sample_id: sample.sample_id.clone(),
            class_idx,
            split: sample.split.clone(),
            row: row.to_vec(),
        };
        match labeled.split.as_str() {
            "train" => train_rows.push(labeled),
            "val" => val_rows.push(labeled),
            "test" => test_rows.push(labeled),
            _ => {}
        }
    }
    if val_rows.is_empty() {
        let mut keep_train = Vec::new();
        for row in train_rows {
            if split_u01(&row.sample_id) < 0.1 {
                val_rows.push(row);
            } else {
                keep_train.push(row);
            }
        }
        train_rows = keep_train;
        if val_rows.is_empty() {
            if let Some(row) = train_rows.pop() {
                val_rows.push(row);
            }
        }
    }
    if train_rows.is_empty() || test_rows.is_empty() || val_rows.is_empty() {
        return Err("Dataset needs train/val/test samples".to_string());
    }

    let (train_x, train_y) = unzip_rows(train_rows);
    let (val_x, val_y) = unzip_rows(val_rows);
    let (test_x, test_y) = unzip_rows(test_rows);

    Ok((
        TrainDataset {
            feature_len_f32: expected_len,
            feat_version: 0,
            classes: classes.clone(),
            x: train_x,
            y: train_y,
        },
        TrainDataset {
            feature_len_f32: expected_len,
            feat_version: 0,
            classes: classes.clone(),
            x: val_x,
            y: val_y,
        },
        TrainDataset {
            feature_len_f32: expected_len,
            feat_version: 0,
            classes,
            x: test_x,
            y: test_y,
        },
        input_kind,
    ))
}

#[derive(Clone)]
struct LabeledRow {
    sample_id: String,
    class_idx: usize,
    split: String,
    row: Vec<f32>,
}

fn split_u01(value: &str) -> f32 {
    let hash = blake3::hash(value.as_bytes());
    let bytes = hash.as_bytes();
    let raw = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    raw as f32 / u32::MAX as f32
}

fn unzip_rows(rows: Vec<LabeledRow>) -> (Vec<Vec<f32>>, Vec<usize>) {
    let mut x = Vec::with_capacity(rows.len());
    let mut y = Vec::with_capacity(rows.len());
    for row in rows {
        x.push(row.row);
        y.push(row.class_idx);
    }
    (x, y)
}

fn evaluate(
    model: &sempal::ml::mlp::MlpModel,
    dataset: &TrainDataset,
) -> (f32, ConfusionMatrix, Vec<sempal::ml::metrics::PerClassStats>) {
    let mut cm = ConfusionMatrix::new(model.classes.len());
    for (row, &truth) in dataset.x.iter().zip(dataset.y.iter()) {
        let predicted = model.predict_class_index(row);
        cm.add(truth, predicted);
    }
    let acc = accuracy(&cm);
    let per_class = precision_recall_by_class(&cm);
    (acc, cm, per_class)
}

fn save_model(path: &PathBuf, model: &sempal::ml::mlp::MlpModel) -> Result<(), String> {
    let json = serde_json::to_string_pretty(model).map_err(|err| err.to_string())?;
    std::fs::write(path, json).map_err(|err| err.to_string())
}
