use rusqlite::{Connection, TransactionBehavior, params};

pub(in crate::egui_app::controller::analysis_jobs) fn reset_running_to_pending(
    conn: &Connection,
) -> Result<usize, String> {
    conn.execute(
        "UPDATE analysis_jobs
         SET status = 'pending', running_at = NULL
         WHERE status = 'running'",
        [],
    )
    .map_err(|err| format!("Failed to reset running analysis jobs: {err}"))
}

pub(in crate::egui_app::controller::analysis_jobs) fn reset_stale_running_jobs(
    conn: &Connection,
    stale_before_epoch: i64,
) -> Result<usize, String> {
    conn.execute(
        "UPDATE analysis_jobs
         SET status = 'pending', running_at = NULL
         WHERE status = 'running'
           AND running_at IS NOT NULL
           AND running_at <= ?1",
        rusqlite::params![stale_before_epoch],
    )
    .map_err(|err| format!("Failed to reset stale analysis jobs: {err}"))
}

pub(in crate::egui_app::controller::analysis_jobs) fn fail_stale_running_jobs(
    conn: &Connection,
    stale_before_epoch: i64,
) -> Result<usize, String> {
    conn.execute(
        "UPDATE analysis_jobs
         SET status = 'failed',
             last_error = 'Timed out while running',
             running_at = NULL
         WHERE status = 'running'
           AND running_at IS NOT NULL
           AND running_at <= ?1",
        rusqlite::params![stale_before_epoch],
    )
    .map_err(|err| format!("Failed to fail stale analysis jobs: {err}"))
}

pub(in crate::egui_app::controller::analysis_jobs) fn prune_jobs_for_missing_sources(
    conn: &Connection,
) -> Result<usize, String> {
    conn.execute(
        "DELETE FROM analysis_jobs
         WHERE job_type = ?1
           AND NOT EXISTS (
            SELECT 1
            FROM wav_files wf
            WHERE wf.path = substr(analysis_jobs.sample_id, instr(analysis_jobs.sample_id, '::') + 2)
         )",
        params![super::ANALYZE_SAMPLE_JOB_TYPE],
    )
    .map_err(|err| format!("Failed to prune analysis jobs for missing files: {err}"))
}

pub(in crate::egui_app::controller) fn purge_orphaned_samples(
    conn: &mut Connection,
) -> Result<usize, String> {
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|err| format!("Failed to start purge transaction: {err}"))?;
    let mut removed = 0usize;
    for table in [
        "analysis_jobs",
        "analysis_features",
        "features",
        "embeddings",
        "samples",
    ] {
        let (sql, params) = if table == "analysis_jobs" {
            (
                "DELETE FROM analysis_jobs
                 WHERE job_type = ?1
                   AND NOT EXISTS (
                      SELECT 1
                      FROM wav_files wf
                      WHERE wf.path = substr(analysis_jobs.sample_id, instr(analysis_jobs.sample_id, '::') + 2)
                   )"
                    .to_string(),
                params![super::ANALYZE_SAMPLE_JOB_TYPE],
            )
        } else {
            (
                format!(
                    "DELETE FROM {table}
                     WHERE NOT EXISTS (
                        SELECT 1
                        FROM wav_files wf
                        WHERE wf.path = substr({table}.sample_id, instr({table}.sample_id, '::') + 2)
                     )"
                ),
                params![],
            )
        };
        removed += tx
            .execute(&sql, params)
            .map_err(|err| format!("Failed to purge {table}: {err}"))? as usize;
    }
    tx.commit()
        .map_err(|err| format!("Failed to commit purge transaction: {err}"))?;
    Ok(removed)
}
