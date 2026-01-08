use crate::egui_app::controller::library::analysis_jobs::db;

use super::analysis_cache::{load_existing_embedding, lookup_cache_by_hash};
use super::analysis_db::{
    apply_cached_embedding, apply_cached_features_and_embedding, finalize_analysis_job,
    update_metadata_for_skip,
};
use super::analysis_decode::{DecodeOutcome, decode_for_analysis};
use super::support::JobHeartbeat;

pub(crate) struct AnalysisContext<'a> {
    pub(crate) use_cache: bool,
    pub(crate) max_analysis_duration_seconds: f32,
    pub(crate) analysis_sample_rate: u32,
    pub(crate) analysis_version: &'a str,
}

pub(crate) fn run_analysis_job(
    conn: &rusqlite::Connection,
    job: &db::ClaimedJob,
    context: &AnalysisContext<'_>,
) -> Result<(), String> {
    let mut heartbeat = JobHeartbeat::new(std::time::Duration::from_secs(4));
    let job_ids = [job.id];
    heartbeat.touch_jobs(conn, &job_ids)?;
    let content_hash = job
        .content_hash
        .as_deref()
        .ok_or_else(|| format!("Missing content_hash for analysis job {}", job.sample_id))?;
    let current_hash = db::sample_content_hash(conn, &job.sample_id)?;
    if current_hash.as_deref() != Some(content_hash) {
        return Ok(());
    }
    if context.use_cache {
        let cache = lookup_cache_by_hash(conn, content_hash, context.analysis_version)?;
        if let (Some(features), Some(embedding), Some(embedding_vec)) =
            (&cache.features, &cache.embedding, &cache.embedding_vec)
        {
            apply_cached_features_and_embedding(
                conn,
                job,
                content_hash,
                features,
                embedding,
                embedding_vec,
                context.analysis_version,
            )?;
            return Ok(());
        }
        if let Some(embedding) = cache.embedding.as_ref() {
            apply_cached_embedding(conn, job, embedding)?;
        }
    }

    heartbeat.touch_jobs(conn, &job_ids)?;
    match decode_for_analysis(job, context)? {
        DecodeOutcome::Decoded(decoded) => {
            heartbeat.touch_jobs(conn, &job_ids)?;
            run_analysis_job_with_decoded(conn, job, decoded, context)
        }
        DecodeOutcome::Skipped {
            duration_seconds,
            sample_rate,
        } => {
            heartbeat.touch_jobs(conn, &job_ids)?;
            update_metadata_for_skip(
                conn,
                job,
                duration_seconds,
                sample_rate,
                context.analysis_version,
            )
        }
    }
}

pub(crate) fn run_analysis_job_with_decoded(
    conn: &rusqlite::Connection,
    job: &db::ClaimedJob,
    decoded: crate::analysis::audio::AnalysisAudio,
    context: &AnalysisContext<'_>,
) -> Result<(), String> {
    let needs_embedding_upsert = if context.use_cache {
        load_existing_embedding(conn, &job.sample_id)?.is_none()
    } else {
        true
    };
    finalize_analysis_job(
        conn,
        job,
        decoded,
        context.analysis_version,
        needs_embedding_upsert,
        true,
    )
}

pub(crate) fn run_analysis_jobs_with_decoded_batch(
    conn: &rusqlite::Connection,
    jobs: Vec<(db::ClaimedJob, crate::analysis::audio::AnalysisAudio)>,
    context: &AnalysisContext<'_>,
) -> Vec<(db::ClaimedJob, Result<(), String>)> {
    struct BatchJob {
        job: db::ClaimedJob,
        decoded: crate::analysis::audio::AnalysisAudio,
        needs_embedding_upsert: bool,
        error: Option<String>,
    }

    let job_ids: Vec<i64> = jobs.iter().map(|(job, _)| job.id).collect();
    let mut heartbeat = JobHeartbeat::new(std::time::Duration::from_secs(4));
    let mut batch_jobs = Vec::with_capacity(jobs.len());
    let _ = heartbeat.touch_jobs(conn, &job_ids);
    for (job, decoded) in jobs {
        let sample_id = job.sample_id.clone();
        let mut item = BatchJob {
            job,
            decoded,
            needs_embedding_upsert: false,
            error: None,
        };
        if item.job.content_hash.as_deref().is_none() {
            item.error = Some(format!(
                "Missing content_hash for analysis job {}",
                sample_id
            ));
            batch_jobs.push(item);
            continue;
        }
        if context.use_cache {
            match load_existing_embedding(conn, &sample_id) {
                Ok(Some(_cached)) => {
                    item.needs_embedding_upsert = false;
                }
                Ok(None) => {
                    item.needs_embedding_upsert = true;
                }
                Err(err) => {
                    item.error = Some(err);
                }
            }
        } else {
            item.needs_embedding_upsert = true;
        }
        batch_jobs.push(item);
    }
    let _ = heartbeat.touch_jobs(conn, &job_ids);
    let mut outcomes = Vec::with_capacity(batch_jobs.len());
    for item in batch_jobs {
        let _ = heartbeat.touch_jobs(conn, &job_ids);
        let result = if let Some(err) = item.error {
            Err(err)
        } else {
            finalize_analysis_job(
                conn,
                &item.job,
                item.decoded,
                context.analysis_version,
                item.needs_embedding_upsert,
                true,
            )
        };
        outcomes.push((item.job, result));
    }
    outcomes
}
