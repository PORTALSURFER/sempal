use rusqlite::{Connection, params};

use super::constants::REBUILD_INDEX_JOB_TYPE;

const ANN_INDEX_DIRTY_KEY: &str = "ann_index_dirty_v1";
const ANN_INDEX_REBUILD_PATH: &str = "ann_index_rebuild";

#[derive(serde::Serialize)]
struct AnnIndexDirtyEntry<'a> {
    dirty_at: i64,
    reason: &'a str,
}

/// Mark the ANN index as dirty and record a short failure reason.
pub(crate) fn mark_ann_index_dirty(
    conn: &Connection,
    reason: &str,
) -> Result<(), String> {
    let payload = serde_json::to_string(&AnnIndexDirtyEntry {
        dirty_at: now_epoch_seconds(),
        reason,
    })
    .map_err(|err| format!("Failed to encode ANN dirty marker: {err}"))?;
    conn.execute(
        "INSERT INTO metadata (key, value)
         VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![ANN_INDEX_DIRTY_KEY, payload],
    )
    .map_err(|err| format!("Failed to mark ANN index dirty: {err}"))?;
    Ok(())
}

/// Clear the ANN dirty marker after a successful rebuild.
pub(crate) fn clear_ann_index_dirty(conn: &Connection) -> Result<(), String> {
    conn.execute(
        "DELETE FROM metadata WHERE key = ?1",
        params![ANN_INDEX_DIRTY_KEY],
    )
    .map_err(|err| format!("Failed to clear ANN index dirty marker: {err}"))?;
    Ok(())
}

/// Ensure a rebuild job exists for the requested source.
pub(crate) fn enqueue_rebuild_ann_index_job(
    conn: &Connection,
    source_id: &str,
    created_at: i64,
) -> Result<usize, String> {
    let existing: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM analysis_jobs
             WHERE job_type = ?1 AND source_id = ?2 AND status IN ('pending','running')",
            params![REBUILD_INDEX_JOB_TYPE, source_id],
            |row| row.get(0),
        )
        .map_err(|err| format!("Failed to query ANN rebuild jobs: {err}"))?;
    if existing > 0 {
        return Ok(0);
    }
    let sample_id = format!("{source_id}::{ANN_INDEX_REBUILD_PATH}");
    let inserted = conn
        .execute(
            "INSERT INTO analysis_jobs (
                 sample_id, source_id, relative_path, job_type, content_hash,
                 status, attempts, created_at
             )
             VALUES (?1, ?2, ?3, ?4, NULL, 'pending', 0, ?5)
             ON CONFLICT(sample_id, job_type) DO UPDATE SET
                 source_id = excluded.source_id,
                 relative_path = excluded.relative_path,
                 status = 'pending',
                 attempts = 0,
                 created_at = excluded.created_at,
                 running_at = NULL,
                 last_error = NULL",
            params![
                sample_id,
                source_id,
                ANN_INDEX_REBUILD_PATH,
                REBUILD_INDEX_JOB_TYPE,
                created_at
            ],
        )
        .map_err(|err| format!("Failed to enqueue ANN rebuild job: {err}"))?;
    Ok(inserted as usize)
}

fn now_epoch_seconds() -> i64 {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs() as i64
}
