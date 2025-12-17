//! Background analysis job queue backed by the global library database.

mod db;
mod pool;
mod types;
mod weak_labels;

pub(super) use pool::AnalysisWorkerPool;
pub(super) use pool::enqueue_jobs_for_source;
pub(super) use types::AnalysisJobMessage;
