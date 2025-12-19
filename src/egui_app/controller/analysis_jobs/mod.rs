//! Background analysis job queue backed by the global library database.

mod db;
mod enqueue;
mod failures;
mod inference;
mod pool;
mod relabel;
mod types;
mod weak_labels;

pub(in crate::egui_app::controller) use db::open_library_db;
pub(in crate::egui_app::controller) use db::purge_orphaned_samples;
pub(in crate::egui_app::controller) use db::{build_sample_id, parse_sample_id};
pub(in crate::egui_app::controller) use enqueue::{
    enqueue_inference_jobs_for_all_features, enqueue_inference_jobs_for_sources,
};
pub(super) use enqueue::enqueue_jobs_for_source;
pub(super) use enqueue::enqueue_jobs_for_source_backfill;
pub(super) use enqueue::enqueue_jobs_for_source_missing_features;
pub(super) use failures::failed_samples_for_source;
pub(super) use pool::AnalysisWorkerPool;
pub(in crate::egui_app::controller) use relabel::recompute_weak_labels_for_source;
pub(in crate::egui_app::controller) use relabel::recompute_weak_labels_for_sources;
pub(super) use types::AnalysisJobMessage;
