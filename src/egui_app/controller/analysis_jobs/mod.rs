//! Background analysis job queue backed by the global library database.

mod db;
mod enqueue;
mod failures;
mod inference;
mod pool;
mod types;
mod weak_labels;

pub(super) use pool::AnalysisWorkerPool;
pub(in crate::egui_app::controller) use db::open_library_db;
pub(super) use enqueue::enqueue_jobs_for_source;
pub(super) use enqueue::enqueue_jobs_for_source_backfill;
pub(in crate::egui_app::controller) use enqueue::enqueue_inference_jobs_for_all_sources;
pub(super) use failures::failed_samples_for_source;
pub(super) use types::AnalysisJobMessage;
