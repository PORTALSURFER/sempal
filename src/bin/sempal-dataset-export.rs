//! Developer utility to export a training dataset from the local library database.

use std::path::PathBuf;

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let Some(options) = parse_args(std::env::args().skip(1).collect())? else {
        return Ok(());
    };
    let db_path = options
        .resolved_db_path()
        .map_err(|err| err.to_string())?;
    println!("Using DB: {}", db_path.display());
    let summary = sempal::dataset::export::export_training_dataset(&options)
        .map_err(|err| err.to_string())?;
    println!(
        "Exported {} samples across {} packs into {}",
        summary.total_exported,
        summary.total_packs,
        options.out_dir.display()
    );
    if !summary.class_counts.is_empty() {
        println!("Per-class counts:");
        for (class_id, count) in &summary.class_counts {
            println!("  {class_id}: {count}");
        }
    }
    if summary.total_exported == 0 {
        let diag = sempal::dataset::export::diagnose_export(&options).map_err(|err| err.to_string())?;
        println!("Diagnostic tables: {}", diag.tables.join(", "));
        if let Some(n) = diag.samples {
            println!("samples: {n}");
        }
        if let Some(n) = diag.features_v1 {
            println!("features(feat_version=1): {n}");
        }
        if let Some(n) = diag.labels_user_total {
            println!("labels_user: {n}");
        }
        if let Some(n) = diag.labels_weak_ruleset_ge_conf {
            println!(
                "labels_weak(ruleset={}, conf>={:.2}): {n}",
                sempal::labeling::weak::WEAK_LABEL_RULESET_VERSION,
                options.min_confidence
            );
        }
        if let Some(n) = diag.join_rows_user {
            println!("features⋈labels_user join rows: {n}");
        }
        if let Some(n) = diag.join_rows {
            println!("features⋈labels_weak join rows: {n}");
        }
        println!("Hints:");
        println!("- Make sure the GUI app has scanned your sources and finished analysis.");
        println!("- Try lowering --min-confidence (e.g. 0.70) to include more weak labels.");
        println!("- If you're exporting from a different machine/profile, pass --db <path-to-library.db>.");
    }
    Ok(())
}

fn parse_args(args: Vec<String>) -> Result<Option<sempal::dataset::export::ExportOptions>, String> {
    let mut options = sempal::dataset::export::ExportOptions::default();

    let mut idx = 0usize;
    while idx < args.len() {
        match args[idx].as_str() {
            "-h" | "--help" => {
                println!("{}", help_text());
                return Ok(None);
            }
            "--out" => {
                idx += 1;
                let value = args.get(idx).ok_or_else(|| "--out requires a value".to_string())?;
                options.out_dir = PathBuf::from(value);
            }
            "--db" => {
                idx += 1;
                let value = args.get(idx).ok_or_else(|| "--db requires a value".to_string())?;
                options.db_path = Some(PathBuf::from(value));
            }
            "--min-confidence" => {
                idx += 1;
                let value = args
                    .get(idx)
                    .ok_or_else(|| "--min-confidence requires a value".to_string())?;
                options.min_confidence = value
                    .parse::<f32>()
                    .map_err(|_| format!("Invalid --min-confidence value: {value}"))?;
            }
            "--pack-depth" => {
                idx += 1;
                let value =
                    args.get(idx).ok_or_else(|| "--pack-depth requires a value".to_string())?;
                options.pack_depth = value
                    .parse::<usize>()
                    .map_err(|_| format!("Invalid --pack-depth value: {value}"))?;
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
            unknown => {
                return Err(format!("Unknown argument: {unknown}\n\n{}", help_text()));
            }
        }
        idx += 1;
    }

    if options.out_dir.as_os_str().is_empty() {
        return Err("--out is required".to_string());
    }

    Ok(Some(options))
}

fn help_text() -> String {
    [
        "sempal-dataset-export",
        "",
        "Exports features + weak labels into a deterministic pack-split dataset.",
        "",
        "Usage:",
        "  sempal-dataset-export --out <dir> [--db <path>] [options]",
        "",
        "Options:",
        "  --out <dir>             Output directory (required).",
        "  --db <path>             Path to library.db (defaults to app data location).",
        "  --min-confidence <f32>  Minimum label confidence (default: 0.85).",
        "  --pack-depth <usize>    Folder components used for pack_id (default: 1).",
        "  --seed <string>         Seed for deterministic pack split (default: sempal-dataset-v1).",
        "  --test-fraction <f64>   Pack fraction assigned to test (default: 0.1).",
        "  --val-fraction <f64>    Pack fraction assigned to val (default: 0.1).",
    ]
    .join("\n")
}
