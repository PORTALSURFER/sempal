use super::types::AnalysisProgress;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::path::{Path, PathBuf};

pub(super) const DEFAULT_JOB_TYPE: &str = "wav_metadata_v1";

#[derive(Clone, Debug)]
pub(super) struct ClaimedJob {
    pub(super) id: i64,
    pub(super) sample_id: String,
    pub(super) job_type: String,
}

pub(super) fn open_library_db(db_path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(db_path).map_err(|err| format!("Open library DB failed: {err}"))?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys=ON;",
    )
    .map_err(|err| format!("Failed to set library DB pragmas: {err}"))?;
    Ok(conn)
}

pub(super) fn reset_running_to_pending(conn: &Connection) -> Result<usize, String> {
    conn.execute(
        "UPDATE analysis_jobs SET status = 'pending' WHERE status = 'running'",
        [],
    )
    .map_err(|err| format!("Failed to reset running analysis jobs: {err}"))
}

pub(super) fn current_progress(conn: &Connection) -> Result<AnalysisProgress, String> {
    let mut stmt = conn
        .prepare("SELECT status, COUNT(*) FROM analysis_jobs GROUP BY status")
        .map_err(|err| format!("Failed to query analysis progress: {err}"))?;
    let mut progress = AnalysisProgress::default();
    let mut rows = stmt
        .query([])
        .map_err(|err| format!("Failed to query analysis progress: {err}"))?;
    while let Some(row) = rows
        .next()
        .map_err(|err| format!("Failed to query analysis progress: {err}"))?
    {
        let status: String = row.get(0).map_err(|err| err.to_string())?;
        let count: i64 = row.get(1).map_err(|err| err.to_string())?;
        let count = count.max(0) as usize;
        match status.as_str() {
            "pending" => progress.pending = count,
            "running" => progress.running = count,
            "done" => progress.done = count,
            "failed" => progress.failed = count,
            _ => {}
        }
    }
    Ok(progress)
}

pub(super) fn enqueue_jobs(
    conn: &mut Connection,
    sample_ids: &[String],
    job_type: &str,
    created_at: i64,
) -> Result<usize, String> {
    if sample_ids.is_empty() {
        return Ok(0);
    }
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|err| format!("Failed to start analysis enqueue transaction: {err}"))?;
    let inserted = enqueue_jobs_tx(&tx, sample_ids, job_type, created_at)?;
    tx.commit()
        .map_err(|err| format!("Failed to commit analysis enqueue transaction: {err}"))?;
    Ok(inserted)
}

fn enqueue_jobs_tx(
    tx: &rusqlite::Transaction<'_>,
    sample_ids: &[String],
    job_type: &str,
    created_at: i64,
) -> Result<usize, String> {
    let mut stmt = tx
        .prepare(
            "INSERT INTO analysis_jobs (sample_id, job_type, status, attempts, created_at)
             VALUES (?1, ?2, 'pending', 0, ?3)
             ON CONFLICT(sample_id, job_type) DO NOTHING",
        )
        .map_err(|err| format!("Failed to prepare analysis enqueue statement: {err}"))?;
    let mut inserted = 0usize;
    for sample_id in sample_ids {
        let changed = stmt
            .execute(params![sample_id, job_type, created_at])
            .map_err(|err| format!("Failed to enqueue analysis job: {err}"))?;
        inserted += changed;
    }
    Ok(inserted)
}

pub(super) fn claim_next_job(conn: &mut Connection) -> Result<Option<ClaimedJob>, String> {
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|err| format!("Failed to start analysis claim transaction: {err}"))?;
    let job = tx
        .query_row(
            "SELECT id, sample_id, job_type
             FROM analysis_jobs
             WHERE status = 'pending'
             ORDER BY created_at ASC, id ASC
             LIMIT 1",
            [],
            |row| {
                Ok(ClaimedJob {
                    id: row.get(0)?,
                    sample_id: row.get(1)?,
                    job_type: row.get(2)?,
                })
            },
        )
        .optional()
        .map_err(|err| format!("Failed to query next analysis job: {err}"))?;
    let Some(job) = job else {
        tx.commit()
            .map_err(|err| format!("Failed to commit empty analysis claim transaction: {err}"))?;
        return Ok(None);
    };

    let updated = tx
        .execute(
            "UPDATE analysis_jobs
             SET status = 'running', attempts = attempts + 1
             WHERE id = ?1 AND status = 'pending'",
            params![job.id],
        )
        .map_err(|err| format!("Failed to claim analysis job: {err}"))?;
    if updated == 0 {
        tx.commit()
            .map_err(|err| format!("Failed to commit analysis claim transaction: {err}"))?;
        return Ok(None);
    }
    tx.commit()
        .map_err(|err| format!("Failed to commit analysis claim transaction: {err}"))?;
    Ok(Some(job))
}

#[cfg_attr(test, allow(dead_code))]
pub(super) fn mark_done(conn: &Connection, job_id: i64) -> Result<(), String> {
    conn.execute(
        "UPDATE analysis_jobs
         SET status = 'done', last_error = NULL
         WHERE id = ?1",
        params![job_id],
    )
    .map_err(|err| format!("Failed to mark analysis job done: {err}"))?;
    Ok(())
}

#[cfg_attr(test, allow(dead_code))]
pub(super) fn mark_failed(conn: &Connection, job_id: i64, error: &str) -> Result<(), String> {
    conn.execute(
        "UPDATE analysis_jobs
         SET status = 'failed', last_error = ?2
         WHERE id = ?1",
        params![job_id, error],
    )
    .map_err(|err| format!("Failed to mark analysis job failed: {err}"))?;
    Ok(())
}

#[cfg_attr(test, allow(dead_code))]
pub(super) fn source_root_for(conn: &Connection, source_id: &str) -> Result<Option<PathBuf>, String> {
    conn.query_row(
        "SELECT root FROM sources WHERE id = ?1",
        params![source_id],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map(|root| root.map(PathBuf::from))
    .map_err(|err| format!("Failed to lookup source root for analysis job: {err}"))
}

#[cfg_attr(test, allow(dead_code))]
pub(super) fn parse_sample_id(sample_id: &str) -> Result<(String, PathBuf), String> {
    let (source, path) = sample_id
        .split_once("::")
        .ok_or_else(|| format!("Invalid sample_id: {sample_id}"))?;
    if source.is_empty() {
        return Err(format!("Invalid sample_id: {sample_id}"));
    }
    if path.is_empty() {
        return Err(format!("Invalid sample_id: {sample_id}"));
    }
    Ok((source.to_string(), PathBuf::from(path)))
}

pub(super) fn build_sample_id(source_id: &str, relative_path: &Path) -> String {
    format!("{}::{}", source_id, relative_path.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn conn_with_schema() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE analysis_jobs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                sample_id TEXT NOT NULL,
                job_type TEXT NOT NULL,
                status TEXT NOT NULL,
                attempts INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                last_error TEXT,
                UNIQUE(sample_id, job_type)
            );
            CREATE TABLE sources (
                id TEXT PRIMARY KEY,
                root TEXT NOT NULL,
                sort_order INTEGER NOT NULL
            );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn enqueue_jobs_dedupes_by_sample_and_type() {
        let mut conn = conn_with_schema();
        let sample_ids = vec!["s::a.wav".to_string(), "s::a.wav".to_string()];
        let inserted = enqueue_jobs(&mut conn, &sample_ids, DEFAULT_JOB_TYPE, 123).unwrap();
        assert_eq!(inserted, 1);
        let progress = current_progress(&conn).unwrap();
        assert_eq!(progress.pending, 1);
        assert_eq!(progress.total(), 1);
    }

    #[test]
    fn claim_next_job_marks_running_and_increments_attempts() {
        let mut conn = conn_with_schema();
        let sample_ids = vec!["s::a.wav".to_string()];
        enqueue_jobs(&mut conn, &sample_ids, DEFAULT_JOB_TYPE, 123).unwrap();
        let job = claim_next_job(&mut conn).unwrap().expect("job claimed");
        assert_eq!(job.sample_id, "s::a.wav");
        assert_eq!(job.job_type, DEFAULT_JOB_TYPE);
        let (status, attempts): (String, i64) = conn
            .query_row(
                "SELECT status, attempts FROM analysis_jobs WHERE id = ?1",
                params![job.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "running");
        assert_eq!(attempts, 1);
    }

    #[test]
    fn reset_running_to_pending_updates_rows() {
        let conn = conn_with_schema();
        conn.execute(
            "INSERT INTO analysis_jobs (sample_id, job_type, status, attempts, created_at)
             VALUES ('s::a.wav', 'x', 'running', 1, 0)",
            [],
        )
        .unwrap();
        let changed = reset_running_to_pending(&conn).unwrap();
        assert_eq!(changed, 1);
        let status: String = conn
            .query_row(
                "SELECT status FROM analysis_jobs WHERE sample_id = 's::a.wav'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "pending");
    }
}
