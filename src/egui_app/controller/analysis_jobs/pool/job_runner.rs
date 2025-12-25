use crate::egui_app::controller::analysis_jobs::db;
use rusqlite::OptionalExtension;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc::channel};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::warn;

pub(super) fn run_job(
    conn: &rusqlite::Connection,
    job: &db::ClaimedJob,
    use_cache: bool,
    max_analysis_duration_seconds: f32,
    analysis_sample_rate: u32,
    analysis_version: &str,
) -> Result<(), String> {
    match job.job_type.as_str() {
        db::ANALYZE_SAMPLE_JOB_TYPE => run_analysis_job(
            conn,
            job,
            use_cache,
            max_analysis_duration_seconds,
            analysis_sample_rate,
            analysis_version,
        ),
        db::EMBEDDING_BACKFILL_JOB_TYPE => {
            run_embedding_backfill_job(conn, job, use_cache, analysis_sample_rate, analysis_version)
        }
        db::REBUILD_INDEX_JOB_TYPE => Err("Rebuild index job not implemented yet".to_string()),
        _ => Err(format!("Unknown job type: {}", job.job_type)),
    }
}

struct EmbeddingWork {
    sample_id: String,
    absolute_path: PathBuf,
    content_hash: String,
}

struct EmbeddingResult {
    sample_id: String,
    content_hash: String,
    embedding: Vec<f32>,
}

fn run_embedding_backfill_job(
    conn: &rusqlite::Connection,
    job: &db::ClaimedJob,
    use_cache: bool,
    analysis_sample_rate: u32,
    analysis_version: &str,
) -> Result<(), String> {
    let payload = job
        .content_hash
        .as_deref()
        .ok_or_else(|| "Embedding backfill payload missing".to_string())?;
    let sample_ids: Vec<String> = serde_json::from_str(payload)
        .map_err(|err| format!("Invalid embedding backfill payload: {err}"))?;
    if sample_ids.is_empty() {
        return Ok(());
    }

    let mut items = Vec::new();
    for sample_id in sample_ids {
        if load_embedding_vec_optional(
            conn,
            &sample_id,
            crate::analysis::embedding::EMBEDDING_MODEL_ID,
            crate::analysis::embedding::EMBEDDING_DIM,
        )?
        .is_some()
        {
            continue;
        }
        let Some(content_hash) = db::sample_content_hash(conn, &sample_id)? else {
            continue;
        };
        if use_cache {
            if let Some(cached) = db::cached_embedding_by_hash(
                conn,
                &content_hash,
                analysis_version,
                crate::analysis::embedding::EMBEDDING_MODEL_ID,
            )? {
                if let Ok(vec) = crate::analysis::decode_f32_le_blob(&cached.vec_blob) {
                    if vec.len() == crate::analysis::embedding::EMBEDDING_DIM {
                        db::upsert_embedding(
                            conn,
                            &sample_id,
                            &cached.model_id,
                            cached.dim,
                            &cached.dtype,
                            cached.l2_normed,
                            &cached.vec_blob,
                            cached.created_at,
                        )?;
                        crate::analysis::ann_index::upsert_embedding(conn, &sample_id, &vec)?;
                        continue;
                    }
                }
            }
        }
        let (_source_id, relative_path) = match db::parse_sample_id(&sample_id) {
            Ok(parsed) => parsed,
            Err(err) => {
                warn!("Skipping embed backfill sample_id={sample_id}: {err}");
                continue;
            }
        };
        let absolute_path = job.source_root.join(&relative_path);
        if !absolute_path.exists() {
            warn!(
                "Missing file for embed backfill: {}",
                absolute_path.display()
            );
            continue;
        }
        items.push(EmbeddingWork {
            sample_id,
            absolute_path,
            content_hash,
        });
    }

    if items.is_empty() {
        return Ok(());
    }

    let worker_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(items.len())
        .max(1);
    let queue = Arc::new(Mutex::new(VecDeque::from(items)));
    let (tx, rx) = channel();

    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            let queue = Arc::clone(&queue);
            let tx = tx.clone();
            scope.spawn(move || {
                let embedding_batch_max = crate::analysis::embedding::embedding_batch_max();
                loop {
                    let mut batch = Vec::with_capacity(embedding_batch_max);
                    let work = {
                        let mut guard = match queue.lock() {
                            Ok(guard) => guard,
                            Err(_) => return,
                        };
                        guard.pop_front()
                    };
                    let Some(work) = work else {
                        break;
                    };
                    batch.push(work);
                    loop {
                        if batch.len() >= embedding_batch_max {
                            break;
                        }
                        let next = {
                            let mut guard = match queue.lock() {
                                Ok(guard) => guard,
                                Err(_) => return,
                            };
                            guard.pop_front()
                        };
                        let Some(next) = next else {
                            break;
                        };
                        batch.push(next);
                    }

                    let mut payloads = Vec::new();
                    let mut logmels = Vec::new();
                    let mut logmel_scratch =
                        crate::analysis::embedding::PannsLogMelScratch::default();
                    for work in batch {
                        let decoded =
                            match crate::analysis::audio::decode_for_analysis_with_rate(
                                &work.absolute_path,
                                analysis_sample_rate,
                            ) {
                                Ok(decoded) => decoded,
                                Err(err) => {
                                    let _ = tx.send(Err(format!(
                                        "Decode failed for {}: {err}",
                                        work.absolute_path.display()
                                    )));
                                    continue;
                                }
                        };
                        let processed = crate::analysis::audio::preprocess_mono_for_embedding(
                            &decoded.mono,
                            decoded.sample_rate_used,
                        );
                        let mut logmel =
                            vec![0.0_f32; crate::analysis::embedding::PANNS_LOGMEL_LEN];
                        if let Err(err) = crate::analysis::embedding::build_panns_logmel_into(
                            &processed,
                            decoded.sample_rate_used,
                            &mut logmel,
                            &mut logmel_scratch,
                        ) {
                            let _ = tx.send(Err(format!(
                                "Log-mel failed for {}: {err}",
                                work.absolute_path.display()
                            )));
                            continue;
                        }
                        logmels.push(logmel);
                        payloads.push((work.sample_id, work.content_hash));
                    }

                    if logmels.is_empty() {
                        continue;
                    }

                    let mut batch_input = Vec::with_capacity(
                        logmels.len() * crate::analysis::embedding::PANNS_LOGMEL_LEN,
                    );
                    for logmel in &logmels {
                        batch_input.extend_from_slice(logmel);
                    }
                    match crate::analysis::embedding::infer_embeddings_from_logmel_batch(
                        batch_input.as_slice(),
                        logmels.len(),
                    ) {
                        Ok(embeddings) => {
                            for ((sample_id, content_hash), embedding) in
                                payloads.into_iter().zip(embeddings.into_iter())
                            {
                                let _ = tx.send(Ok(EmbeddingResult {
                                    sample_id,
                                    content_hash,
                                    embedding,
                                }));
                            }
                        }
                        Err(err) => {
                            for (sample_id, _) in payloads {
                                let _ =
                                    tx.send(Err(format!("Embed failed for {}: {err}", sample_id)));
                            }
                        }
                    }
                }
            });
        }
        drop(tx);
    });

    let mut results = Vec::new();
    let mut errors = Vec::new();
    while let Ok(result) = rx.recv() {
        match result {
            Ok(result) => results.push(result),
            Err(err) => {
                if errors.len() < 3 {
                    errors.push(err);
                }
            }
        }
    }

    if results.is_empty() {
        if !errors.is_empty() {
            return Err(format!("Embedding backfill failed: {:?}", errors));
        }
        return Ok(());
    }

    const INSERT_BATCH: usize = 128;
    for chunk in results.chunks(INSERT_BATCH) {
        let created_at = now_epoch_seconds();
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(|err| format!("Begin embedding backfill tx failed: {err}"))?;
        for result in chunk {
            let embedding_blob = crate::analysis::vector::encode_f32_le_blob(&result.embedding);
            let insert = conn.execute(
                "INSERT INTO embeddings (sample_id, model_id, dim, dtype, l2_normed, vec, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(sample_id) DO UPDATE SET
                    model_id = excluded.model_id,
                    dim = excluded.dim,
                    dtype = excluded.dtype,
                    l2_normed = excluded.l2_normed,
                    vec = excluded.vec,
                    created_at = excluded.created_at",
                rusqlite::params![
                    result.sample_id,
                    crate::analysis::embedding::EMBEDDING_MODEL_ID,
                    crate::analysis::embedding::EMBEDDING_DIM as i64,
                    crate::analysis::embedding::EMBEDDING_DTYPE_F32,
                    true,
                    &embedding_blob,
                    created_at
                ],
            );
            if let Err(err) = insert {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(format!("Embedding backfill insert failed: {err}"));
            }
            db::upsert_cached_embedding(
                conn,
                &result.content_hash,
                analysis_version,
                crate::analysis::embedding::EMBEDDING_MODEL_ID,
                crate::analysis::embedding::EMBEDDING_DIM as i64,
                crate::analysis::embedding::EMBEDDING_DTYPE_F32,
                true,
                &embedding_blob,
                created_at,
            )?;
        }
        conn.execute_batch("COMMIT")
            .map_err(|err| format!("Commit embedding backfill tx failed: {err}"))?;
        if let Err(err) = crate::analysis::ann_index::upsert_embeddings_batch(
            conn,
            chunk.iter().map(|result| (result.sample_id.as_str(), result.embedding.as_slice())),
        ) {
            warn!("ANN index batch update failed: {err}");
        }
    }

    if !errors.is_empty() {
        warn!("Embedding backfill had errors: {:?}", errors);
    }

    Ok(())
}

fn run_analysis_job(
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
            let mut logmel =
                vec![0.0_f32; crate::analysis::embedding::PANNS_LOGMEL_LEN];
            let mut logmel_scratch =
                crate::analysis::embedding::PannsLogMelScratch::default();
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
        let mut logmel =
            vec![0.0_f32; crate::analysis::embedding::PANNS_LOGMEL_LEN];
        let mut logmel_scratch =
            crate::analysis::embedding::PannsLogMelScratch::default();
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

pub(super) fn run_analysis_jobs_with_decoded_batch(
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
                    let mut logmel =
                        vec![0.0_f32; crate::analysis::embedding::PANNS_LOGMEL_LEN];
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

    let mut batch_input = Vec::new();
    let mut input_indices = Vec::new();
    for (idx, item) in batch_jobs.iter().enumerate() {
        if let Some(logmel) = item.logmel.as_ref() {
            batch_input.extend_from_slice(logmel.as_slice());
            input_indices.push(idx);
        }
    }

    if !input_indices.is_empty() {
        let batch_result = std::panic::catch_unwind(|| {
            crate::analysis::embedding::infer_embeddings_from_logmel_batch(
                batch_input.as_slice(),
                input_indices.len(),
            )
        })
        .unwrap_or_else(|_| Err("PANNs batch inference panicked".to_string()));
        match batch_result {
            Ok(embeddings) => {
                for (idx, embedding) in input_indices.iter().copied().zip(embeddings.into_iter()) {
                    if let Some(item) = batch_jobs.get_mut(idx) {
                        item.embedding = Some(embedding);
                    }
                }
            }
            Err(err) => {
                for idx in input_indices.iter().copied() {
                    let logmel = match batch_jobs.get(idx) {
                        Some(item) => item.logmel.as_ref(),
                        None => None,
                    };
                    let fallback = match logmel {
                        Some(logmel) => std::panic::catch_unwind(|| {
                            crate::analysis::embedding::infer_embedding_from_logmel(
                                logmel.as_slice(),
                            )
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

fn load_embedding_vec_optional(
    conn: &rusqlite::Connection,
    sample_id: &str,
    model_id: &str,
    expected_dim: usize,
) -> Result<Option<Vec<f32>>, String> {
    let row: Option<Vec<u8>> = conn
        .query_row(
            "SELECT vec FROM embeddings WHERE sample_id = ?1 AND model_id = ?2",
            rusqlite::params![sample_id, model_id],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()
        .map_err(|err| format!("Failed to load embedding blob for {sample_id}: {err}"))?;
    let Some(blob) = row else {
        return Ok(None);
    };
    let vec = crate::analysis::decode_f32_le_blob(&blob)?;
    if vec.len() != expected_dim {
        return Ok(None);
    }
    Ok(Some(vec))
}

fn now_epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs() as i64
}
