use super::super::types::{AnalysisProgress, RunningJobInfo};
use super::constants::{ANALYZE_SAMPLE_JOB_TYPE, EMBEDDING_BACKFILL_JOB_TYPE};
use rusqlite::Connection;

pub(in crate::egui_app::controller::analysis_jobs) fn current_progress(
    conn: &Connection,
) -> Result<AnalysisProgress, String> {
    current_progress_for_job_type(conn, ANALYZE_SAMPLE_JOB_TYPE, true)
}

pub(in crate::egui_app::controller::analysis_jobs) fn current_embedding_backfill_progress(
    conn: &Connection,
) -> Result<AnalysisProgress, String> {
    current_progress_for_job_type(conn, EMBEDDING_BACKFILL_JOB_TYPE, false)
}

pub(in crate::egui_app::controller::analysis_jobs) fn current_running_jobs(
    conn: &Connection,
    limit: usize,
) -> Result<Vec<RunningJobInfo>, String> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let mut stmt = conn
        .prepare(
            "SELECT aj.sample_id, aj.running_at
             FROM analysis_jobs aj
             JOIN wav_files wf
               ON wf.path = substr(aj.sample_id, instr(aj.sample_id, '::') + 2)
              AND wf.missing = 0
             WHERE aj.job_type = ?1
               AND aj.status = 'running'
             ORDER BY aj.running_at IS NULL, aj.running_at ASC
             LIMIT ?2",
        )
        .map_err(|err| format!("Failed to query running analysis jobs: {err}"))?;
    let mut rows = stmt
        .query(rusqlite::params![ANALYZE_SAMPLE_JOB_TYPE, limit as i64])
        .map_err(|err| format!("Failed to query running analysis jobs: {err}"))?;
    let mut out = Vec::new();
    while let Some(row) = rows
        .next()
        .map_err(|err| format!("Failed to query running analysis jobs: {err}"))?
    {
        let sample_id: String = row.get(0).map_err(|err| err.to_string())?;
        let running_at: Option<i64> = row.get(1).map_err(|err| err.to_string())?;
        out.push(RunningJobInfo {
            sample_id,
            running_at,
        });
    }
    Ok(out)
}

fn current_progress_for_job_type(
    conn: &Connection,
    job_type: &str,
    filter_missing: bool,
) -> Result<AnalysisProgress, String> {
    let (status_sql, total_sql, pending_sql) = if filter_missing {
        (
            "SELECT aj.status, COUNT(*)
             FROM analysis_jobs aj
             JOIN wav_files wf
               ON wf.path = substr(aj.sample_id, instr(aj.sample_id, '::') + 2)
              AND wf.missing = 0
             WHERE aj.job_type = ?1
             GROUP BY aj.status",
            "SELECT COUNT(DISTINCT aj.sample_id)
             FROM analysis_jobs aj
             JOIN wav_files wf
               ON wf.path = substr(aj.sample_id, instr(aj.sample_id, '::') + 2)
              AND wf.missing = 0
             WHERE aj.job_type = ?1",
            "SELECT COUNT(DISTINCT aj.sample_id)
             FROM analysis_jobs aj
             JOIN wav_files wf
               ON wf.path = substr(aj.sample_id, instr(aj.sample_id, '::') + 2)
              AND wf.missing = 0
             WHERE aj.job_type = ?1
               AND aj.status IN ('pending','running')",
        )
    } else {
        (
            "SELECT status, COUNT(*)
             FROM analysis_jobs
             WHERE job_type = ?1
             GROUP BY status",
            "SELECT COUNT(DISTINCT sample_id)
             FROM analysis_jobs
             WHERE job_type = ?1",
            "SELECT COUNT(DISTINCT sample_id)
             FROM analysis_jobs
             WHERE job_type = ?1
               AND status IN ('pending','running')",
        )
    };
    let mut stmt = conn
        .prepare(status_sql)
        .map_err(|err| format!("Failed to query analysis progress: {err}"))?;
    let mut progress = AnalysisProgress::default();
    let mut rows = stmt
        .query([job_type])
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
        .query_row(total_sql, [job_type], |row| row.get::<_, i64>(0))
        .map_err(|err| format!("Failed to query analysis sample total: {err}"))?
        .max(0) as usize;
    progress.samples_pending_or_running = conn
        .query_row(pending_sql, [job_type], |row| row.get::<_, i64>(0))
        .map_err(|err| format!("Failed to query analysis sample pending/running: {err}"))?
        .max(0) as usize;
    Ok(progress)
}
