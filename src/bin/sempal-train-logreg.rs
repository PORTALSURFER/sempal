//! Developer utility to train and export a logistic regression embedding classifier.

use std::path::PathBuf;

use sempal::analysis::embedding::EMBEDDING_DIM;
use sempal::dataset::curated;
use sempal::dataset::loader::{LoadedDataset, load_dataset};
use sempal::ml::logreg::{LogRegModel, TrainDataset, TrainOptions, default_head_id, train_logreg};
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
    let (train, val, test) = if manifest_path.is_file() {
        let loaded = load_dataset(&options.dataset_dir).map_err(|err| err.to_string())?;
        split_train_val_test(&loaded)?
    } else {
        let samples = curated::collect_training_samples(&options.dataset_dir)?;
        if samples.is_empty() {
            return Err("Training dataset folder is empty".to_string());
        }
        let samples = curated::filter_training_samples(samples, options.min_class_samples);
        if samples.is_empty() {
            return Err("Training dataset has no classes after hygiene filter".to_string());
        }
        let split_map =
            curated::stratified_split_map(&samples, "sempal-training-dataset-v1", 0.1, 0.1)?;
        curated::build_logreg_dataset_from_samples(
            &samples,
            &split_map,
            options.min_class_samples,
            &options.augmentation,
            options.seed,
        )?
    };

    let mut train_options = TrainOptions::default();
    train_options.epochs = options.epochs;
    train_options.learning_rate = options.learning_rate;
    train_options.l2 = options.l2;
    train_options.batch_size = options.batch_size;
    train_options.seed = options.seed;
    train_options.balance_classes = options.balance_classes;
    if train_options.batch_size == 0 {
        train_options.batch_size = 1;
    }

    let mut model = train_logreg(&train, &train_options, Some(&val))?;
    model.model_id = Some(default_head_id());
    model.temperature = options.temperature;
    save_model(&options.model_out, &model)?;

    let (acc, cm, per_class, top1, top3) = evaluate(&model, &test);
    println!("test accuracy: {:.4}", acc);
    println!("top-1 accuracy: {:.4}", top1);
    println!("top-3 accuracy: {:.4}", top3);
    for (idx, stats) in per_class.iter().enumerate() {
        let f1 = f1_score(stats.precision, stats.recall);
        println!(
            "class {:>2} {:<16}  precision={:.3}  recall={:.3}  f1={:.3}  support={}",
            idx,
            model.classes[idx],
            stats.precision,
            stats.recall,
            f1,
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
    epochs: usize,
    learning_rate: f32,
    l2: f32,
    batch_size: usize,
    seed: u64,
    balance_classes: bool,
    temperature: f32,
    min_class_samples: usize,
    augmentation: TrainingAugmentation,
}

#[derive(Clone)]
struct LabeledRow {
    sample_id: String,
    class_idx: usize,
    split: String,
    row: Vec<f32>,
}

fn parse_args(args: Vec<String>) -> Result<CliOptions, String> {
    let mut dataset_dir: Option<PathBuf> = None;
    let mut model_out = PathBuf::from("model.json");
    let mut epochs = 30usize;
    let mut learning_rate = 0.1f32;
    let mut l2 = 1e-4f32;
    let mut batch_size = 256usize;
    let mut seed = 42u64;
    let mut balance_classes = true;
    let mut temperature = 1.0f32;
    let mut min_class_samples = 30usize;
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
            "--epochs" => {
                idx += 1;
                let value = args.get(idx).ok_or_else(|| "--epochs requires a value".to_string())?;
                epochs = value
                    .parse::<usize>()
                    .map_err(|_| format!("Invalid --epochs value: {value}"))?;
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
                l2 = value
                    .parse::<f32>()
                    .map_err(|_| format!("Invalid --l2 value: {value}"))?;
            }
            "--batch-size" => {
                idx += 1;
                let value = args
                    .get(idx)
                    .ok_or_else(|| "--batch-size requires a value".to_string())?;
                batch_size = value
                    .parse::<usize>()
                    .map_err(|_| format!("Invalid --batch-size value: {value}"))?;
            }
            "--seed" => {
                idx += 1;
                let value = args.get(idx).ok_or_else(|| "--seed requires a value".to_string())?;
                seed = value
                    .parse::<u64>()
                    .map_err(|_| format!("Invalid --seed value: {value}"))?;
            }
            "--no-balance" => {
                balance_classes = false;
            }
            "--temperature" => {
                idx += 1;
                let value = args
                    .get(idx)
                    .ok_or_else(|| "--temperature requires a value".to_string())?;
                temperature = value
                    .parse::<f32>()
                    .map_err(|_| format!("Invalid --temperature value: {value}"))?;
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
        epochs,
        learning_rate,
        l2,
        batch_size,
        seed,
        balance_classes,
        temperature,
        min_class_samples,
        augmentation,
    })
}

fn help_text() -> String {
    [
        "sempal-train-logreg",
        "",
        "Trains a multinomial logistic regression classifier from an embedding dataset.",
        "",
        "Usage:",
        "  sempal-train-logreg --dataset <dir> [--out model.json] [options]",
        "",
        "Options:",
        "  --dataset <dir>        Dataset export (manifest.json) or curated class-folder root (required).",
        "  --out <file>           Output model path (default: model.json).",
        "  --epochs <n>           Epoch count (default: 30).",
        "  --learning-rate <f32>  Learning rate (default: 0.1).",
        "  --l2 <f32>             L2 regularization (default: 1e-4).",
        "  --batch-size <n>       Batch size (default: 256).",
        "  --seed <u64>           RNG seed (default: 42).",
        "  --temperature <f32>    Softmax temperature (default: 1.0).",
        "  --min-class-samples <n> Minimum samples per class for curated folders (default: 30).",
        "  --augment             Enable default augmentation for curated folders.",
        "  --no-balance           Disable class-balanced loss weights.",
    ]
    .join("\n")
}

fn split_train_val_test(
    loaded: &LoadedDataset,
) -> Result<(TrainDataset, TrainDataset, TrainDataset), String> {
    if loaded.manifest.feature_len_f32 != EMBEDDING_DIM {
        return Err(format!(
            "Unsupported embedding dimension {} (expected {})",
            loaded.manifest.feature_len_f32, EMBEDDING_DIM
        ));
    }

    let class_map = loaded.class_index_map();
    let classes: Vec<String> = class_map
        .iter()
        .map(|(name, _)| name.clone())
        .collect();

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
            classes: classes.clone(),
            x: train_x,
            y: train_y,
        },
        TrainDataset {
            classes: classes.clone(),
            x: val_x,
            y: val_y,
        },
        TrainDataset {
            classes,
            x: test_x,
            y: test_y,
        },
    ))
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

fn save_model(path: &PathBuf, model: &LogRegModel) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(model).map_err(|err| err.to_string())?;
    std::fs::write(path, bytes).map_err(|err| err.to_string())
}

fn evaluate(
    model: &LogRegModel,
    dataset: &TrainDataset,
) -> (
    f32,
    ConfusionMatrix,
    Vec<sempal::ml::metrics::PerClassStats>,
    f32,
    f32,
) {
    let mut cm = ConfusionMatrix::new(model.classes.len());
    let mut top1_hits = 0usize;
    let mut top3_hits = 0usize;
    for (row, &truth) in dataset.x.iter().zip(dataset.y.iter()) {
        let proba = model.predict_proba(row);
        if proba.is_empty() {
            continue;
        }
        let predicted = argmax(&proba);
        cm.add(truth, predicted);
        if predicted == truth {
            top1_hits += 1;
        }
        if topk_contains(&proba, truth, 3) {
            top3_hits += 1;
        }
    }
    let acc = accuracy(&cm);
    let per_class = precision_recall_by_class(&cm);
    let total = dataset.x.len().max(1) as f32;
    let top1 = (top1_hits as f32) / total;
    let top3 = (top3_hits as f32) / total;
    (acc, cm, per_class, top1, top3)
}

fn argmax(values: &[f32]) -> usize {
    let mut best = 0usize;
    let mut best_val = f32::NEG_INFINITY;
    for (idx, &val) in values.iter().enumerate() {
        if val > best_val {
            best_val = val;
            best = idx;
        }
    }
    best
}

fn topk_contains(values: &[f32], target: usize, k: usize) -> bool {
    let mut pairs: Vec<(usize, f32)> = values.iter().cloned().enumerate().collect();
    pairs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    pairs.iter().take(k).any(|(idx, _)| *idx == target)
}

fn f1_score(precision: f32, recall: f32) -> f32 {
    if precision + recall == 0.0 {
        0.0
    } else {
        2.0 * precision * recall / (precision + recall)
    }
}
