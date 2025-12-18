use super::types::AnalysisProgress;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::path::{Path, PathBuf};

pub(super) const DEFAULT_JOB_TYPE: &str = "wav_metadata_v1";
pub(super) const INFERENCE_JOB_TYPE: &str = "inference_v1";

#[derive(Clone, Debug)]
pub(super) struct ClaimedJob {
    pub(super) id: i64,
    pub(super) sample_id: String,
    pub(super) content_hash: Option<String>,
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

pub(super) fn sample_content_hash(conn: &Connection, sample_id: &str) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT content_hash FROM samples WHERE sample_id = ?1",
        params![sample_id],
        |row| row.get(0),
    )
    .optional()
    .map_err(|err| format!("Failed to lookup sample content hash: {err}"))
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
    jobs: &[(String, String)],
    job_type: &str,
    created_at: i64,
) -> Result<usize, String> {
    if jobs.is_empty() {
        return Ok(0);
    }
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|err| format!("Failed to start analysis enqueue transaction: {err}"))?;
    let inserted = enqueue_jobs_tx(&tx, jobs, job_type, created_at)?;
    tx.commit()
        .map_err(|err| format!("Failed to commit analysis enqueue transaction: {err}"))?;
    Ok(inserted)
}

fn enqueue_jobs_tx(
    tx: &rusqlite::Transaction<'_>,
    jobs: &[(String, String)],
    job_type: &str,
    created_at: i64,
) -> Result<usize, String> {
    let mut stmt = tx
        .prepare(
            "INSERT INTO analysis_jobs (sample_id, job_type, content_hash, status, attempts, created_at)
             VALUES (?1, ?2, ?3, 'pending', 0, ?4)
             ON CONFLICT(sample_id, job_type) DO UPDATE SET
                content_hash = excluded.content_hash,
                status = 'pending',
                attempts = 0,
                created_at = excluded.created_at,
                last_error = NULL",
        )
        .map_err(|err| format!("Failed to prepare analysis enqueue statement: {err}"))?;
    let mut inserted = 0usize;
    for (sample_id, content_hash) in jobs {
        let changed = stmt
            .execute(params![sample_id, job_type, content_hash, created_at])
            .map_err(|err| format!("Failed to enqueue analysis job: {err}"))?;
        inserted += changed;
    }
    Ok(inserted)
}

pub(super) fn upsert_samples(
    conn: &mut Connection,
    samples: &[SampleMetadata],
) -> Result<usize, String> {
    if samples.is_empty() {
        return Ok(0);
    }
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|err| format!("Failed to start samples upsert transaction: {err}"))?;
    let changed = upsert_samples_tx(&tx, samples)?;
    tx.commit()
        .map_err(|err| format!("Failed to commit samples upsert transaction: {err}"))?;
    Ok(changed)
}

fn upsert_samples_tx(
    tx: &rusqlite::Transaction<'_>,
    samples: &[SampleMetadata],
) -> Result<usize, String> {
    let mut stmt = tx
        .prepare(
            "INSERT INTO samples (sample_id, content_hash, size, mtime_ns, duration_seconds, sr_used)
             VALUES (?1, ?2, ?3, ?4, NULL, NULL)
             ON CONFLICT(sample_id) DO UPDATE SET
                content_hash = excluded.content_hash,
                size = excluded.size,
                mtime_ns = excluded.mtime_ns,
                duration_seconds = NULL,
                sr_used = NULL",
        )
        .map_err(|err| format!("Failed to prepare samples upsert statement: {err}"))?;
    let mut changed = 0usize;
    for sample in samples {
        changed += stmt
            .execute(params![
                &sample.sample_id,
                &sample.content_hash,
                sample.size as i64,
                sample.mtime_ns
            ])
            .map_err(|err| format!("Failed to upsert sample metadata: {err}"))?;
    }
    Ok(changed)
}

pub(super) fn invalidate_analysis_artifacts(
    conn: &mut Connection,
    sample_ids: &[String],
) -> Result<(), String> {
    if sample_ids.is_empty() {
        return Ok(());
    }
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|err| format!("Failed to start analysis invalidation transaction: {err}"))?;
    let mut stmt_features = tx
        .prepare("DELETE FROM features WHERE sample_id = ?1")
        .map_err(|err| format!("Failed to prepare analysis invalidation statement: {err}"))?;
    let mut stmt_legacy_features = tx
        .prepare("DELETE FROM analysis_features WHERE sample_id = ?1")
        .map_err(|err| format!("Failed to prepare analysis invalidation statement: {err}"))?;
    let mut stmt_predictions = tx
        .prepare("DELETE FROM analysis_predictions WHERE sample_id = ?1")
        .map_err(|err| format!("Failed to prepare analysis invalidation statement: {err}"))?;
    let mut stmt_model_predictions = tx
        .prepare("DELETE FROM predictions WHERE sample_id = ?1")
        .map_err(|err| format!("Failed to prepare analysis invalidation statement: {err}"))?;
    for sample_id in sample_ids {
        stmt_features
            .execute(params![sample_id])
            .map_err(|err| format!("Failed to invalidate analysis features: {err}"))?;
        stmt_legacy_features
            .execute(params![sample_id])
            .map_err(|err| format!("Failed to invalidate analysis features: {err}"))?;
        stmt_predictions
            .execute(params![sample_id])
            .map_err(|err| format!("Failed to invalidate analysis predictions: {err}"))?;
        stmt_model_predictions
            .execute(params![sample_id])
            .map_err(|err| format!("Failed to invalidate predictions: {err}"))?;
    }
    drop(stmt_features);
    drop(stmt_legacy_features);
    drop(stmt_predictions);
    drop(stmt_model_predictions);
    tx.commit()
        .map_err(|err| format!("Failed to commit analysis invalidation transaction: {err}"))?;
    Ok(())
}

#[derive(Clone, Debug)]
pub(super) struct SampleMetadata {
    pub(super) sample_id: String,
    pub(super) content_hash: String,
    pub(super) size: u64,
    pub(super) mtime_ns: i64,
}

pub(super) fn claim_next_job(conn: &mut Connection) -> Result<Option<ClaimedJob>, String> {
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|err| format!("Failed to start analysis claim transaction: {err}"))?;
    let job = tx
        .query_row(
            "SELECT id, sample_id, content_hash, job_type
             FROM analysis_jobs
             WHERE status = 'pending'
             ORDER BY created_at ASC, id ASC
             LIMIT 1",
            [],
            |row| {
                Ok(ClaimedJob {
                    id: row.get(0)?,
                    sample_id: row.get(1)?,
                    content_hash: row.get(2)?,
                    job_type: row.get(3)?,
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

pub(super) fn update_analysis_metadata(
    conn: &Connection,
    sample_id: &str,
    content_hash: Option<&str>,
    duration_seconds: f32,
    sr_used: u32,
) -> Result<(), String> {
    let updated = conn
        .execute(
            "UPDATE samples
             SET duration_seconds = ?3, sr_used = ?4
             WHERE sample_id = ?1 AND content_hash = COALESCE(?2, content_hash)",
            params![
                sample_id,
                content_hash,
                duration_seconds as f64,
                sr_used as i64
            ],
        )
        .map_err(|err| format!("Failed to update analysis metadata: {err}"))?;
    if updated == 0 {
        return Err(format!("No sample row updated for sample_id={sample_id}"));
    }
    Ok(())
}

pub(super) fn upsert_analysis_features(
    conn: &Connection,
    sample_id: &str,
    vec_blob: &[u8],
    feat_version: i64,
    computed_at: i64,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO features (sample_id, feat_version, vec_blob, computed_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(sample_id) DO UPDATE SET
            feat_version = excluded.feat_version,
            vec_blob = excluded.vec_blob,
            computed_at = excluded.computed_at",
        params![sample_id, feat_version, vec_blob, computed_at],
    )
    .map_err(|err| format!("Failed to upsert analysis features: {err}"))?;
    Ok(())
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
                content_hash TEXT,
                status TEXT NOT NULL,
                attempts INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                last_error TEXT,
                UNIQUE(sample_id, job_type)
            );
            CREATE TABLE samples (
                sample_id TEXT PRIMARY KEY,
                content_hash TEXT NOT NULL,
                size INTEGER NOT NULL,
                mtime_ns INTEGER NOT NULL,
                duration_seconds REAL,
                sr_used INTEGER
            );
            CREATE TABLE features (
                sample_id TEXT PRIMARY KEY,
                feat_version INTEGER NOT NULL,
                vec_blob BLOB NOT NULL,
                computed_at INTEGER NOT NULL
            ) WITHOUT ROWID;
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
        let jobs = vec![
            ("s::a.wav".to_string(), "h1".to_string()),
            ("s::a.wav".to_string(), "h1".to_string()),
        ];
        let inserted = enqueue_jobs(&mut conn, &jobs, DEFAULT_JOB_TYPE, 123).unwrap();
        assert_eq!(inserted, 2);
        let progress = current_progress(&conn).unwrap();
        assert_eq!(progress.pending, 1);
        assert_eq!(progress.total(), 1);
    }

    #[test]
    fn claim_next_job_marks_running_and_increments_attempts() {
        let mut conn = conn_with_schema();
        let jobs = vec![("s::a.wav".to_string(), "h1".to_string())];
        enqueue_jobs(&mut conn, &jobs, DEFAULT_JOB_TYPE, 123).unwrap();
        let job = claim_next_job(&mut conn).unwrap().expect("job claimed");
        assert_eq!(job.sample_id, "s::a.wav");
        assert_eq!(job.content_hash.as_deref(), Some("h1"));
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

    #[test]
    fn update_analysis_metadata_updates_matching_hash() {
        let conn = conn_with_schema();
        conn.execute(
            "INSERT INTO samples (sample_id, content_hash, size, mtime_ns)
             VALUES ('s::a.wav', 'h1', 10, 5)",
            [],
        )
        .unwrap();
        update_analysis_metadata(&conn, "s::a.wav", Some("h1"), 1.25, 22_050).unwrap();
        let (duration, sr): (Option<f64>, Option<i64>) = conn
            .query_row(
                "SELECT duration_seconds, sr_used FROM samples WHERE sample_id = 's::a.wav'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(duration, Some(1.25));
        assert_eq!(sr, Some(22_050));
    }

    #[test]
    fn upsert_analysis_features_overwrites_existing() {
        let conn = conn_with_schema();
        upsert_analysis_features(&conn, "s::a.wav", b"one", 1, 100).unwrap();
        upsert_analysis_features(&conn, "s::a.wav", b"two", 1, 200).unwrap();
        let (version, blob, computed_at): (i64, Vec<u8>, i64) = conn
            .query_row(
                "SELECT feat_version, vec_blob, computed_at FROM features WHERE sample_id = 's::a.wav'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(version, 1);
        assert_eq!(blob, b"two");
        assert_eq!(computed_at, 200);
    }
}
