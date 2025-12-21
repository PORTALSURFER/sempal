//! Developer utility to list models and set the active classifier model.

use rusqlite::{Connection, OptionalExtension, params};
use std::path::PathBuf;

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

enum Command {
    List { db_path: PathBuf, limit: usize },
    Set { db_path: PathBuf, mode: SetMode },
}

enum SetMode {
    ModelId(String),
    Latest,
    Clear,
}

fn run() -> Result<(), String> {
    let cmd = parse_args(std::env::args().skip(1).collect())?;
    match cmd {
        Command::List { db_path, limit } => list_models(&db_path, limit),
        Command::Set { db_path, mode } => set_active_model(&db_path, mode),
    }
}

fn list_models(db_path: &PathBuf, limit: usize) -> Result<(), String> {
    let conn = open_db(db_path)?;
    let config = sempal::sample_sources::config::load_or_default()
        .map_err(|err| err.to_string())?;
    let active = config.model.classifier_model_id.trim().to_string();
    let active_label = if active.is_empty() {
        "—".to_string()
    } else {
        active
    };
    println!("DB: {}", db_path.display());
    println!("Active model id: {active_label}");

    let mut stmt = conn
        .prepare(
            "SELECT model_id, kind, created_at, feature_len_f32, classes_json
             FROM models
             ORDER BY created_at DESC, model_id DESC
             LIMIT ?1",
        )
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map(params![limit as i64], |row| {
            let classes_json: String = row.get(4)?;
            let class_count = serde_json::from_str::<Vec<String>>(&classes_json)
                .ok()
                .map(|v| v.len())
                .unwrap_or(0);
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                class_count,
            ))
        })
        .map_err(|err| err.to_string())?;
    println!();
    println!("Models:");
    for row in rows {
        let (model_id, kind, created_at, feature_len, class_count) =
            row.map_err(|err| err.to_string())?;
        println!(
            "- {model_id} | {kind} | classes={class_count} | feature_len={feature_len} | created_at={created_at}"
        );
    }
    Ok(())
}

fn set_active_model(db_path: &PathBuf, mode: SetMode) -> Result<(), String> {
    let conn = open_db(db_path)?;
    let model_id = match mode {
        SetMode::ModelId(id) => {
            if !model_exists(&conn, &id)? {
                return Err(format!("Model id not found in DB: {id}"));
            }
            id
        }
        SetMode::Latest => {
            let latest = latest_model_id(&conn)?;
            latest.ok_or_else(|| "No models found in DB".to_string())?
        }
        SetMode::Clear => String::new(),
    };
    let mut config = sempal::sample_sources::config::load_or_default()
        .map_err(|err| err.to_string())?;
    config.model.classifier_model_id = model_id.clone();
    sempal::sample_sources::config::save(&config).map_err(|err| err.to_string())?;
    let label = if model_id.is_empty() { "cleared".to_string() } else { model_id };
    println!("Active model id set to {label}");
    Ok(())
}

fn model_exists(conn: &Connection, model_id: &str) -> Result<bool, String> {
    let exists: Option<String> = conn
        .query_row(
            "SELECT model_id FROM models WHERE model_id = ?1",
            params![model_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|err| err.to_string())?;
    Ok(exists.is_some())
}

fn latest_model_id(conn: &Connection) -> Result<Option<String>, String> {
    let latest: Option<String> = conn
        .query_row(
            "SELECT model_id FROM models ORDER BY created_at DESC, model_id DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|err| err.to_string())?;
    Ok(latest)
}

fn parse_args(args: Vec<String>) -> Result<Command, String> {
    if args.is_empty() {
        return Err(help_text());
    }
    let mut db_path = None;
    let mut limit = 20usize;
    let mut idx = 0usize;
    let command = args.get(idx).map(|s| s.as_str()).unwrap_or("");
    idx += 1;

    match command {
        "list" => {
            while idx < args.len() {
                match args[idx].as_str() {
                    "--db" => {
                        idx += 1;
                        let value = args.get(idx).ok_or_else(|| "--db requires a value".to_string())?;
                        db_path = Some(PathBuf::from(value));
                    }
                    "--limit" => {
                        idx += 1;
                        let value =
                            args.get(idx).ok_or_else(|| "--limit requires a value".to_string())?;
                        limit = value
                            .parse::<usize>()
                            .map_err(|_| format!("Invalid --limit value: {value}"))?;
                    }
                    unknown => return Err(format!("Unknown argument: {unknown}\n\n{}", help_text())),
                }
                idx += 1;
            }
            let db_path = db_path.unwrap_or(default_db_path()?);
            Ok(Command::List { db_path, limit })
        }
        "set" => {
            let mut mode: Option<SetMode> = None;
            while idx < args.len() {
                match args[idx].as_str() {
                    "--db" => {
                        idx += 1;
                        let value = args.get(idx).ok_or_else(|| "--db requires a value".to_string())?;
                        db_path = Some(PathBuf::from(value));
                    }
                    "--model" => {
                        idx += 1;
                        let value =
                            args.get(idx).ok_or_else(|| "--model requires a value".to_string())?;
                        mode = Some(SetMode::ModelId(value.to_string()));
                    }
                    "--latest" => {
                        mode = Some(SetMode::Latest);
                    }
                    "--clear" => {
                        mode = Some(SetMode::Clear);
                    }
                    unknown => return Err(format!("Unknown argument: {unknown}\n\n{}", help_text())),
                }
                idx += 1;
            }
            let db_path = db_path.unwrap_or(default_db_path()?);
            let mode = mode.ok_or_else(|| "--model, --latest, or --clear is required".to_string())?;
            Ok(Command::Set { db_path, mode })
        }
        _ => Err(help_text()),
    }
}

fn default_db_path() -> Result<PathBuf, String> {
    let root = sempal::app_dirs::app_root_dir().map_err(|err| err.to_string())?;
    Ok(root.join(sempal::sample_sources::library::LIBRARY_DB_FILE_NAME))
}

fn open_db(path: &PathBuf) -> Result<Connection, String> {
    let conn = Connection::open(path).map_err(|err| format!("Open DB failed: {err}"))?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
        .map_err(|err| err.to_string())?;
    Ok(conn)
}

fn help_text() -> String {
    [
        "sempal-models",
        "",
        "Usage:",
        "  sempal-models list [--db <library.db>] [--limit <n>]",
        "  sempal-models set --model <id> [--db <library.db>]",
        "  sempal-models set --latest [--db <library.db>]",
        "  sempal-models set --clear [--db <library.db>]",
    ]
    .join("\n")
}
