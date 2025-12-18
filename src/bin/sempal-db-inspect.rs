//! Developer utility to inspect a library database and explain row counts.

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
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
    println!("DB: {}", options.db_path.display());
    if let Ok(meta) = std::fs::metadata(&options.db_path) {
        println!("Size: {} bytes", meta.len());
    }

    let uri = format!("file:{}?immutable=1", options.db_path.display());
    let conn = Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|err| err.to_string())?;
    let _ = conn.execute_batch("PRAGMA temp_store=MEMORY;");

    print_count(&conn, "sources")?;
    print_count(&conn, "samples")?;
    print_count(&conn, "features")?;
    print_count_where(&conn, "features(feat_version=1)", "features", "feat_version = 1")?;
    print_count(&conn, "labels_weak")?;
    print_count_where(&conn, "labels_weak(conf>=0.85)", "labels_weak", "confidence >= 0.85")?;
    print_count(&conn, "labels_user")?;
    print_count(&conn, "predictions")?;
    print_count(&conn, "analysis_jobs")?;
    print_count_where(
        &conn,
        "analysis_jobs(pending/running)",
        "analysis_jobs",
        "status IN ('pending','running')",
    )?;

    println!();
    println!("Samples by source_id prefix (top 50):");
    let mut stmt = conn
        .prepare(
            "SELECT
                CASE
                    WHEN instr(sample_id, '::') > 0 THEN substr(sample_id, 1, instr(sample_id, '::') - 1)
                    ELSE '<no_prefix>'
                END AS source_id,
                COUNT(*) AS n
             FROM samples
             GROUP BY source_id
             ORDER BY n DESC
             LIMIT 50",
        )
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))
        .map_err(|err| err.to_string())?;
    for row in rows {
        let (source_id, n) = row.map_err(|err| err.to_string())?;
        println!("- {source_id}: {n}");
    }

    println!();
    println!("Active model:");
    let active: Option<(String, i64)> = conn
        .query_row(
            "SELECT model_id, created_at FROM models ORDER BY created_at DESC, model_id DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|err| err.to_string())?;
    match active {
        Some((model_id, created_at)) => println!("- model_id={model_id} created_at={created_at}"),
        None => println!("- (none)"),
    }

    Ok(())
}

#[derive(Debug, Clone)]
struct CliOptions {
    db_path: PathBuf,
}

fn parse_args(args: Vec<String>) -> Result<Option<CliOptions>, String> {
    let mut db_path: Option<PathBuf> = None;
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
                db_path = Some(PathBuf::from(value));
            }
            unknown => return Err(format!("Unknown argument: {unknown}\n\n{}", help_text())),
        }
        idx += 1;
    }

    let Some(db_path) = db_path else {
        return Err("--db is required".to_string());
    };
    Ok(Some(CliOptions { db_path }))
}

fn help_text() -> String {
    ["sempal-db-inspect", "", "Usage:", "  sempal-db-inspect --db <path-to-library.db>"]
        .join("\n")
}

fn print_count(conn: &Connection, table: &str) -> Result<(), String> {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    let n: i64 = conn.query_row(&sql, [], |row| row.get(0)).map_err(|err| {
        format!(
            "Query failed ({table} count). Make sure the DB isn't in WAL mode or copy it locally first.\n\n{err}"
        )
    })?;
    println!("{table}: {n}");
    Ok(())
}

fn print_count_where(
    conn: &Connection,
    label: &str,
    table: &str,
    where_sql: &str,
) -> Result<(), String> {
    let sql = format!("SELECT COUNT(*) FROM {table} WHERE {where_sql}");
    let n: i64 = conn
        .query_row(&sql, params![], |row| row.get(0))
        .map_err(|err| format!("Query failed ({label}). {err}"))?;
    println!("{label}: {n}");
    Ok(())
}
