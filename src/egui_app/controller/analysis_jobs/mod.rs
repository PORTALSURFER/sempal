//! Background analysis job queue backed by the global library database.

mod db;
mod enqueue;
mod failures;
mod inference;
mod pool;
mod types;
mod weak_labels;

pub(in crate::egui_app::controller) use db::open_library_db;
pub(in crate::egui_app::controller) use enqueue::enqueue_inference_jobs_for_sources;
pub(super) use enqueue::enqueue_jobs_for_source;
pub(super) use enqueue::enqueue_jobs_for_source_backfill;
pub(super) use enqueue::enqueue_jobs_for_source_missing_features;
pub(super) use failures::failed_samples_for_source;
pub(super) use pool::AnalysisWorkerPool;
pub(super) use types::AnalysisJobMessage;
