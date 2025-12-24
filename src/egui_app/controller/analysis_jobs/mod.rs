//! Background analysis job queue backed by the global library database.

mod db;
mod enqueue;
mod failures;
mod pool;
mod types;

pub(in crate::egui_app::controller) use db::open_source_db;
pub(in crate::egui_app::controller) use db::purge_orphaned_samples;
pub(in crate::egui_app::controller) use db::{build_sample_id, parse_sample_id};
pub(super) use enqueue::enqueue_jobs_for_embedding_backfill;
pub(in crate::egui_app::controller) use enqueue::enqueue_jobs_for_source;
pub(in crate::egui_app::controller) use enqueue::enqueue_jobs_for_source_backfill;
pub(in crate::egui_app::controller) use enqueue::enqueue_jobs_for_source_missing_features;
pub(super) use failures::failed_samples_for_source;
pub(super) use pool::AnalysisWorkerPool;
pub(super) use types::AnalysisJobMessage;
pub(in crate::egui_app::controller) use types::AnalysisProgress;

pub(in crate::egui_app::controller) fn current_progress_for_source(
    source: &crate::sample_sources::SampleSource,
) -> Result<AnalysisProgress, String> {
    let conn = db::open_source_db(&source.root)?;
    db::current_progress(&conn)
}
