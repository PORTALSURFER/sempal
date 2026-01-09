//! Background analysis job queue backed by the global library database.

mod db;
mod enqueue;
mod failures;
mod pool;
mod types;

pub(crate) use db::open_source_db;
pub(crate) use db::purge_orphaned_samples;
pub(crate) use db::{build_sample_id, parse_sample_id, sample_bpm, update_sample_bpm};
pub(crate) use enqueue::enqueue_jobs_for_source;
pub(crate) use enqueue::enqueue_jobs_for_source_backfill;
pub(crate) use enqueue::enqueue_jobs_for_source_backfill_full;
pub(crate) use enqueue::enqueue_jobs_for_source_missing_features;
pub(crate) use enqueue::{enqueue_jobs_for_embedding_backfill, enqueue_jobs_for_embedding_samples};
pub(crate) use failures::failed_samples_for_source;
pub(crate) use pool::AnalysisWorkerPool;
pub(crate) use types::AnalysisJobMessage;
pub(crate) use types::AnalysisProgress;

pub(crate) fn current_progress_for_source(
    source: &crate::sample_sources::SampleSource,
) -> Result<AnalysisProgress, String> {
    let conn = db::open_source_db(&source.root)?;
    db::current_progress(&conn)
}

pub(crate) fn current_embedding_backfill_progress_for_source(
    source: &crate::sample_sources::SampleSource,
) -> Result<AnalysisProgress, String> {
    let conn = db::open_source_db(&source.root)?;
    db::current_embedding_backfill_progress(&conn)
}

pub(crate) fn current_running_jobs_for_source(
    source: &crate::sample_sources::SampleSource,
    limit: usize,
) -> Result<Vec<types::RunningJobInfo>, String> {
    let conn = db::open_source_db(&source.root)?;
    db::current_running_jobs(&conn, limit)
}

pub(crate) fn default_worker_count() -> u32 {
    pool::default_worker_count().max(1) as u32
}

pub(crate) fn stale_running_job_seconds() -> i64 {
    if let Ok(value) = std::env::var("SEMPAL_ANALYSIS_STALE_SECS") {
        if let Ok(parsed) = value.trim().parse::<i64>() {
            if parsed >= 60 {
                return parsed;
            }
        }
    }
    2 * 60
}
