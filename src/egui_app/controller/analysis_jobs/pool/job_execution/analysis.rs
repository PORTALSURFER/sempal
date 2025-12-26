use crate::egui_app::controller::analysis_jobs::db;
use tracing::warn;

use super::analysis_cache::{load_existing_embedding, lookup_cache_by_hash};
use super::analysis_db::{apply_cached_embedding, apply_cached_features_and_embedding, finalize_analysis_job, update_metadata_for_skip};
use super::analysis_decode::{build_logmel_for_embedding, decode_for_analysis, infer_embedding_from_logmel, DecodeOutcome};

pub(in crate::egui_app::controller::analysis_jobs::pool) struct AnalysisContext<'a> {
    pub(in crate::egui_app::controller::analysis_jobs::pool) use_cache: bool,
    pub(in crate::egui_app::controller::analysis_jobs::pool) max_analysis_duration_seconds: f32,
    pub(in crate::egui_app::controller::analysis_jobs::pool) analysis_sample_rate: u32,
    pub(in crate::egui_app::controller::analysis_jobs::pool) analysis_version: &'a str,
}

pub(super) fn run_analysis_job(
    conn: &rusqlite::Connection,
    job: &db::ClaimedJob,
    context: &AnalysisContext<'_>,
) -> Result<(), String> {
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

    match decode_for_analysis(job, context)? {
        DecodeOutcome::Decoded(decoded) => run_analysis_job_with_decoded(conn, job, decoded, context),
        DecodeOutcome::Skipped {
            duration_seconds,
            sample_rate,
        } => update_metadata_for_skip(
            conn,
            job,
            duration_seconds,
            sample_rate,
            context.analysis_version,
        ),
    }
}

pub(super) fn run_analysis_job_with_decoded(
    conn: &rusqlite::Connection,
    job: &db::ClaimedJob,
    decoded: crate::analysis::audio::AnalysisAudio,
    context: &AnalysisContext<'_>,
) -> Result<(), String> {
    let mut needs_embedding_upsert = false;
    let embedding = if context.use_cache {
        if let Some(cached) = load_existing_embedding(conn, &job.sample_id)? {
            cached
        } else {
            let embedding = super::analysis_decode::infer_embedding_from_audio(&decoded)?;
            needs_embedding_upsert = true;
            embedding
        }
    } else {
        let embedding = super::analysis_decode::infer_embedding_from_audio(&decoded)?;
        needs_embedding_upsert = true;
        embedding
    };
    finalize_analysis_job(
        conn,
        job,
        decoded,
        context.analysis_version,
        embedding,
        needs_embedding_upsert,
        true,
    )
}

pub(in crate::egui_app::controller::analysis_jobs::pool) fn run_analysis_jobs_with_decoded_batch(
    conn: &rusqlite::Connection,
    jobs: Vec<(db::ClaimedJob, crate::analysis::audio::AnalysisAudio)>,
    context: &AnalysisContext<'_>,
) -> Vec<(db::ClaimedJob, Result<(), String>)> {
    struct BatchJob {
        job: db::ClaimedJob,
        decoded: crate::analysis::audio::AnalysisAudio,
        embedding: Option<Vec<f32>>,
        logmel: Option<Vec<f32>>,
        needs_embedding_upsert: bool,
        error: Option<String>,
    }

    let mut batch_jobs = Vec::with_capacity(jobs.len());
    let mut logmel_scratch = crate::analysis::embedding::PannsLogMelScratch::default();
    for (job, decoded) in jobs {
        let sample_id = job.sample_id.clone();
        let sample_rate_used = decoded.sample_rate_used;
        let mut item = BatchJob {
            job,
            decoded,
            embedding: None,
            logmel: None,
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
                Ok(Some(cached)) => {
                    item.embedding = Some(cached);
                }
                Ok(None) => match build_logmel_for_embedding(
                    &item.decoded.mono,
                    sample_rate_used,
                    &mut logmel_scratch,
                ) {
                    Ok(logmel) => {
                        item.logmel = Some(logmel);
                        item.needs_embedding_upsert = true;
                    }
                    Err(err) => {
                        item.error = Some(err);
                    }
                },
                Err(err) => {
                    item.error = Some(err);
                }
            }
        } else {
            match build_logmel_for_embedding(
                &item.decoded.mono,
                sample_rate_used,
                &mut logmel_scratch,
            ) {
                Ok(logmel) => {
                    item.logmel = Some(logmel);
                    item.needs_embedding_upsert = true;
                }
                Err(err) => {
                    item.error = Some(err);
                }
            }
        }
        batch_jobs.push(item);
    }

    let mut input_indices = Vec::new();
    let mut logmel_inputs = Vec::new();
    for (idx, item) in batch_jobs.iter().enumerate() {
        if let Some(logmel) = item.logmel.as_ref() {
            input_indices.push(idx);
            logmel_inputs.push(logmel.clone());
        }
    }

    if !input_indices.is_empty() {
        let results = if crate::analysis::embedding::embedding_pipeline_enabled() {
            let inflight = crate::analysis::embedding::embedding_inflight_max();
            let micro_batch = crate::analysis::embedding::embedding_batch_max();
            crate::analysis::embedding::infer_embeddings_from_logmel_batch_pipelined(
                &logmel_inputs,
                micro_batch,
                inflight,
            )
        } else {
            let micro_batch = crate::analysis::embedding::embedding_batch_max();
            crate::analysis::embedding::infer_embeddings_from_logmel_batch_chunked(
                &logmel_inputs,
                micro_batch,
            )
        };
        for (idx, result) in input_indices.iter().copied().zip(results.into_iter()) {
            match result {
                Ok(embedding) => {
                    if let Some(item) = batch_jobs.get_mut(idx) {
                        item.embedding = Some(embedding);
                    }
                }
                Err(err) => {
                    let logmel = match batch_jobs.get(idx) {
                        Some(item) => item.logmel.as_ref(),
                        None => None,
                    };
                    let fallback = match logmel {
                        Some(logmel) => std::panic::catch_unwind(|| {
                            infer_embedding_from_logmel(logmel.as_slice())
                        })
                        .unwrap_or_else(|_| Err("PANNs single inference panicked".to_string())),
                        None => Err("Missing log-mel for fallback".to_string()),
                    };
                    if let Some(item) = batch_jobs.get_mut(idx) {
                        match fallback {
                            Ok(embedding) => {
                                item.embedding = Some(embedding);
                            }
                            Err(fallback_err) => {
                                item.error = Some(format!(
                                    "Batch inference failed: {err}; fallback failed: {fallback_err}"
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    let mut ann_batch: Vec<(String, Vec<f32>)> = Vec::new();
    let mut outcomes = Vec::with_capacity(batch_jobs.len());
    for item in batch_jobs {
        let result = if let Some(err) = item.error {
            Err(err)
        } else if let Some(embedding) = item.embedding {
            let sample_id = item.job.sample_id.clone();
            finalize_analysis_job(
                conn,
                &item.job,
                item.decoded,
                context.analysis_version,
                embedding.clone(),
                item.needs_embedding_upsert,
                false,
            )
            .map(|_| {
                ann_batch.push((sample_id, embedding));
            })
        } else {
            Err("Missing embedding for analysis job".to_string())
        };
        outcomes.push((item.job, result));
    }
    if !ann_batch.is_empty() {
        if let Err(err) = crate::analysis::ann_index::upsert_embeddings_batch(
            conn,
            ann_batch
                .iter()
                .map(|(sample_id, embedding)| (sample_id.as_str(), embedding.as_slice())),
        ) {
            warn!("ANN index batch update failed: {err}");
        }
    }
    outcomes
}
