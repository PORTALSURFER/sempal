use super::types::ClaimedJob;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params, params_from_iter};
use std::collections::HashMap;
use std::path::PathBuf;

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
