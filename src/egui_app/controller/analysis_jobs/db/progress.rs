use super::super::types::AnalysisProgress;
use rusqlite::Connection;

pub(in crate::egui_app::controller::analysis_jobs) fn current_progress(
    conn: &Connection,
) -> Result<AnalysisProgress, String> {
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

    progress.samples_total = conn
        .query_row("SELECT COUNT(DISTINCT sample_id) FROM analysis_jobs", [], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|err| format!("Failed to query analysis sample total: {err}"))?
        .max(0) as usize;
    progress.samples_pending_or_running = conn
        .query_row(
            "SELECT COUNT(DISTINCT sample_id)
             FROM analysis_jobs
             WHERE status IN ('pending','running')",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|err| format!("Failed to query analysis sample pending/running: {err}"))?
        .max(0) as usize;
    Ok(progress)
}
