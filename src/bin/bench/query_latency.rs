use super::options::BenchOptions;
use super::stats;
use rusqlite::{Connection, params};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub(super) struct QueryBenchResult {
    pub(super) seeded_rows: usize,
    pub(super) latest_model_id: stats::LatencySummary,
    pub(super) predictions_by_prefix: stats::LatencySummary,
    pub(super) labels_user_by_prefix: stats::LatencySummary,
}

pub(super) fn run(options: &BenchOptions) -> Result<QueryBenchResult, String> {
    let mut conn = open_db()?;
    seed_db(&mut conn, options.query_rows)?;
    let (prefix, prefix_end) = sample_prefix_bounds();
    let model_id = fetch_latest_model_id(&conn)?;

    let mut stmt_latest = conn
        .prepare("SELECT model_id FROM models ORDER BY created_at DESC, model_id DESC LIMIT 1")
        .map_err(|err| format!("Prepare latest model query failed: {err}"))?;
    let mut stmt_predictions = conn
        .prepare(
            "SELECT sample_id, top_class, confidence
             FROM predictions
             WHERE model_id = ?1 AND sample_id >= ?2 AND sample_id < ?3",
        )
        .map_err(|err| format!("Prepare predictions query failed: {err}"))?;
    let mut stmt_labels_user = conn
        .prepare(
            "SELECT sample_id, class_id
             FROM labels_user
             WHERE sample_id >= ?1 AND sample_id < ?2",
        )
        .map_err(|err| format!("Prepare user labels query failed: {err}"))?;

    let latest_model_id = stats::bench_sql_query(options, || {
        let _: String = stmt_latest.query_row([], |row| row.get(0))?;
        Ok(())
    })?;

    let predictions_by_prefix = stats::bench_sql_query(options, || {
        let mut rows = stmt_predictions.query(params![model_id, prefix, prefix_end])?;
        while let Some(row) = rows.next()? {
            let _: String = row.get(0)?;
        }
        Ok(())
    })?;

    let labels_user_by_prefix = stats::bench_sql_query(options, || {
        let mut rows = stmt_labels_user.query(params![prefix, prefix_end])?;
        while let Some(row) = rows.next()? {
            let _: String = row.get(0)?;
        }
        Ok(())
    })?;

    Ok(QueryBenchResult {
        seeded_rows: options.query_rows,
        latest_model_id,
        predictions_by_prefix,
        labels_user_by_prefix,
    })
}

fn open_db() -> Result<Connection, String> {
    let conn = Connection::open_in_memory().map_err(|err| format!("Open sqlite failed: {err}"))?;
    conn.execute_batch(
        "PRAGMA journal_mode=OFF;
         PRAGMA synchronous=OFF;
         PRAGMA foreign_keys=ON;
         CREATE TABLE models (
            model_id TEXT PRIMARY KEY,
            classes_json TEXT NOT NULL,
            created_at INTEGER NOT NULL
         ) WITHOUT ROWID;
         CREATE INDEX idx_models_created_at ON models (created_at);
         CREATE TABLE predictions (
            sample_id TEXT NOT NULL,
            model_id TEXT NOT NULL,
            top_class TEXT NOT NULL,
            confidence REAL NOT NULL,
            computed_at INTEGER NOT NULL,
            PRIMARY KEY (sample_id, model_id)
         ) WITHOUT ROWID;
         CREATE INDEX idx_predictions_model_sample_id ON predictions (model_id, sample_id);
         CREATE TABLE labels_user (
            sample_id TEXT PRIMARY KEY,
            class_id TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
         ) WITHOUT ROWID;",
    )
    .map_err(|err| format!("Create schema failed: {err}"))?;
    Ok(conn)
}

fn seed_db(conn: &mut Connection, rows: usize) -> Result<(), String> {
    conn.execute(
        "INSERT INTO models (model_id, classes_json, created_at) VALUES (?1, '[]', 1)",
        ["model-1"],
    )
    .map_err(|err| format!("Insert model failed: {err}"))?;

    let tx = conn
        .transaction()
        .map_err(|err| format!("Start seed transaction failed: {err}"))?;
    {
        let mut pred_stmt = tx
            .prepare(
                "INSERT INTO predictions (sample_id, model_id, top_class, confidence, computed_at)
                 VALUES (?1, ?2, ?3, ?4, 1)",
            )
            .map_err(|err| format!("Prepare seed predictions failed: {err}"))?;
        let mut label_stmt = tx
            .prepare(
                "INSERT INTO labels_user (sample_id, class_id, created_at, updated_at)
                 VALUES (?1, ?2, 1, 1)",
            )
            .map_err(|err| format!("Prepare seed labels failed: {err}"))?;
        for i in 0..rows {
            let sample_id = format!("source-a::{i:08}.wav");
            pred_stmt
                .execute(params![sample_id, "model-1", "kick", 0.9_f64])
                .map_err(|err| format!("Seed predictions failed: {err}"))?;
            if i % 10 == 0 {
                label_stmt
                    .execute(params![sample_id, "snare"])
                    .map_err(|err| format!("Seed labels failed: {err}"))?;
            }
        }
    }
    tx.commit()
        .map_err(|err| format!("Commit seed transaction failed: {err}"))?;
    Ok(())
}

fn sample_prefix_bounds() -> (&'static str, String) {
    let prefix = "source-a::";
    let prefix_end = format!("{prefix}\u{10FFFF}");
    (prefix, prefix_end)
}

fn fetch_latest_model_id(conn: &Connection) -> Result<String, String> {
    conn.query_row(
        "SELECT model_id FROM models ORDER BY created_at DESC, model_id DESC LIMIT 1",
        [],
        |row| row.get(0),
    )
    .map_err(|err| format!("Fetch model id failed: {err}"))
}
