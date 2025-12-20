//! Developer utility to import a trained model into the local library database.

use rusqlite::{Connection, params};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let options = parse_args(std::env::args().skip(1).collect())?;

    let model_json =
        std::fs::read_to_string(&options.model_path).map_err(|err| err.to_string())?;
    let (kind, model_version, feat_version, feature_len_f32, classes_json) = match options
        .kind
        .as_str()
    {
        "logreg" | "logreg_v1" => {
            let model: sempal::ml::logreg::LogRegModel =
                serde_json::from_str(&model_json).map_err(|err| err.to_string())?;
            let classes_json =
                serde_json::to_string(&model.classes).map_err(|err| err.to_string())?;
            (
                "logreg_v1",
                model.model_version,
                0,
                model.embedding_dim,
                classes_json,
            )
        }
        "mlp" | "mlp_v1" => {
            let model: sempal::ml::mlp::MlpModel =
                serde_json::from_str(&model_json).map_err(|err| err.to_string())?;
            let classes_json =
                serde_json::to_string(&model.classes).map_err(|err| err.to_string())?;
            (
                "mlp_v1",
                model.model_version,
                model.feat_version,
                model.feature_len_f32,
                classes_json,
            )
        }
        _ => {
            let model = sempal::ml::gbdt_stump::GbdtStumpModel::load_json(&options.model_path)?;
            let classes_json =
                serde_json::to_string(&model.classes).map_err(|err| err.to_string())?;
            (
                "gbdt_stump_v1",
                model.model_version,
                model.feat_version,
                model.feature_len_f32,
                classes_json,
            )
        }
    };
    let created_at = now_epoch_seconds();
    let model_id = uuid::Uuid::new_v4().to_string();

    let db_path = match options.db_path {
        Some(path) => path,
        None => {
            let _ = sempal::sample_sources::library::load().map_err(|err| err.to_string())?;
            let root = sempal::app_dirs::app_root_dir().map_err(|err| err.to_string())?;
            root.join(sempal::sample_sources::library::LIBRARY_DB_FILE_NAME)
        }
    };
    let conn = open_db(&db_path)?;
    conn.execute(
        "INSERT INTO models (
            model_id, kind, model_version, feat_version, feature_len_f32, classes_json, model_json, created_at
         )
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            model_id,
            kind,
            model_version,
            feat_version,
            feature_len_f32 as i64,
            classes_json,
            model_json,
            created_at
        ],
    )
    .map_err(|err| format!("Failed to insert model: {err}"))?;

    println!(
        "Imported model_id={} into {}",
        model_id,
        db_path.display()
    );
    Ok(())
}

#[derive(Debug, Clone)]
struct CliOptions {
    model_path: PathBuf,
    db_path: Option<PathBuf>,
    kind: String,
}

fn parse_args(args: Vec<String>) -> Result<CliOptions, String> {
    let mut model_path: Option<PathBuf> = None;
    let mut db_path: Option<PathBuf> = None;
    let mut kind = "gbdt".to_string();

    let mut idx = 0usize;
    while idx < args.len() {
        match args[idx].as_str() {
            "-h" | "--help" => return Err(help_text()),
            "--model" => {
                idx += 1;
                let value = args.get(idx).ok_or_else(|| "--model requires a value".to_string())?;
                model_path = Some(PathBuf::from(value));
            }
            "--db" => {
                idx += 1;
                let value = args.get(idx).ok_or_else(|| "--db requires a value".to_string())?;
                db_path = Some(PathBuf::from(value));
            }
            "--kind" => {
                idx += 1;
                let value = args.get(idx).ok_or_else(|| "--kind requires a value".to_string())?;
                kind = value.to_string();
            }
            unknown => return Err(format!("Unknown argument: {unknown}\n\n{}", help_text())),
        }
        idx += 1;
    }

    let model_path = model_path.ok_or_else(|| help_text())?;
    Ok(CliOptions {
        model_path,
        db_path,
        kind,
    })
}

fn help_text() -> String {
    [
        "sempal-model-import",
        "",
        "Imports a model JSON file (e.g. produced by sempal-train-baseline) into the library database.",
        "",
        "Usage:",
        "  sempal-model-import --model <model.json> [--db <library.db>] [--kind gbdt|mlp|logreg]",
    ]
    .join("\n")
}

fn open_db(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(path).map_err(|err| format!("Open DB failed: {err}"))?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys=ON;",
    )
    .map_err(|err| format!("Failed to set DB pragmas: {err}"))?;
    Ok(conn)
}

fn now_epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
