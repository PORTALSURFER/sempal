use crate::egui_app::controller::analysis_jobs::db;
use tracing::warn;

use super::support::{load_embedding_vec_optional, now_epoch_seconds};

pub(super) fn run_analysis_job(
    conn: &rusqlite::Connection,
    job: &db::ClaimedJob,
    use_cache: bool,
    max_analysis_duration_seconds: f32,
    analysis_sample_rate: u32,
    analysis_version: &str,
) -> Result<(), String> {
    let content_hash = job
        .content_hash
        .as_deref()
        .ok_or_else(|| format!("Missing content_hash for analysis job {}", job.sample_id))?;
    let current_hash = db::sample_content_hash(conn, &job.sample_id)?;
    if current_hash.as_deref() != Some(content_hash) {
        return Ok(());
    }
    if use_cache {
        let cached_features = db::cached_features_by_hash(
            conn,
            content_hash,
            analysis_version,
            crate::analysis::vector::FEATURE_VERSION_V1,
        )?;
        let cached_embedding = db::cached_embedding_by_hash(
            conn,
            content_hash,
            analysis_version,
            crate::analysis::embedding::EMBEDDING_MODEL_ID,
        )?;
        if let (Some(features), Some(embedding)) = (&cached_features, &cached_embedding) {
            if let Ok(vec) = crate::analysis::decode_f32_le_blob(&embedding.vec_blob) {
                if vec.len() == crate::analysis::embedding::EMBEDDING_DIM {
                    db::update_analysis_metadata(
                        conn,
                        &job.sample_id,
                        Some(content_hash),
                        features.duration_seconds,
                        features.sr_used,
                        analysis_version,
                    )?;
                    db::upsert_analysis_features(
                        conn,
                        &job.sample_id,
                        &features.vec_blob,
                        features.feat_version,
                        features.computed_at,
                    )?;
                    db::upsert_embedding(
                        conn,
                        &job.sample_id,
                        &embedding.model_id,
                        embedding.dim,
                        &embedding.dtype,
                        embedding.l2_normed,
                        &embedding.vec_blob,
                        embedding.created_at,
                    )?;
                    crate::analysis::ann_index::upsert_embedding(conn, &job.sample_id, &vec)?;
                    return Ok(());
                }
            }
        }
        if let Some(embedding) = cached_embedding {
            db::upsert_embedding(
                conn,
                &job.sample_id,
                &embedding.model_id,
                embedding.dim,
                &embedding.dtype,
                embedding.l2_normed,
                &embedding.vec_blob,
                embedding.created_at,
            )?;
        }
    }

    let (_source_id, relative_path) = db::parse_sample_id(&job.sample_id)?;
    let absolute = job.source_root.join(&relative_path);
    if max_analysis_duration_seconds.is_finite() && max_analysis_duration_seconds > 0.0 {
        if let Ok(probe) = crate::analysis::audio::probe_metadata(&absolute) {
            if let Some(duration_seconds) = probe.duration_seconds {
                if duration_seconds > max_analysis_duration_seconds {
                    let sample_rate = probe
                        .sample_rate
                        .unwrap_or(crate::analysis::audio::ANALYSIS_SAMPLE_RATE);
                    db::update_analysis_metadata(
                        conn,
                        &job.sample_id,
                        job.content_hash.as_deref(),
                        duration_seconds,
                        sample_rate,
                        analysis_version,
                    )?;
                    return Ok(());
                }
            }
        }
    }
    let decode_limit_seconds =
        if max_analysis_duration_seconds.is_finite() && max_analysis_duration_seconds > 0.0 {
            Some(max_analysis_duration_seconds)
        } else {
            None
        };
    let decoded = crate::analysis::audio::decode_for_analysis_with_rate_limit(
        &absolute,
        analysis_sample_rate,
        decode_limit_seconds,
    )?;
    run_analysis_job_with_decoded(conn, job, decoded, use_cache, analysis_version)
}

pub(super) fn run_analysis_job_with_decoded(
    conn: &rusqlite::Connection,
    job: &db::ClaimedJob,
    decoded: crate::analysis::audio::AnalysisAudio,
    use_cache: bool,
    analysis_version: &str,
) -> Result<(), String> {
    let mut needs_embedding_upsert = false;
    let embedding = if use_cache {
        if let Some(cached) = load_embedding_vec_optional(
            conn,
            &job.sample_id,
            crate::analysis::embedding::EMBEDDING_MODEL_ID,
            crate::analysis::embedding::EMBEDDING_DIM,
        )? {
            cached
        } else {
            let processed = crate::analysis::audio::preprocess_mono_for_embedding(
                &decoded.mono,
                decoded.sample_rate_used,
            );
            let mut logmel = vec![0.0_f32; crate::analysis::embedding::PANNS_LOGMEL_LEN];
            let mut logmel_scratch = crate::analysis::embedding::PannsLogMelScratch::default();
            crate::analysis::embedding::build_panns_logmel_into(
                &processed,
                decoded.sample_rate_used,
                &mut logmel,
                &mut logmel_scratch,
            )?;
            let embedding = crate::analysis::embedding::infer_embedding_from_logmel(&logmel)?;
            needs_embedding_upsert = true;
            embedding
        }
    } else {
        let processed = crate::analysis::audio::preprocess_mono_for_embedding(
            &decoded.mono,
            decoded.sample_rate_used,
        );
        let mut logmel = vec![0.0_f32; crate::analysis::embedding::PANNS_LOGMEL_LEN];
        let mut logmel_scratch = crate::analysis::embedding::PannsLogMelScratch::default();
        crate::analysis::embedding::build_panns_logmel_into(
            &processed,
            decoded.sample_rate_used,
            &mut logmel,
            &mut logmel_scratch,
        )?;
        let embedding = crate::analysis::embedding::infer_embedding_from_logmel(&logmel)?;
        needs_embedding_upsert = true;
        embedding
    };
    finalize_analysis_job(
        conn,
        job,
        decoded,
        analysis_version,
        embedding,
        needs_embedding_upsert,
        true,
    )
}

pub(in crate::egui_app::controller::analysis_jobs::pool) fn run_analysis_jobs_with_decoded_batch(
    conn: &rusqlite::Connection,
    jobs: Vec<(db::ClaimedJob, crate::analysis::audio::AnalysisAudio)>,
    use_cache: bool,
    analysis_version: &str,
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
        if use_cache {
            match load_embedding_vec_optional(
                conn,
                &sample_id,
                crate::analysis::embedding::EMBEDDING_MODEL_ID,
                crate::analysis::embedding::EMBEDDING_DIM,
            ) {
                Ok(Some(cached)) => {
                    item.embedding = Some(cached);
                }
                Ok(None) => {
                    let processed = crate::analysis::audio::preprocess_mono_for_embedding(
                        &item.decoded.mono,
                        sample_rate_used,
                    );
                    let mut logmel = vec![0.0_f32; crate::analysis::embedding::PANNS_LOGMEL_LEN];
                    match crate::analysis::embedding::build_panns_logmel_into(
                        &processed,
                        sample_rate_used,
                        &mut logmel,
                        &mut logmel_scratch,
                    ) {
                        Ok(()) => {
                            item.logmel = Some(logmel);
                            item.needs_embedding_upsert = true;
                        }
                        Err(err) => {
                            item.error = Some(err);
                        }
                    }
                }
                Err(err) => {
                    item.error = Some(err);
                }
            }
        } else {
            let processed = crate::analysis::audio::preprocess_mono_for_embedding(
                &item.decoded.mono,
                sample_rate_used,
            );
            let mut logmel = vec![0.0_f32; crate::analysis::embedding::PANNS_LOGMEL_LEN];
            match crate::analysis::embedding::build_panns_logmel_into(
                &processed,
                sample_rate_used,
                &mut logmel,
                &mut logmel_scratch,
            ) {
                Ok(()) => {
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
                            crate::analysis::embedding::infer_embedding_from_logmel(logmel.as_slice())
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
                analysis_version,
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

fn finalize_analysis_job(
    conn: &rusqlite::Connection,
    job: &db::ClaimedJob,
    decoded: crate::analysis::audio::AnalysisAudio,
    analysis_version: &str,
    embedding: Vec<f32>,
    needs_embedding_upsert: bool,
    do_ann_upsert: bool,
) -> Result<(), String> {
    let content_hash = job
        .content_hash
        .as_deref()
        .ok_or_else(|| format!("Missing content_hash for analysis job {}", job.sample_id))?;
    if needs_embedding_upsert {
        let embedding_blob = crate::analysis::vector::encode_f32_le_blob(&embedding);
        let created_at = now_epoch_seconds();
        db::upsert_embedding(
            conn,
            &job.sample_id,
            crate::analysis::embedding::EMBEDDING_MODEL_ID,
            crate::analysis::embedding::EMBEDDING_DIM as i64,
            crate::analysis::embedding::EMBEDDING_DTYPE_F32,
            true,
            &embedding_blob,
            created_at,
        )?;
    }
    let time_domain = crate::analysis::time_domain::extract_time_domain_features(
        &decoded.mono,
        decoded.sample_rate_used,
    );
    let frequency_domain = crate::analysis::frequency_domain::extract_frequency_domain_features(
        &decoded.mono,
        decoded.sample_rate_used,
    );
    let features =
        crate::analysis::features::AnalysisFeaturesV1::new(time_domain, frequency_domain);
    db::update_analysis_metadata(
        conn,
        &job.sample_id,
        job.content_hash.as_deref(),
        decoded.duration_seconds,
        decoded.sample_rate_used,
        analysis_version,
    )?;
    let current_hash = db::sample_content_hash(conn, &job.sample_id)?;
    if current_hash.as_deref() != Some(content_hash) {
        return Ok(());
    }
    if do_ann_upsert {
        crate::analysis::ann_index::upsert_embedding(conn, &job.sample_id, &embedding)?;
    }
    let vector = crate::analysis::vector::to_f32_vector_v1(&features);
    let blob = crate::analysis::vector::encode_f32_le_blob(&vector);
    let computed_at = now_epoch_seconds();
    db::upsert_analysis_features(
        conn,
        &job.sample_id,
        &blob,
        crate::analysis::vector::FEATURE_VERSION_V1,
        computed_at,
    )?;
    let embedding_blob = crate::analysis::vector::encode_f32_le_blob(&embedding);
    db::upsert_cached_features(
        conn,
        content_hash,
        analysis_version,
        crate::analysis::vector::FEATURE_VERSION_V1,
        &blob,
        computed_at,
        decoded.duration_seconds,
        decoded.sample_rate_used,
    )?;
    db::upsert_cached_embedding(
        conn,
        content_hash,
        analysis_version,
        crate::analysis::embedding::EMBEDDING_MODEL_ID,
        crate::analysis::embedding::EMBEDDING_DIM as i64,
        crate::analysis::embedding::EMBEDDING_DTYPE_F32,
        true,
        &embedding_blob,
        now_epoch_seconds(),
    )?;
    Ok(())
}
