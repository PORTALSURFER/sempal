//! Developer utility to train and export an MLP classifier.

use std::path::PathBuf;

use sempal::analysis::{FEATURE_VECTOR_LEN_V1, FEATURE_VERSION_V1};
use sempal::dataset::loader::{LoadedDataset, load_dataset};
use sempal::ml::gbdt_stump::TrainDataset;
use sempal::ml::mlp::{MlpInputKind, TrainOptions, train_mlp};
use sempal::ml::metrics::{ConfusionMatrix, accuracy, precision_recall_by_class};

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let options = parse_args(std::env::args().skip(1).collect())?;
    let loaded = load_dataset(&options.dataset_dir).map_err(|err| err.to_string())?;
    let (train, test) = split_train_test(&loaded)?;

    let train_options = TrainOptions {
        hidden_size: options.hidden_size,
        epochs: options.epochs,
        batch_size: options.batch_size,
        learning_rate: options.learning_rate,
        l2_penalty: options.l2_penalty,
        dropout: 0.15,
        label_smoothing: 0.05,
        balance_classes: true,
        input_kind: MlpInputKind::FeaturesV1,
        seed: options.seed,
    };
    let model = train_mlp(&train, &train_options)?;
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
    })
}

fn help_text() -> String {
    [
        "sempal-train-mlp",
        "",
        "Trains an MLP classifier on a dataset export and writes model.json.",
        "",
        "Usage:",
        "  sempal-train-mlp --dataset <dataset_dir> [--out model.json]",
        "",
        "Options:",
        "  --hidden <n>         Hidden layer size (default 128)",
        "  --epochs <n>         Training epochs (default 20)",
        "  --batch <n>          Batch size (default 128)",
        "  --learning-rate <f>  Learning rate (default 0.01)",
        "  --l2 <f>             L2 penalty (default 1e-4)",
        "  --seed <n>           RNG seed (default 42)",
    ]
    .join("\n")
}

fn split_train_test(
    loaded: &LoadedDataset,
) -> Result<(TrainDataset, TrainDataset), String> {
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
    let classes: Vec<String> = class_map.iter().map(|(name, _)| name.clone()).collect();

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
            "train" | "val" => {
                train_x.push(row.to_vec());
                train_y.push(class_idx);
            }
            "test" => {
                test_x.push(row.to_vec());
                test_y.push(class_idx);
            }
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
