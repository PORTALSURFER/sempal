//! Developer utility to export an embedding dataset from curated class folders.

use std::io::Write;
use std::path::PathBuf;

use sempal::dataset::curated::{
    CuratedExportOptions, TrainingProgress, export_curated_embedding_dataset_with_progress,
};

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
    println!("Scanning curated dataset...");
    println!(
        "Exporting to {} (this uses the CLAP embedding model)",
        options.out_dir.display()
    );
    let mut last_print = 0usize;
    let mut progress_cb = |update: TrainingProgress| {
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
    let mut progress = Some(&mut progress_cb);
    let summary = export_curated_embedding_dataset_with_progress(&options, &mut progress)?;
    println!(
        "Exported {} rows from {} samples across {} classes",
        summary.total_exported, summary.total_samples, summary.total_classes
    );
    if summary.skipped > 0 {
        println!("Skipped {} samples during export", summary.skipped);
    }
    if !summary.class_counts.is_empty() {
        println!("Per-class counts:");
        for (class_id, count) in summary.class_counts {
            println!("  {class_id}: {count}");
        }
    }
    Ok(())
}

fn parse_args(args: Vec<String>) -> Result<CuratedExportOptions, String> {
    let mut options = CuratedExportOptions::default();
    let mut idx = 0usize;
    while idx < args.len() {
        match args[idx].as_str() {
            "-h" | "--help" => return Err(help_text()),
            "--dataset" => {
                idx += 1;
                let value = args.get(idx).ok_or_else(|| "--dataset requires a value".to_string())?;
                options.dataset_dir = PathBuf::from(value);
            }
            "--out" => {
                idx += 1;
                let value = args.get(idx).ok_or_else(|| "--out requires a value".to_string())?;
                options.out_dir = PathBuf::from(value);
            }
            "--min-class-samples" => {
                idx += 1;
                let value = args
                    .get(idx)
                    .ok_or_else(|| "--min-class-samples requires a value".to_string())?;
                options.min_class_samples = value
                    .parse::<usize>()
                    .map_err(|_| format!("Invalid --min-class-samples value: {value}"))?;
            }
            "--seed" => {
                idx += 1;
                let value = args.get(idx).ok_or_else(|| "--seed requires a value".to_string())?;
                options.seed = value.to_string();
            }
            "--test-fraction" => {
                idx += 1;
                let value = args
                    .get(idx)
                    .ok_or_else(|| "--test-fraction requires a value".to_string())?;
                options.test_fraction = value
                    .parse::<f64>()
                    .map_err(|_| format!("Invalid --test-fraction value: {value}"))?;
            }
            "--val-fraction" => {
                idx += 1;
                let value = args
                    .get(idx)
                    .ok_or_else(|| "--val-fraction requires a value".to_string())?;
                options.val_fraction = value
                    .parse::<f64>()
                    .map_err(|_| format!("Invalid --val-fraction value: {value}"))?;
            }
            "--pack-depth" => {
                idx += 1;
                let value =
                    args.get(idx).ok_or_else(|| "--pack-depth requires a value".to_string())?;
                options.pack_depth = value
                    .parse::<usize>()
                    .map_err(|_| format!("Invalid --pack-depth value: {value}"))?;
            }
            "--augment" => {
                options.augmentation.enabled = true;
            }
            unknown => {
                return Err(format!("Unknown argument: {unknown}\n\n{}", help_text()));
            }
        }
        idx += 1;
    }
    if options.dataset_dir.as_os_str().is_empty() {
        return Err("--dataset is required".to_string());
    }
    if options.out_dir.as_os_str().is_empty() {
        return Err("--out is required".to_string());
    }
    Ok(options)
}

fn help_text() -> String {
    [
        "sempal-dataset-export-curated",
        "",
        "Exports a manifest dataset from a folder-per-class curated dataset.",
        "",
        "Usage:",
        "  sempal-dataset-export-curated --dataset <dir> --out <dir> [options]",
        "",
        "Options:",
        "  --dataset <dir>          Curated dataset root folder (required).",
        "  --out <dir>              Output directory (required).",
        "  --min-class-samples <n>  Minimum samples per class (default: 30).",
        "  --seed <string>          Seed for stratified split (default: sempal-curated-dataset-v1).",
        "  --test-fraction <f64>    Test fraction (default: 0.1).",
        "  --val-fraction <f64>     Val fraction (default: 0.1).",
        "  --pack-depth <usize>     Pack folder depth in pack_id (default: 1).",
        "  --augment                Enable training-time augmentation copies.",
    ]
    .join("\n")
}
