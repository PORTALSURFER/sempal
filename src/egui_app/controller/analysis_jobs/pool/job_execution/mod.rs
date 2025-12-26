use crate::egui_app::controller::analysis_jobs::db;

mod analysis;
mod backfill;
mod errors;
mod status;
mod support;

pub(super) use analysis::run_analysis_jobs_with_decoded_batch;
pub(super) use status::update_job_status_with_retry;

pub(super) fn run_job(
    conn: &rusqlite::Connection,
    job: &db::ClaimedJob,
    use_cache: bool,
    max_analysis_duration_seconds: f32,
    analysis_sample_rate: u32,
    analysis_version: &str,
) -> Result<(), String> {
    match job.job_type.as_str() {
        db::ANALYZE_SAMPLE_JOB_TYPE => analysis::run_analysis_job(
            conn,
            job,
            use_cache,
            max_analysis_duration_seconds,
            analysis_sample_rate,
            analysis_version,
        ),
        db::EMBEDDING_BACKFILL_JOB_TYPE => backfill::run_embedding_backfill_job(
            conn,
            job,
            use_cache,
            analysis_sample_rate,
            analysis_version,
        ),
        db::REBUILD_INDEX_JOB_TYPE => Err("Rebuild index job not implemented yet".to_string()),
        _ => Err(format!("Unknown job type: {}", job.job_type)),
    }
}
