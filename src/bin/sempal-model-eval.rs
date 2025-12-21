//! Developer utility to evaluate a model against an exported dataset.

use std::path::PathBuf;

use sempal::dataset::loader::load_dataset;
use sempal::ml::metrics::{ConfusionMatrix, accuracy, precision_recall_by_class};

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

#[derive(Debug, Clone)]
struct CliOptions {
    model_path: PathBuf,
    dataset_dir: PathBuf,
    kind: ModelKind,
    split: String,
    top: usize,
}

#[derive(Debug, Clone, Copy)]
enum ModelKind {
    Auto,
    LogReg,
    Mlp,
}

#[derive(Debug, Clone)]
struct MisclassifiedSample {
    sample_id: String,
    truth: String,
    predicted: String,
    confidence: f32,
}

fn run() -> Result<(), String> {
    let options = parse_args(std::env::args().skip(1).collect())?;
    let dataset = load_dataset(&options.dataset_dir).map_err(|err| err.to_string())?;
    let model_json =
        std::fs::read_to_string(&options.model_path).map_err(|err| err.to_string())?;

    let model = load_model(&model_json, options.kind)?;
    let class_map = model.class_index_map();
    let mut cm = ConfusionMatrix::new(model.classes().len());
    let mut misclassified = Vec::new();

    for sample in &dataset.samples {
        if options.split != "all" && sample.split != options.split {
            continue;
        }
        let Some(row) = dataset.feature_row(sample) else {
            continue;
        };
        let Some(&truth_idx) = class_map.get(&sample.label.class_id) else {
            continue;
        };
        let proba = model.predict_proba(row);
        if proba.len() != model.classes().len() {
            continue;
        }
        let (pred_idx, confidence) = argmax(&proba);
        cm.add(truth_idx, pred_idx);
        if pred_idx != truth_idx {
            misclassified.push(MisclassifiedSample {
                sample_id: sample.sample_id.clone(),
                truth: model.classes()[truth_idx].clone(),
                predicted: model.classes()[pred_idx].clone(),
                confidence,
            });
        }
    }

    let acc = accuracy(&cm);
    println!("accuracy: {:.4}", acc);
    let per_class = precision_recall_by_class(&cm);
    for (idx, stats) in per_class.iter().enumerate() {
        println!(
            "class {:>2} {:<16}  precision={:.3}  recall={:.3}  support={}",
            idx,
            model.classes()[idx],
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

    if !misclassified.is_empty() {
        println!();
        println!("Top misclassified samples (highest confidence):");
        misclassified.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());
        for item in misclassified.iter().take(options.top) {
            println!(
                "- {}  truth={}  pred={}  conf={:.3}",
                item.sample_id, item.truth, item.predicted, item.confidence
            );
        }
    }

    println!();
    println!("Top confusions:");
    let mut confusions = Vec::new();
    for truth in 0..cm.n_classes {
        for pred in 0..cm.n_classes {
            if truth == pred {
                continue;
            }
            let count = cm.get(truth, pred);
            if count > 0 {
                confusions.push((count, truth, pred));
            }
        }
    }
    confusions.sort_by(|a, b| b.0.cmp(&a.0));
    for (count, truth, pred) in confusions.into_iter().take(options.top) {
        println!(
            "- {} -> {}: {}",
            model.classes()[truth],
            model.classes()[pred],
            count
        );
    }

    Ok(())
}

fn parse_args(args: Vec<String>) -> Result<CliOptions, String> {
    let mut model_path: Option<PathBuf> = None;
    let mut dataset_dir: Option<PathBuf> = None;
    let mut kind = ModelKind::Auto;
    let mut split = "test".to_string();
    let mut top = 20usize;

    let mut idx = 0usize;
    while idx < args.len() {
        match args[idx].as_str() {
            "-h" | "--help" => return Err(help_text()),
            "--model" => {
                idx += 1;
                let value = args.get(idx).ok_or_else(|| "--model requires a value".to_string())?;
                model_path = Some(PathBuf::from(value));
            }
            "--dataset" => {
                idx += 1;
                let value =
                    args.get(idx).ok_or_else(|| "--dataset requires a value".to_string())?;
                dataset_dir = Some(PathBuf::from(value));
            }
            "--kind" => {
                idx += 1;
                let value = args.get(idx).ok_or_else(|| "--kind requires a value".to_string())?;
                kind = match value.as_str() {
                    "auto" => ModelKind::Auto,
                    "logreg" => ModelKind::LogReg,
                    "mlp" => ModelKind::Mlp,
                    _ => return Err(format!("Invalid --kind value: {value}")),
                };
            }
            "--split" => {
                idx += 1;
                let value = args.get(idx).ok_or_else(|| "--split requires a value".to_string())?;
                split = value.to_string();
            }
            "--top" => {
                idx += 1;
                let value = args.get(idx).ok_or_else(|| "--top requires a value".to_string())?;
                top = value
                    .parse::<usize>()
                    .map_err(|_| format!("Invalid --top value: {value}"))?;
            }
            unknown => return Err(format!("Unknown argument: {unknown}\n\n{}", help_text())),
        }
        idx += 1;
    }

    let model_path = model_path.ok_or_else(|| "--model is required".to_string())?;
    let dataset_dir = dataset_dir.ok_or_else(|| "--dataset is required".to_string())?;
    Ok(CliOptions {
        model_path,
        dataset_dir,
        kind,
        split,
        top,
    })
}

fn help_text() -> String {
    [
        "sempal-model-eval",
        "",
        "Usage:",
        "  sempal-model-eval --model <model.json> --dataset <dir> [options]",
        "",
        "Options:",
        "  --kind <auto|logreg|mlp>  Model type (default: auto).",
        "  --split <train|val|test|all>  Split filter (default: test).",
        "  --top <n>                Top N confusions/misclassifications (default: 20).",
    ]
    .join("\n")
}

fn argmax(values: &[f32]) -> (usize, f32) {
    let mut best_idx = 0usize;
    let mut best_val = f32::NEG_INFINITY;
    for (idx, &value) in values.iter().enumerate() {
        if value > best_val {
            best_val = value;
            best_idx = idx;
        }
    }
    (best_idx, best_val)
}

enum ModelWrapper {
    LogReg(sempal::ml::logreg::LogRegModel),
    Mlp(sempal::ml::mlp::MlpModel),
}

impl ModelWrapper {
    fn classes(&self) -> &Vec<String> {
        match self {
            ModelWrapper::LogReg(model) => &model.classes,
            ModelWrapper::Mlp(model) => &model.classes,
        }
    }

    fn class_index_map(&self) -> std::collections::BTreeMap<String, usize> {
        self.classes()
            .iter()
            .cloned()
            .enumerate()
            .map(|(idx, name)| (name, idx))
            .collect()
    }

    fn predict_proba(&self, row: &[f32]) -> Vec<f32> {
        match self {
            ModelWrapper::LogReg(model) => model.predict_proba(row),
            ModelWrapper::Mlp(model) => model.predict_proba(row),
        }
    }
}

fn load_model(model_json: &str, kind: ModelKind) -> Result<ModelWrapper, String> {
    match kind {
        ModelKind::LogReg => {
            let model: sempal::ml::logreg::LogRegModel =
                serde_json::from_str(model_json).map_err(|err| err.to_string())?;
            Ok(ModelWrapper::LogReg(model))
        }
        ModelKind::Mlp => {
            let model: sempal::ml::mlp::MlpModel =
                serde_json::from_str(model_json).map_err(|err| err.to_string())?;
            Ok(ModelWrapper::Mlp(model))
        }
        ModelKind::Auto => {
            if let Ok(model) = serde_json::from_str::<sempal::ml::mlp::MlpModel>(model_json) {
                return Ok(ModelWrapper::Mlp(model));
            }
            if let Ok(model) = serde_json::from_str::<sempal::ml::logreg::LogRegModel>(model_json) {
                return Ok(ModelWrapper::LogReg(model));
            }
            Err("Unable to parse model.json (expected mlp or logreg)".to_string())
        }
    }
}
