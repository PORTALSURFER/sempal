use crate::egui_app::controller::library::analysis_jobs::db;
use crate::egui_app::controller::library::analysis_jobs::types::AnalysisProgress;

pub(crate) fn write_changed_samples(
    conn: &mut rusqlite::Connection,
    sample_metadata: &[db::SampleMetadata],
    invalidate: &[String],
    jobs: &[(String, String)],
    source_id: &str,
    created_at: i64,
) -> Result<(usize, AnalysisProgress), String> {
    db::upsert_samples(conn, sample_metadata)?;
    if !invalidate.is_empty() {
        db::invalidate_analysis_artifacts(conn, invalidate)?;
    }
    let inserted = db::enqueue_jobs(
        conn,
        jobs,
        db::ANALYZE_SAMPLE_JOB_TYPE,
        created_at,
        source_id,
    )?;
    let progress = db::current_progress(conn)?;
    Ok((inserted, progress))
}

pub(crate) fn write_backfill_samples(
    conn: &mut rusqlite::Connection,
    sample_metadata: &[db::SampleMetadata],
    invalidate: &[String],
    jobs: &[(String, String)],
    job_type: &str,
    source_id: &str,
    created_at: i64,
) -> Result<(usize, AnalysisProgress), String> {
    if !invalidate.is_empty() {
        db::invalidate_analysis_artifacts(conn, invalidate)?;
    }
    db::upsert_samples(conn, sample_metadata)?;
    let inserted = db::enqueue_jobs(conn, jobs, job_type, created_at, source_id)?;
    let progress = db::current_progress(conn)?;
    Ok((inserted, progress))
}

pub(crate) fn stage_backfill_samples(
    conn: &mut rusqlite::Connection,
    samples: &[db::SampleMetadata],
) -> Result<(), String> {
    prepare_backfill_staging(conn)?;
    const BATCH_SIZE: usize = 400;
    for chunk in samples.chunks(BATCH_SIZE) {
        let mut sql = String::from(
            "INSERT INTO temp_backfill_samples (sample_id, content_hash, size, mtime_ns) VALUES ",
        );
        let mut params: Vec<rusqlite::types::Value> = Vec::with_capacity(chunk.len() * 4);
        for (idx, sample) in chunk.iter().enumerate() {
            if idx > 0 {
                sql.push_str(", ");
            }
            let size = i64::try_from(sample.size)
                .map_err(|_| "Sample size exceeds storage limits".to_string())?;
            let base = idx * 4;
            sql.push_str(&format!(
                "(?{}, ?{}, ?{}, ?{})",
                base + 1,
                base + 2,
                base + 3,
                base + 4
            ));
            params.push(rusqlite::types::Value::from(sample.sample_id.clone()));
            params.push(rusqlite::types::Value::from(sample.content_hash.clone()));
            params.push(rusqlite::types::Value::from(size));
            params.push(rusqlite::types::Value::from(sample.mtime_ns));
        }
        conn.execute(&sql, rusqlite::params_from_iter(params))
            .map_err(|err| format!("Insert backfill staging rows failed: {err}"))?;
    }
    Ok(())
}

fn prepare_backfill_staging(conn: &mut rusqlite::Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS temp_backfill_samples (
            sample_id TEXT PRIMARY KEY,
            content_hash TEXT NOT NULL,
            size INTEGER NOT NULL,
            mtime_ns INTEGER NOT NULL
        );
        DELETE FROM temp_backfill_samples;",
    )
    .map_err(|err| format!("Prepare backfill staging table failed: {err}"))?;
    Ok(())
}
