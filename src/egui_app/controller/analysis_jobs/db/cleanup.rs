use rusqlite::{Connection, TransactionBehavior};

pub(in crate::egui_app::controller::analysis_jobs) fn reset_running_to_pending(
    conn: &Connection,
) -> Result<usize, String> {
    conn.execute(
        "UPDATE analysis_jobs SET status = 'pending' WHERE status = 'running'",
        [],
    )
    .map_err(|err| format!("Failed to reset running analysis jobs: {err}"))
}

pub(in crate::egui_app::controller::analysis_jobs) fn prune_jobs_for_missing_sources(
    conn: &Connection,
) -> Result<usize, String> {
    conn.execute(
        "DELETE FROM analysis_jobs
         WHERE NOT EXISTS (
            SELECT 1
            FROM sources s
            WHERE analysis_jobs.sample_id LIKE s.id || '::%'
         )",
        [],
    )
    .map_err(|err| format!("Failed to prune analysis jobs for missing sources: {err}"))
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
        let sql = format!(
            "DELETE FROM {table}
             WHERE NOT EXISTS (
                SELECT 1
                FROM sources s
                WHERE {table}.sample_id LIKE s.id || '::%'
             )"
        );
        removed += tx
            .execute(&sql, [])
            .map_err(|err| format!("Failed to purge {table}: {err}"))? as usize;
    }
    tx.commit()
        .map_err(|err| format!("Failed to commit purge transaction: {err}"))?;
    Ok(removed)
}
