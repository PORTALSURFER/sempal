use super::types::ClaimedJob;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::path::PathBuf;

pub(in crate::egui_app::controller::analysis_jobs) fn sample_content_hash(
    conn: &Connection,
    sample_id: &str,
) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT content_hash FROM samples WHERE sample_id = ?1",
        params![sample_id],
        |row| row.get(0),
    )
    .optional()
    .map_err(|err| format!("Failed to lookup sample content hash: {err}"))
}

pub(in crate::egui_app::controller::analysis_jobs) fn claim_next_job(
    conn: &mut Connection,
) -> Result<Option<ClaimedJob>, String> {
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
pub(in crate::egui_app::controller::analysis_jobs) fn mark_done(
    conn: &Connection,
    job_id: i64,
) -> Result<(), String> {
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
pub(in crate::egui_app::controller::analysis_jobs) fn mark_failed(
    conn: &Connection,
    job_id: i64,
    error: &str,
) -> Result<(), String> {
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
pub(in crate::egui_app::controller::analysis_jobs) fn source_root_for(
    conn: &Connection,
    source_id: &str,
) -> Result<Option<PathBuf>, String> {
    conn.query_row(
        "SELECT root FROM sources WHERE id = ?1",
        params![source_id],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map(|root| root.map(PathBuf::from))
    .map_err(|err| format!("Failed to lookup source root for analysis job: {err}"))
}
