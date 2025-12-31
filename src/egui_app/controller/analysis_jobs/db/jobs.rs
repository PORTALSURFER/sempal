use super::types::ClaimedJob;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params, params_from_iter};
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Cached analysis state for a sample row.
pub(in crate::egui_app::controller::analysis_jobs) struct SampleAnalysisState {
    pub(in crate::egui_app::controller::analysis_jobs) content_hash: String,
    pub(in crate::egui_app::controller::analysis_jobs) analysis_version: Option<String>,
}

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

/// Load content hashes and analysis versions for the requested sample ids.
pub(in crate::egui_app::controller::analysis_jobs) fn sample_analysis_states(
    conn: &Connection,
    sample_ids: &[String],
) -> Result<HashMap<String, SampleAnalysisState>, String> {
    if sample_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let placeholders = std::iter::repeat("?")
        .take(sample_ids.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT sample_id, content_hash, analysis_version
         FROM samples
         WHERE sample_id IN ({placeholders})"
    );
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|err| format!("Failed to prepare sample analysis lookup: {err}"))?;
    let mut rows = stmt
        .query(params_from_iter(sample_ids.iter()))
        .map_err(|err| format!("Failed to query sample analysis metadata: {err}"))?;
    let mut states = HashMap::new();
    while let Some(row) = rows
        .next()
        .map_err(|err| format!("Failed to query sample analysis metadata: {err}"))?
    {
        let sample_id: String = row.get(0).map_err(|err| err.to_string())?;
        let content_hash: String = row.get(1).map_err(|err| err.to_string())?;
        let analysis_version: Option<String> = row.get(2).map_err(|err| err.to_string())?;
        states.insert(
            sample_id,
            SampleAnalysisState {
                content_hash,
                analysis_version,
            },
        );
    }
    Ok(states)
}

#[cfg(test)]
pub(in crate::egui_app::controller::analysis_jobs) fn claim_next_job(
    conn: &mut Connection,
    source_root: &Path,
    source_id: &str,
) -> Result<Option<ClaimedJob>, String> {
    let mut jobs = claim_next_jobs(conn, source_root, source_id, 1)?;
    Ok(jobs.pop())
}

pub(in crate::egui_app::controller::analysis_jobs) fn claim_next_jobs(
    conn: &mut Connection,
    source_root: &Path,
    source_id: &str,
    limit: usize,
) -> Result<Vec<ClaimedJob>, String> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|err| format!("Failed to start analysis claim transaction: {err}"))?;
    let running_at = now_epoch_seconds();
    let mut jobs = Vec::new();
    {
        let mut stmt = tx
            .prepare(
                "WITH ranked AS (
                     SELECT
                         pending.id,
                         pending.sample_id,
                         pending.content_hash,
                         pending.job_type,
                         pending.created_at,
                         ROW_NUMBER() OVER (
                             PARTITION BY pending.sample_id, pending.job_type
                             ORDER BY pending.created_at ASC, pending.id ASC
                         ) AS rn
                     FROM analysis_jobs AS pending
                     WHERE pending.status = 'pending'
                       AND pending.sample_id LIKE ?1
                       AND NOT EXISTS (
                           SELECT 1
                           FROM analysis_jobs AS running
                           WHERE running.sample_id = pending.sample_id
                             AND running.job_type = pending.job_type
                             AND running.status = 'running'
                       )
                 ),
                 to_claim AS (
                     SELECT id
                     FROM ranked
                     WHERE rn = 1
                     ORDER BY created_at ASC, id ASC
                     LIMIT ?2
                 )
                 UPDATE analysis_jobs
                 SET status = 'running', attempts = attempts + 1, running_at = ?3
                 WHERE id IN (SELECT id FROM to_claim)
                 RETURNING id, sample_id, content_hash, job_type",
            )
            .map_err(|err| format!("Failed to prepare analysis job claim: {err}"))?;
        let mut rows = stmt
            .query(params![format!("{source_id}::%"), limit as i64, running_at])
            .map_err(|err| format!("Failed to query analysis jobs: {err}"))?;
        while let Some(row) = rows
            .next()
            .map_err(|err| format!("Failed to query analysis jobs: {err}"))?
        {
            let id: i64 = row.get(0).map_err(|err| err.to_string())?;
            let sample_id: String = row.get(1).map_err(|err| err.to_string())?;
            let content_hash: Option<String> = row.get(2).map_err(|err| err.to_string())?;
            let job_type: String = row.get(3).map_err(|err| err.to_string())?;
            jobs.push(ClaimedJob {
                id,
                sample_id,
                content_hash,
                job_type,
                source_root: source_root.to_path_buf(),
            });
        }
    }
    if jobs.is_empty() {
        tx.commit()
            .map_err(|err| format!("Failed to commit empty analysis claim transaction: {err}"))?;
        return Ok(Vec::new());
    }
    tx.commit()
        .map_err(|err| format!("Failed to commit analysis claim transaction: {err}"))?;
    Ok(jobs)
}

#[cfg_attr(test, allow(dead_code))]
pub(in crate::egui_app::controller::analysis_jobs) fn mark_done(
    conn: &Connection,
    job_id: i64,
) -> Result<(), String> {
    conn.execute(
        "UPDATE analysis_jobs
         SET status = 'done', last_error = NULL, running_at = NULL
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
         SET status = 'failed', last_error = ?2, running_at = NULL
         WHERE id = ?1",
        params![job_id, error],
    )
    .map_err(|err| format!("Failed to mark analysis job failed: {err}"))?;
    Ok(())
}

pub(in crate::egui_app::controller::analysis_jobs) fn mark_pending(
    conn: &Connection,
    job_id: i64,
) -> Result<(), String> {
    conn.execute(
        "UPDATE analysis_jobs
         SET status = 'pending', running_at = NULL
         WHERE id = ?1",
        params![job_id],
    )
    .map_err(|err| format!("Failed to mark analysis job pending: {err}"))?;
    Ok(())
}

fn now_epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs() as i64
}
