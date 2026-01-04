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
    cached_embedding_by_hash, cached_features_by_hash, invalidate_analysis_artifacts,
    update_analysis_metadata, upsert_analysis_features, upsert_cached_embedding,
    upsert_cached_features, upsert_embedding, CachedEmbedding, CachedFeatures,
};
pub(in crate::egui_app::controller) use cleanup::purge_orphaned_samples;
pub(in crate::egui_app::controller::analysis_jobs) use cleanup::{
    fail_stale_running_jobs, fail_stale_running_jobs_with_sources, prune_jobs_for_missing_sources,
    reset_running_to_pending,
};
pub(in crate::egui_app::controller) use connection::open_source_db;
#[cfg(test)]
pub(in crate::egui_app::controller::analysis_jobs) use constants::DEFAULT_JOB_TYPE;
pub(in crate::egui_app::controller::analysis_jobs) use constants::{
    ANALYZE_SAMPLE_JOB_TYPE, EMBEDDING_BACKFILL_JOB_TYPE, REBUILD_INDEX_JOB_TYPE,
};
pub(in crate::egui_app::controller::analysis_jobs) use enqueue::{enqueue_jobs, upsert_samples};
pub(in crate::egui_app::controller) use ids::{build_sample_id, parse_sample_id};
#[cfg(test)]
pub(in crate::egui_app::controller::analysis_jobs) use jobs::claim_next_job;
pub(in crate::egui_app::controller::analysis_jobs) use jobs::{
    claim_next_jobs, mark_done, mark_failed, mark_failed_with_reason, mark_pending,
    sample_analysis_states, sample_content_hash, touch_running_at, SampleAnalysisState,
};
pub(in crate::egui_app::controller::analysis_jobs) use progress::{
    current_embedding_backfill_progress, current_progress, current_running_jobs,
};
pub(in crate::egui_app::controller::analysis_jobs) use types::{ClaimedJob, SampleMetadata};
