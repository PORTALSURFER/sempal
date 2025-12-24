use crate::egui_app::controller::analysis_jobs::db;
use rusqlite::OptionalExtension;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc::channel};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::warn;

pub(super) fn run_job(
    conn: &rusqlite::Connection,
    job: &db::ClaimedJob,
    max_analysis_duration_seconds: f32,
) -> Result<(), String> {
    match job.job_type.as_str() {
        db::ANALYZE_SAMPLE_JOB_TYPE => run_analysis_job(conn, job, max_analysis_duration_seconds),
        db::EMBEDDING_BACKFILL_JOB_TYPE => run_embedding_backfill_job(conn, job),
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

    let mut roots: HashMap<String, PathBuf> = HashMap::new();
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
        if let Some(cached) = db::cached_embedding_by_hash(
            conn,
            &content_hash,
            crate::analysis::version::analysis_version(),
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
        let (source_id, relative_path) = match db::parse_sample_id(&sample_id) {
            Ok(parsed) => parsed,
            Err(err) => {
                warn!("Skipping embed backfill sample_id={sample_id}: {err}");
                continue;
            }
        };
        let root = if let Some(root) = roots.get(&source_id) {
            root.clone()
        } else {
            let Some(root) = db::source_root_for(conn, &source_id)? else {
                warn!("Missing source root for embed backfill source_id={source_id}");
                continue;
            };
            roots.insert(source_id.clone(), root.clone());
            root
        };
        let absolute_path = root.join(&relative_path);
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
                loop {
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
                    let decoded =
                        match crate::analysis::audio::decode_for_analysis(&work.absolute_path) {
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
                    let embedding = match crate::analysis::embedding::infer_embedding(
                        &processed,
                        decoded.sample_rate_used,
                    ) {
                        Ok(embedding) => embedding,
                        Err(err) => {
                            let _ = tx.send(Err(format!(
                                "Embed failed for {}: {err}",
                                work.absolute_path.display()
                            )));
                            continue;
                        }
                    };
                    let _ = tx.send(Ok(EmbeddingResult {
                        sample_id: work.sample_id,
                        content_hash: work.content_hash,
                        embedding,
                    }));
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
                crate::analysis::version::analysis_version(),
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
    max_analysis_duration_seconds: f32,
) -> Result<(), String> {
    let content_hash = job
        .content_hash
        .as_deref()
        .ok_or_else(|| format!("Missing content_hash for analysis job {}", job.sample_id))?;
    let current_hash = db::sample_content_hash(conn, &job.sample_id)?;
    if current_hash.as_deref() != Some(content_hash) {
        return Ok(());
    }
    let analysis_version = crate::analysis::version::analysis_version();
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

    let (source_id, relative_path) = db::parse_sample_id(&job.sample_id)?;
    let Some(root) = db::source_root_for(conn, &source_id)? else {
        return Err(format!(
            "Source not found for job sample_id={}",
            job.sample_id
        ));
    };
    let absolute = root.join(&relative_path);
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
                    )?;
                    return Ok(());
                }
            }
        }
    }
    let decoded = crate::analysis::audio::decode_for_analysis(&absolute)?;
    run_analysis_job_with_decoded(conn, job, decoded)
}

pub(super) fn run_analysis_job_with_decoded(
    conn: &rusqlite::Connection,
    job: &db::ClaimedJob,
    decoded: crate::analysis::audio::AnalysisAudio,
) -> Result<(), String> {
    let content_hash = job
        .content_hash
        .as_deref()
        .ok_or_else(|| format!("Missing content_hash for analysis job {}", job.sample_id))?;
    let embedding = if let Some(cached) = load_embedding_vec_optional(
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
        let embedding =
            crate::analysis::embedding::infer_embedding(&processed, decoded.sample_rate_used)?;
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
        embedding
    };
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
    )?;
    let current_hash = db::sample_content_hash(conn, &job.sample_id)?;
    if current_hash.as_deref() != Some(content_hash) {
        return Ok(());
    }
    crate::analysis::ann_index::upsert_embedding(conn, &job.sample_id, &embedding)?;
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
    let analysis_version = crate::analysis::version::analysis_version();
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
