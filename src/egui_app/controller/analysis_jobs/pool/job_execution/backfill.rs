use crate::egui_app::controller::analysis_jobs::db;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{mpsc::channel, Arc, Mutex};
use std::thread::sleep;
use std::time::Duration;
use tracing::warn;

use super::errors::ErrorCollector;
use super::support::{load_embedding_vec_optional, now_epoch_seconds};

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

pub(super) fn run_embedding_backfill_job(
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

                    let mut payloads = Vec::new();
                    let mut logmels = Vec::new();
                    let mut logmel_scratch =
                        crate::analysis::embedding::PannsLogMelScratch::default();
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

                    let embeddings = if crate::analysis::embedding::embedding_pipeline_enabled() {
                        let inflight = crate::analysis::embedding::embedding_inflight_max();
                        let micro_batch = crate::analysis::embedding::embedding_batch_max();
                        crate::analysis::embedding::infer_embeddings_from_logmel_batch_pipelined(
                            &logmels,
                            micro_batch,
                            inflight,
                        )
                    } else {
                        let micro_batch = crate::analysis::embedding::embedding_batch_max();
                        crate::analysis::embedding::infer_embeddings_from_logmel_batch_chunked(
                            &logmels,
                            micro_batch,
                        )
                    };
                    for ((sample_id, content_hash), result) in
                        payloads.into_iter().zip(embeddings.into_iter())
                    {
                        match result {
                            Ok(embedding) => {
                                let _ = tx.send(Ok(EmbeddingResult {
                                    sample_id,
                                    content_hash,
                                    embedding,
                                }));
                            }
                            Err(err) => {
                                let _ = tx.send(Err(format!(
                                    "Embed failed for {}: {err}",
                                    sample_id
                                )));
                            }
                        }
                    }
                }
            });
        }
        drop(tx);
    });

    let (results, errors) = collect_results(rx);
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
    rx: std::sync::mpsc::Receiver<Result<EmbeddingResult, String>>,
) -> (Vec<EmbeddingResult>, Vec<String>) {
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
        chunk.iter()
            .map(|result| (result.sample_id.as_str(), result.embedding.as_slice())),
    ) {
        warn!("ANN index batch update failed: {err}");
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_work(id: &str) -> EmbeddingWork {
        EmbeddingWork {
            sample_id: id.to_string(),
            absolute_path: PathBuf::from(format!("dummy/{id}.wav")),
            content_hash: format!("hash-{id}"),
        }
    }

    #[test]
    fn drain_batch_caps_at_limit() {
        let mut queue = VecDeque::new();
        queue.push_back(make_work("a"));
        queue.push_back(make_work("b"));
        queue.push_back(make_work("c"));

        let batch = drain_batch(&mut queue, 2);
        assert_eq!(batch.len(), 2);
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.front().unwrap().sample_id, "c");
    }

    #[test]
    fn collect_results_limits_error_list() {
        let (tx, rx) = channel();
        tx.send(Err("err-1".to_string())).unwrap();
        tx.send(Ok(EmbeddingResult {
            sample_id: "a".to_string(),
            content_hash: "hash-a".to_string(),
            embedding: vec![0.0_f32; 2],
        }))
        .unwrap();
        tx.send(Err("err-2".to_string())).unwrap();
        tx.send(Err("err-3".to_string())).unwrap();
        tx.send(Err("err-4".to_string())).unwrap();
        drop(tx);

        let (results, errors) = collect_results(rx);
        assert_eq!(results.len(), 1);
        assert_eq!(errors.len(), 3);
        assert_eq!(errors[0], "err-1");
        assert_eq!(errors[2], "err-3");
    }

    #[test]
    fn backfill_retry_succeeds_after_failures() {
        let mut attempts = 0;
        let result = retry_backfill_write_with(
            || {
                attempts += 1;
                if attempts < 3 {
                    Err("nope".to_string())
                } else {
                    Ok(())
                }
            },
            4,
            Duration::from_millis(0),
        );
        assert!(result.is_ok());
        assert_eq!(attempts, 3);
    }

    #[test]
    fn backfill_retry_stops_after_limit() {
        let mut attempts = 0;
        let result = retry_backfill_write_with(
            || {
                attempts += 1;
                Err("nope".to_string())
            },
            3,
            Duration::from_millis(0),
        );
        assert!(result.is_err());
        assert_eq!(attempts, 3);
    }
}
