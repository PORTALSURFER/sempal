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

pub(crate) use artifacts::{
    invalidate_analysis_artifacts, update_analysis_metadata, upsert_analysis_features,
    upsert_embedding,
};
pub(crate) use cleanup::{prune_jobs_for_missing_sources, reset_running_to_pending};
pub(in crate::egui_app::controller) use cleanup::purge_orphaned_samples;
pub(in crate::egui_app::controller) use connection::open_library_db;
pub(crate) use constants::{
    ANALYZE_SAMPLE_JOB_TYPE, EMBEDDING_BACKFILL_JOB_TYPE, REBUILD_INDEX_JOB_TYPE,
};
#[cfg(test)]
pub(crate) use constants::DEFAULT_JOB_TYPE;
pub(crate) use enqueue::{enqueue_jobs, upsert_samples};
pub(in crate::egui_app::controller) use ids::{build_sample_id, parse_sample_id};
pub(crate) use jobs::{claim_next_job, mark_done, mark_failed, sample_content_hash, source_root_for};
pub(crate) use progress::current_progress;
pub(crate) use types::{ClaimedJob, SampleMetadata};
