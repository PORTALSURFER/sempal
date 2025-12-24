mod artifacts;
mod cleanup;
mod connection;
mod constants;
mod enqueue;
mod ids;
mod jobs;
mod progress;
mod types;

#[cfg(test)]
mod tests;

pub(in crate::egui_app::controller::analysis_jobs) use artifacts::{
    invalidate_analysis_artifacts, update_analysis_metadata, upsert_analysis_features,
    upsert_embedding,
};
pub(in crate::egui_app::controller) use cleanup::purge_orphaned_samples;
pub(in crate::egui_app::controller::analysis_jobs) use cleanup::{
    prune_jobs_for_missing_sources, reset_running_to_pending,
};
pub(in crate::egui_app::controller) use connection::open_library_db;
#[cfg(test)]
pub(in crate::egui_app::controller::analysis_jobs) use constants::DEFAULT_JOB_TYPE;
pub(in crate::egui_app::controller::analysis_jobs) use constants::{
    ANALYZE_SAMPLE_JOB_TYPE, EMBEDDING_BACKFILL_JOB_TYPE, REBUILD_INDEX_JOB_TYPE,
};
pub(in crate::egui_app::controller::analysis_jobs) use enqueue::{enqueue_jobs, upsert_samples};
pub(in crate::egui_app::controller) use ids::{build_sample_id, parse_sample_id};
pub(in crate::egui_app::controller::analysis_jobs) use jobs::{
    claim_next_job, mark_done, mark_failed, sample_content_hash, source_root_for,
};
pub(in crate::egui_app::controller::analysis_jobs) use progress::current_progress;
pub(in crate::egui_app::controller::analysis_jobs) use types::{ClaimedJob, SampleMetadata};
