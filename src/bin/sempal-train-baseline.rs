//! Developer utility to train and export a baseline classifier.

use std::path::PathBuf;

use sempal::analysis::{FEATURE_VECTOR_LEN_V1, FEATURE_VERSION_V1};
use sempal::dataset::curated;
use sempal::dataset::loader::{LoadedDataset, load_dataset};
use sempal::ml::gbdt_stump::{TrainDataset, TrainOptions, train_gbdt_stump};
use sempal::ml::metrics::{ConfusionMatrix, accuracy, precision_recall_by_class};

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
    let (train, test) = if manifest_path.is_file() {
        let loaded = load_dataset(&options.dataset_dir).map_err(|err| err.to_string())?;
        split_train_test(&loaded)?
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
        curated::build_feature_dataset_from_samples(&samples, &split_map)?
    };

    let train_options = TrainOptions {
        rounds: options.rounds,
        learning_rate: options.learning_rate,
        bins: options.bins,
    };
    let model = train_gbdt_stump(&train, &train_options)?;
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
    rounds: usize,
    learning_rate: f32,
    bins: usize,
    min_class_samples: usize,
}

fn parse_args(args: Vec<String>) -> Result<CliOptions, String> {
    let mut dataset_dir: Option<PathBuf> = None;
    let mut model_out = PathBuf::from("model.json");
    let mut rounds = 100usize;
    let mut learning_rate = 0.1f32;
    let mut bins = 32usize;
    let mut min_class_samples = 30usize;

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
            "--rounds" => {
                idx += 1;
                let value = args.get(idx).ok_or_else(|| "--rounds requires a value".to_string())?;
                rounds = value
                    .parse::<usize>()
                    .map_err(|_| format!("Invalid --rounds value: {value}"))?;
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
            "--bins" => {
                idx += 1;
                let value = args.get(idx).ok_or_else(|| "--bins requires a value".to_string())?;
                bins = value
                    .parse::<usize>()
                    .map_err(|_| format!("Invalid --bins value: {value}"))?;
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
            unknown => return Err(format!("Unknown argument: {unknown}\n\n{}", help_text())),
        }
        idx += 1;
    }

    let dataset_dir = dataset_dir.ok_or_else(|| help_text())?;
    Ok(CliOptions {
        dataset_dir,
        model_out,
        rounds,
        learning_rate,
        bins,
        min_class_samples,
    })
}

fn help_text() -> String {
    [
        "sempal-train-baseline",
        "",
        "Trains a deterministic gradient-boosted stump classifier from a dataset export.",
        "",
        "Usage:",
        "  sempal-train-baseline --dataset <dir> [--out model.json] [options]",
        "",
        "Options:",
        "  --dataset <dir>        Dataset export (manifest.json) or curated class-folder root (required).",
        "  --out <file>           Output model path (default: model.json).",
        "  --rounds <n>           Boosting rounds (default: 100).",
        "  --learning-rate <f32>  Learning rate (default: 0.1).",
        "  --bins <n>             Feature bin count for split search (default: 32).",
        "  --min-class-samples <n> Minimum samples per class for curated folders (default: 30).",
    ]
    .join("\n")
}

fn split_train_test(loaded: &LoadedDataset) -> Result<(TrainDataset, TrainDataset), String> {
    if loaded.manifest.feat_version != FEATURE_VERSION_V1 {
        return Err(format!(
            "Unsupported feat_version {} (expected {})",
            loaded.manifest.feat_version, FEATURE_VERSION_V1
        ));
    }
    if loaded.manifest.feature_len_f32 != FEATURE_VECTOR_LEN_V1 {
        return Err(format!(
            "Unsupported feature_len_f32 {} (expected {})",
            loaded.manifest.feature_len_f32, FEATURE_VECTOR_LEN_V1
        ));
    }

    let class_map = loaded.class_index_map();
    let classes: Vec<String> = class_map
        .iter()
        .map(|(name, _)| name.clone())
        .collect();

    let mut train_x = Vec::new();
    let mut train_y = Vec::new();
    let mut test_x = Vec::new();
    let mut test_y = Vec::new();

    for sample in &loaded.samples {
        let Some(row) = loaded.feature_row(sample) else {
            continue;
        };
        let Some(&class_idx) = class_map.get(&sample.label.class_id) else {
            continue;
        };
        match sample.split.as_str() {
            "train" => {
                train_x.push(row.to_vec());
                train_y.push(class_idx);
            }
            "test" => {
                test_x.push(row.to_vec());
                test_y.push(class_idx);
            }
            // Keep validation for future tuning.
            _ => {}
        }
    }

    if train_x.is_empty() || test_x.is_empty() {
        return Err("Dataset needs both train and test samples".to_string());
    }

    Ok((
        TrainDataset {
            feature_len_f32: FEATURE_VECTOR_LEN_V1,
            feat_version: FEATURE_VERSION_V1,
            classes: classes.clone(),
            x: train_x,
            y: train_y,
        },
        TrainDataset {
            feature_len_f32: FEATURE_VECTOR_LEN_V1,
            feat_version: FEATURE_VERSION_V1,
            classes,
            x: test_x,
            y: test_y,
        },
    ))
}

fn save_model(
    path: &PathBuf,
    model: &sempal::ml::gbdt_stump::GbdtStumpModel,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(model).map_err(|err| err.to_string())?;
    std::fs::write(path, bytes).map_err(|err| err.to_string())
}

fn evaluate(
    model: &sempal::ml::gbdt_stump::GbdtStumpModel,
    dataset: &TrainDataset,
) -> (
    f32,
    ConfusionMatrix,
    Vec<sempal::ml::metrics::PerClassStats>,
) {
    let mut cm = ConfusionMatrix::new(model.classes.len());
    for (row, &truth) in dataset.x.iter().zip(dataset.y.iter()) {
        let predicted = model.predict_class_index(row);
        cm.add(truth, predicted);
    }
    let acc = accuracy(&cm);
    let per_class = precision_recall_by_class(&cm);
    (acc, cm, per_class)
}
