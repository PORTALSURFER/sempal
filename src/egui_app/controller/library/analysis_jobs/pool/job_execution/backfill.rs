use crate::egui_app::controller::library::analysis_jobs::db;
use rusqlite::{OptionalExtension, params};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc::channel};
use std::thread::sleep;
use std::time::Duration;
use tracing::warn;

use super::errors::ErrorCollector;
use super::support::{load_embedding_vec_optional, now_epoch_seconds};

struct EmbeddingWork {
    content_hash: String,
    absolute_path: PathBuf,
    sample_ids: Vec<String>,
}

struct EmbeddingComputation {
    content_hash: String,
    sample_ids: Vec<String>,
    embedding: Vec<f32>,
    created_at: i64,
}

struct EmbeddingResult {
    sample_id: String,
    content_hash: String,
    embedding: Vec<f32>,
    created_at: i64,
}

#[derive(Clone)]
struct EmbeddingData {
    embedding: Vec<f32>,
    created_at: i64,
}

struct BackfillPlan {
    ready: Vec<EmbeddingResult>,
    work: Vec<EmbeddingWork>,
}

struct WorkEntry {
    absolute_path: PathBuf,
    sample_ids: Vec<String>,
}

impl WorkEntry {
    fn new(absolute_path: PathBuf) -> Self {
        Self {
            absolute_path,
            sample_ids: Vec::new(),
        }
    }
}

pub(crate) fn run_embedding_backfill_job(
    conn: &rusqlite::Connection,
    job: &db::ClaimedJob,
    use_cache: bool,
    analysis_sample_rate: u32,
    analysis_version: &str,
) -> Result<(), String> {
    let sample_ids = parse_backfill_payload(job)?;
    if sample_ids.is_empty() {
        return Ok(());
    }

    let plan = build_backfill_plan(conn, job, &sample_ids, use_cache, analysis_version)?;
    let (computed, errors) = run_embedding_workers(plan.work, analysis_sample_rate);
    let mut results = plan.ready;
    results.extend(expand_computations(computed));
    if results.is_empty() {
        if !errors.is_empty() {
            return Err(format!("Embedding backfill failed: {:?}", errors));
        }
        return Ok(());
    }

    write_backfill_results(conn, &results, analysis_version)?;

    if !errors.is_empty() {
        warn!("Embedding backfill had errors: {:?}", errors);
    }

    Ok(())
}

fn parse_backfill_payload(job: &db::ClaimedJob) -> Result<Vec<String>, String> {
    let payload = job
        .content_hash
        .as_deref()
        .ok_or_else(|| "Embedding backfill payload missing".to_string())?;
    serde_json::from_str(payload)
        .map_err(|err| format!("Invalid embedding backfill payload: {err}"))
}

fn build_backfill_plan(
    conn: &rusqlite::Connection,
    job: &db::ClaimedJob,
    sample_ids: &[String],
    use_cache: bool,
    analysis_version: &str,
) -> Result<BackfillPlan, String> {
    let mut ready = Vec::new();
    let mut work_by_hash: HashMap<String, WorkEntry> = HashMap::new();
    let mut embedding_cache: HashMap<String, EmbeddingData> = HashMap::new();

    for sample_id in sample_ids {
        if load_embedding_vec_optional(
            conn,
            sample_id,
            crate::analysis::similarity::SIMILARITY_MODEL_ID,
            crate::analysis::similarity::SIMILARITY_DIM,
        )?
        .is_some()
        {
            continue;
        }
        let Some(content_hash) = db::sample_content_hash(conn, sample_id)? else {
            continue;
        };
        if let Some(data) = embedding_cache.get(&content_hash) {
            ready.push(materialize_result(sample_id, &content_hash, data));
            continue;
        }
        if use_cache {
            if let Some(data) = cached_embedding_data(conn, &content_hash, analysis_version)? {
                embedding_cache.insert(content_hash.clone(), data.clone());
                ready.push(materialize_result(sample_id, &content_hash, &data));
                continue;
            }
        }
        if let Some(features) = load_features_vec_optional(conn, sample_id)? {
            if let Ok(data) = embedding_data_from_features(&features) {
                embedding_cache.insert(content_hash.clone(), data.clone());
                ready.push(materialize_result(sample_id, &content_hash, &data));
                continue;
            }
        }
        if use_cache {
            if let Some(data) =
                cached_feature_embedding_data(conn, &content_hash, analysis_version)?
            {
                embedding_cache.insert(content_hash.clone(), data.clone());
                ready.push(materialize_result(sample_id, &content_hash, &data));
                continue;
            }
        }

        let Some(absolute_path) = resolve_backfill_path(job, sample_id) else {
            continue;
        };
        let entry = work_by_hash
            .entry(content_hash.clone())
            .or_insert_with(|| WorkEntry::new(absolute_path.clone()));
        entry.sample_ids.push(sample_id.clone());
    }

    let work = work_by_hash
        .into_iter()
        .map(|(content_hash, entry)| EmbeddingWork {
            content_hash,
            absolute_path: entry.absolute_path,
            sample_ids: entry.sample_ids,
        })
        .collect();

    Ok(BackfillPlan { ready, work })
}

fn resolve_backfill_path(job: &db::ClaimedJob, sample_id: &str) -> Option<PathBuf> {
    let (_source_id, relative_path) = match db::parse_sample_id(sample_id) {
        Ok(parsed) => parsed,
        Err(err) => {
            warn!("Skipping embed backfill sample_id={sample_id}: {err}");
            return None;
        }
    };
    let absolute_path = job.source_root.join(&relative_path);
    if !absolute_path.exists() {
        warn!(
            "Missing file for embed backfill: {}",
            absolute_path.display()
        );
        return None;
    }
    Some(absolute_path)
}

fn cached_embedding_data(
    conn: &rusqlite::Connection,
    content_hash: &str,
    analysis_version: &str,
) -> Result<Option<EmbeddingData>, String> {
    let Some(cached) = db::cached_embedding_by_hash(
        conn,
        content_hash,
        analysis_version,
        crate::analysis::similarity::SIMILARITY_MODEL_ID,
    )?
    else {
        return Ok(None);
    };
    let Ok(vec) = crate::analysis::decode_f32_le_blob(&cached.vec_blob) else {
        return Ok(None);
    };
    if vec.len() != crate::analysis::similarity::SIMILARITY_DIM {
        return Ok(None);
    }
    Ok(Some(EmbeddingData {
        embedding: vec,
        created_at: cached.created_at,
    }))
}

fn cached_feature_embedding_data(
    conn: &rusqlite::Connection,
    content_hash: &str,
    analysis_version: &str,
) -> Result<Option<EmbeddingData>, String> {
    let Some(cached) = db::cached_features_by_hash(
        conn,
        content_hash,
        analysis_version,
        crate::analysis::vector::FEATURE_VERSION_V1,
    )?
    else {
        return Ok(None);
    };
    let Ok(features) = crate::analysis::decode_f32_le_blob(&cached.vec_blob) else {
        return Ok(None);
    };
    let Ok(data) = embedding_data_from_features(&features) else {
        return Ok(None);
    };
    Ok(Some(data))
}

fn embedding_data_from_features(features: &[f32]) -> Result<EmbeddingData, String> {
    let embedding = crate::analysis::similarity::embedding_from_features(features)?;
    Ok(EmbeddingData {
        embedding,
        created_at: now_epoch_seconds(),
    })
}

fn materialize_result(
    sample_id: &str,
    content_hash: &str,
    data: &EmbeddingData,
) -> EmbeddingResult {
    EmbeddingResult {
        sample_id: sample_id.to_string(),
        content_hash: content_hash.to_string(),
        embedding: data.embedding.clone(),
        created_at: data.created_at,
    }
}

fn run_embedding_workers(
    work: Vec<EmbeddingWork>,
    analysis_sample_rate: u32,
) -> (Vec<EmbeddingComputation>, Vec<String>) {
    if work.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let worker_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(work.len())
        .max(1);
    let queue = Arc::new(Mutex::new(VecDeque::from(work)));
    let (tx, rx) = channel();

    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            let queue = Arc::clone(&queue);
            let tx = tx.clone();
            scope.spawn(move || {
                let embedding_batch_max = crate::analysis::similarity::SIMILARITY_BATCH_MAX;
                loop {
                    let batch = {
                        let mut guard = match queue.lock() {
                            Ok(guard) => guard,
                            Err(_) => return,
                        };
                        drain_batch(&mut guard, embedding_batch_max)
                    };
                    if batch.is_empty() {
                        break;
                    }

                    for work in batch {
                        let decoded = match crate::analysis::audio::decode_for_analysis_with_rate(
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
                        let features =
                            match crate::analysis::compute_feature_vector_v1_for_mono_samples(
                                &decoded.mono,
                                decoded.sample_rate_used,
                            ) {
                                Ok(features) => features,
                                Err(err) => {
                                    let _ = tx.send(Err(format!(
                                        "Feature extraction failed for {}: {err}",
                                        work.absolute_path.display()
                                    )));
                                    continue;
                                }
                            };
                        let embedding =
                            match crate::analysis::similarity::embedding_from_features(&features) {
                                Ok(embedding) => embedding,
                                Err(err) => {
                                    let _ = tx.send(Err(format!(
                                        "Embedding build failed for {}: {err}",
                                        work.absolute_path.display()
                                    )));
                                    continue;
                                }
                            };
                        let _ = tx.send(Ok(EmbeddingComputation {
                            content_hash: work.content_hash,
                            sample_ids: work.sample_ids,
                            embedding,
                            created_at: now_epoch_seconds(),
                        }));
                    }
                }
            });
        }
        drop(tx);
    });

    collect_results(rx)
}

fn expand_computations(computed: Vec<EmbeddingComputation>) -> Vec<EmbeddingResult> {
    let mut results = Vec::new();
    for item in computed {
        for sample_id in item.sample_ids {
            results.push(EmbeddingResult {
                sample_id,
                content_hash: item.content_hash.clone(),
                embedding: item.embedding.clone(),
                created_at: item.created_at,
            });
        }
    }
    results
}

fn drain_batch(queue: &mut VecDeque<EmbeddingWork>, batch_max: usize) -> Vec<EmbeddingWork> {
    let mut batch = Vec::with_capacity(batch_max);
    for _ in 0..batch_max {
        let Some(work) = queue.pop_front() else {
            break;
        };
        batch.push(work);
    }
    batch
}

fn collect_results(
    rx: std::sync::mpsc::Receiver<Result<EmbeddingComputation, String>>,
) -> (Vec<EmbeddingComputation>, Vec<String>) {
    let mut results = Vec::new();
    let mut errors = ErrorCollector::new(3);
    while let Ok(result) = rx.recv() {
        match result {
            Ok(result) => results.push(result),
            Err(err) => errors.push(err),
        }
    }
    (results, errors.into_vec())
}

fn write_backfill_results(
    conn: &rusqlite::Connection,
    results: &[EmbeddingResult],
    analysis_version: &str,
) -> Result<(), String> {
    const INSERT_BATCH: usize = 128;
    for chunk in results.chunks(INSERT_BATCH) {
        retry_backfill_write_with(
            || write_backfill_chunk(conn, chunk, analysis_version),
            3,
            Duration::from_millis(50),
        )?;
    }
    Ok(())
}

fn write_backfill_chunk(
    conn: &rusqlite::Connection,
    chunk: &[EmbeddingResult],
    analysis_version: &str,
) -> Result<(), String> {
    conn.execute_batch("BEGIN IMMEDIATE")
        .map_err(|err| format!("Begin embedding backfill tx failed: {err}"))?;
    for result in chunk {
        let embedding_blob = crate::analysis::vector::encode_f32_le_blob(&result.embedding);
        if let Err(err) = db::upsert_embedding(
            conn,
            &result.sample_id,
            crate::analysis::similarity::SIMILARITY_MODEL_ID,
            crate::analysis::similarity::SIMILARITY_DIM as i64,
            crate::analysis::similarity::SIMILARITY_DTYPE_F32,
            true,
            &embedding_blob,
            result.created_at,
        ) {
            let _ = conn.execute_batch("ROLLBACK");
            return Err(err);
        }
        db::upsert_cached_embedding(
            conn,
            &result.content_hash,
            analysis_version,
            crate::analysis::similarity::SIMILARITY_MODEL_ID,
            crate::analysis::similarity::SIMILARITY_DIM as i64,
            crate::analysis::similarity::SIMILARITY_DTYPE_F32,
            true,
            &embedding_blob,
            result.created_at,
        )?;
    }
    if let Err(err) = crate::analysis::ann_index::upsert_embeddings_batch(
        conn,
        chunk
            .iter()
            .map(|result| (result.sample_id.as_str(), result.embedding.as_slice())),
    ) {
        warn!("ANN index batch update failed: {err}");
    }
    conn.execute_batch("COMMIT")
        .map_err(|err| format!("Commit embedding backfill tx failed: {err}"))?;
    Ok(())
}

fn retry_backfill_write_with<F>(mut op: F, retries: usize, delay: Duration) -> Result<(), String>
where
    F: FnMut() -> Result<(), String>,
{
    for attempt in 0..retries {
        match op() {
            Ok(()) => return Ok(()),
            Err(_err) if attempt + 1 < retries => {
                if !delay.is_zero() {
                    sleep(delay);
                }
            }
            Err(err) => return Err(err),
        }
    }
    Err("Embedding backfill retries exhausted".to_string())
}

fn load_features_vec_optional(
    conn: &rusqlite::Connection,
    sample_id: &str,
) -> Result<Option<Vec<f32>>, String> {
    let blob: Option<Vec<u8>> = conn
        .query_row(
            "SELECT vec_blob FROM features WHERE sample_id = ?1",
            params![sample_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|err| format!("Failed to load features for {sample_id}: {err}"))?;
    let Some(blob) = blob else {
        return Ok(None);
    };
    let vec = crate::analysis::decode_f32_le_blob(&blob)?;
    if vec.len() != crate::analysis::vector::FEATURE_VECTOR_LEN_V1 {
        return Ok(None);
    }
    Ok(Some(vec))
}

#[cfg(test)]
mod tests;
