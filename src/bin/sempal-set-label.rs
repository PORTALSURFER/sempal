//! CLI utility to set or clear user labels for samples.

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use rusqlite::OptionalExtension;

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
    let mut conn = if let Some(db_path) = &options.db_path {
        rusqlite::Connection::open(db_path)
            .map_err(|err| format!("Open DB failed: {err}"))?
    } else {
        sempal::sample_sources::library::open_connection()
            .map_err(|err| err.to_string())?
    };
    apply_pragmas(&mut conn)?;

    if !options.clear {
        let exists: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM classes WHERE class_id = ?1",
                rusqlite::params![options.class_id.as_deref().unwrap_or_default()],
                |row| row.get(0),
            )
            .optional()
            .map_err(|err| format!("Failed to query classes: {err}"))?;
        if exists.is_none() {
            return Err("class_id not found in classes table".to_string());
        }
    }

    let now = now_epoch_seconds();
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|err| format!("Start labels_user transaction failed: {err}"))?;
    if options.clear {
        let mut stmt = tx
            .prepare("DELETE FROM labels_user WHERE sample_id = ?1")
            .map_err(|err| format!("Prepare labels_user delete failed: {err}"))?;
        for sample_id in &options.sample_ids {
            stmt.execute(rusqlite::params![sample_id])
                .map_err(|err| format!("Delete labels_user failed: {err}"))?;
        }
    } else {
        let class_id = options
            .class_id
            .as_deref()
            .ok_or_else(|| "class_id is required unless --clear is set".to_string())?;
        let mut stmt = tx
            .prepare(
                "INSERT INTO labels_user (sample_id, class_id, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?3)
                 ON CONFLICT(sample_id) DO UPDATE SET
                    class_id = excluded.class_id,
                    updated_at = excluded.updated_at",
            )
            .map_err(|err| format!("Prepare labels_user upsert failed: {err}"))?;
        for sample_id in &options.sample_ids {
            stmt.execute(rusqlite::params![sample_id, class_id, now])
                .map_err(|err| format!("Upsert labels_user failed: {err}"))?;
        }
    }
    tx.commit()
        .map_err(|err| format!("Commit labels_user transaction failed: {err}"))?;

    println!(
        "{} {} sample(s).",
        if options.clear { "Cleared" } else { "Labeled" },
        options.sample_ids.len()
    );
    Ok(())
}

#[derive(Default)]
struct Options {
    db_path: Option<PathBuf>,
    sample_ids: Vec<String>,
    class_id: Option<String>,
    clear: bool,
}

fn parse_args(args: Vec<String>) -> Result<Option<Options>, String> {
    let mut options = Options::default();
    let mut idx = 0usize;
    while idx < args.len() {
        match args[idx].as_str() {
            "-h" | "--help" => {
                println!("{}", help_text());
                return Ok(None);
            }
            "--db" => {
                idx += 1;
                let value = args.get(idx).ok_or_else(|| "--db requires a value".to_string())?;
                options.db_path = Some(PathBuf::from(value));
            }
            "--sample-id" => {
                idx += 1;
                let value =
                    args.get(idx).ok_or_else(|| "--sample-id requires a value".to_string())?;
                options.sample_ids.push(value.to_string());
            }
            "--class-id" => {
                idx += 1;
                let value =
                    args.get(idx).ok_or_else(|| "--class-id requires a value".to_string())?;
                options.class_id = Some(value.to_string());
            }
            "--clear" => {
                options.clear = true;
            }
            unknown => {
                return Err(format!("Unknown argument: {unknown}\n\n{}", help_text()));
            }
        }
        idx += 1;
    }

    if options.sample_ids.is_empty() {
        return Err("--sample-id is required".to_string());
    }
    if options.clear {
        return Ok(Some(options));
    }
    if options.class_id.is_none() {
        return Err("--class-id is required unless --clear is set".to_string());
    }
    Ok(Some(options))
}

fn apply_pragmas(conn: &mut rusqlite::Connection) -> Result<(), String> {
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys=ON;
         PRAGMA busy_timeout=5000;
         PRAGMA temp_store=MEMORY;
         PRAGMA cache_size=-64000;
         PRAGMA mmap_size=268435456;",
    )
    .map_err(|err| format!("Failed to set library DB pragmas: {err}"))?;
    Ok(())
}

fn now_epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs() as i64
}

fn help_text() -> String {
    [
        "sempal-set-label",
        "",
        "Sets or clears a user label for one or more samples.",
        "",
        "Usage:",
        "  sempal-set-label --sample-id <id> --class-id <class>",
        "  sempal-set-label --sample-id <id> --clear",
        "",
        "Options:",
        "  --sample-id <id>   Sample id (repeatable).",
        "  --class-id <id>    Class id from classes_v1.json.",
        "  --clear            Clear user label for the sample(s).",
        "  --db <path>        Path to library.db (defaults to app data location).",
    ]
    .join("\n")
}
