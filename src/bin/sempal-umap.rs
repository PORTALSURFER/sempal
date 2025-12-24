//! Developer utility to build a t-SNE layout from stored embeddings.

use rusqlite::Connection;
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
    let db_path = resolve_db_path(options.db_path.as_ref())?;
    let conn = Connection::open(&db_path).map_err(|err| format!("Open DB failed: {err}"))?;
    let mut conn = conn;
    let report = sempal::analysis::umap::build_umap_layout(
        &mut conn,
        &options.model_id,
        &options.umap_version,
        options.seed,
        options.min_coverage,
    )?;
    println!(
        "Built t-SNE layout for {} samples (coverage {:.2}%)",
        report.total,
        report.coverage_ratio * 100.0
    );
    let report_path = sempal::analysis::umap::default_report_path(&db_path, &options.umap_version);
    sempal::analysis::umap::write_report(&report_path, &report)?;
    println!(
        "t-SNE coverage {:.2}% (report: {})",
        report.coverage_ratio * 100.0,
        report_path.display()
    );
    Ok(())
}

#[derive(Debug, Clone)]
struct Options {
    db_path: Option<PathBuf>,
    model_id: String,
    umap_version: String,
    seed: u64,
    min_coverage: f32,
}

fn parse_args(args: Vec<String>) -> Result<Option<Options>, String> {
    let mut options = Options {
        db_path: None,
        model_id: String::new(),
        umap_version: String::new(),
        seed: 0,
        min_coverage: 0.95,
    };

    let mut idx = 0usize;
    while idx < args.len() {
        match args[idx].as_str() {
            "-h" | "--help" => {
                println!("{}", help_text());
                return Ok(None);
            }
            "--db" => {
                idx += 1;
                let value = args
                    .get(idx)
                    .ok_or_else(|| "--db requires a value".to_string())?;
                options.db_path = Some(PathBuf::from(value));
            }
            "--model-id" => {
                idx += 1;
                let value = args
                    .get(idx)
                    .ok_or_else(|| "--model-id requires a value".to_string())?;
                options.model_id = value.to_string();
            }
            "--umap-version" => {
                idx += 1;
                let value = args
                    .get(idx)
                    .ok_or_else(|| "--umap-version requires a value".to_string())?;
                options.umap_version = value.to_string();
            }
            "--seed" => {
                idx += 1;
                let value = args
                    .get(idx)
                    .ok_or_else(|| "--seed requires a value".to_string())?;
                options.seed = value
                    .parse::<u64>()
                    .map_err(|_| format!("Invalid --seed value: {value}"))?;
            }
            "--min-coverage" => {
                idx += 1;
                let value = args
                    .get(idx)
                    .ok_or_else(|| "--min-coverage requires a value".to_string())?;
                options.min_coverage = value
                    .parse::<f32>()
                    .map_err(|_| format!("Invalid --min-coverage value: {value}"))?;
                if !(0.0..=1.0).contains(&options.min_coverage) {
                    return Err("--min-coverage must be between 0.0 and 1.0".to_string());
                }
            }
            unknown => {
                return Err(format!("Unknown argument: {unknown}\n\n{}", help_text()));
            }
        }
        idx += 1;
    }

    if options.model_id.trim().is_empty() {
        return Err("--model-id is required".to_string());
    }
    if options.umap_version.trim().is_empty() {
        return Err("--umap-version is required".to_string());
    }

    Ok(Some(options))
}

fn help_text() -> String {
    [
        "sempal-umap",
        "",
        "Build a t-SNE layout for stored embeddings.",
        "",
        "Usage:",
        "  sempal-umap --model-id <id> --umap-version <version> [--db <path>] [--seed <u64>]",
        "",
        "Options:",
        "  --db <path>          Path to library.db (defaults to app data location).",
        "  --model-id <id>      Embedding model id to read (required).",
        "  --umap-version <v>   Layout version tag to store (required).",
        "  --seed <u64>         Seed for deterministic layouts (default: 0).",
        "  --min-coverage <f>   Fail if coverage below threshold (default: 0.95).",
    ]
    .join("\n")
}

fn resolve_db_path(db_path: Option<&PathBuf>) -> Result<PathBuf, String> {
    if let Some(path) = db_path {
        return Ok(path.clone());
    }
    let root = sempal::app_dirs::app_root_dir().map_err(|err| err.to_string())?;
    Ok(root.join(sempal::sample_sources::library::LIBRARY_DB_FILE_NAME))
}
