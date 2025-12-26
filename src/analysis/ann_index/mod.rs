mod build;
mod state;
mod storage;
mod update;

use crate::analysis::{decode_f32_le_blob, embedding};
use rusqlite::Connection;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

#[derive(Debug)]
pub struct SimilarNeighbor {
    pub sample_id: String,
    pub distance: f32,
}

static ANN_INDEX: LazyLock<Mutex<HashMap<String, state::AnnIndexState>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn with_index_state<R>(
    conn: &Connection,
    f: impl FnOnce(&mut state::AnnIndexState) -> Result<R, String>,
) -> Result<R, String> {
    let key = storage::index_key(conn)?;
    let mut guard = ANN_INDEX
        .lock()
        .map_err(|_| "ANN index lock poisoned".to_string())?;
    if !guard.contains_key(&key) {
        let state = build::load_or_build_index(conn)?;
        guard.insert(key.clone(), state);
    }
    let state = guard
        .get_mut(&key)
        .ok_or_else(|| "ANN index missing".to_string())?;
    f(state)
}

pub fn upsert_embedding(
    conn: &Connection,
    sample_id: &str,
    embedding: &[f32],
) -> Result<(), String> {
    with_index_state(conn, |state| {
        update::upsert_embedding(conn, state, sample_id, embedding)
    })
}

pub fn upsert_embeddings_batch<'a, I>(conn: &Connection, items: I) -> Result<(), String>
where
    I: IntoIterator<Item = (&'a str, &'a [f32])>,
{
    let mut iter = items.into_iter().peekable();
    if iter.peek().is_none() {
        return Ok(());
    }
    with_index_state(conn, |state| update::upsert_embeddings_batch(conn, state, iter))
}

pub fn flush_pending_inserts(conn: &Connection) -> Result<(), String> {
    with_index_state(conn, |state| update::flush_pending_inserts(conn, state))
}

pub fn find_similar(
    conn: &Connection,
    sample_id: &str,
    k: usize,
) -> Result<Vec<SimilarNeighbor>, String> {
    if k == 0 {
        return Ok(Vec::new());
    }
    let embedding = load_embedding(conn, sample_id)?;
    with_index_state(conn, |state| {
        if !state.id_lookup.contains_key(sample_id) {
            update::upsert_embedding(conn, state, sample_id, embedding.as_slice())?;
        }
        let ef = state.params.ef_search.max(k + 1);
        let neighbours = state.hnsw.search(&embedding, k + 1, ef);
        let mut results = Vec::with_capacity(k);
        for neighbour in neighbours {
            if let Some(candidate) = state.id_map.get(neighbour.d_id) {
                if candidate == sample_id {
                    continue;
                }
                results.push(SimilarNeighbor {
                    sample_id: candidate.clone(),
                    distance: neighbour.distance,
                });
                if results.len() >= k {
                    break;
                }
            }
        }
        Ok(results)
    })
}

pub fn find_similar_for_embedding(
    conn: &Connection,
    embedding: &[f32],
    k: usize,
) -> Result<Vec<SimilarNeighbor>, String> {
    if k == 0 {
        return Ok(Vec::new());
    }
    if embedding.len() != embedding::EMBEDDING_DIM {
        return Err(format!(
            "Embedding dim mismatch: expected {}, got {}",
            embedding::EMBEDDING_DIM,
            embedding.len()
        ));
    }
    with_index_state(conn, |state| {
        if state.id_map.is_empty() {
            return Err("ANN index has no embeddings".to_string());
        }
        let ef = state.params.ef_search.max(k);
        let neighbours = state.hnsw.search(embedding, k, ef);
        let mut results = Vec::with_capacity(k);
        for neighbour in neighbours {
            if let Some(candidate) = state.id_map.get(neighbour.d_id) {
                results.push(SimilarNeighbor {
                    sample_id: candidate.clone(),
                    distance: neighbour.distance,
                });
                if results.len() >= k {
                    break;
                }
            }
        }
        Ok(results)
    })
}

pub fn rebuild_index(conn: &Connection) -> Result<(), String> {
    let params = state::default_params();
    let index_path = storage::read_meta(conn, &params.model_id)?
        .map(|meta| meta.index_path)
        .unwrap_or(storage::default_index_path(conn)?);
    let mut state = build::build_index_from_db(conn, params, index_path)?;
    update::flush_index(conn, &mut state)?;
    let key = storage::index_key(conn)?;
    let mut guard = ANN_INDEX
        .lock()
        .map_err(|_| "ANN index lock poisoned".to_string())?;
    guard.insert(key, state);
    Ok(())
}

fn load_embedding(conn: &Connection, sample_id: &str) -> Result<Vec<f32>, String> {
    let blob: Vec<u8> = conn
        .query_row(
            "SELECT vec FROM embeddings WHERE sample_id = ?1 AND model_id = ?2",
            rusqlite::params![sample_id, embedding::EMBEDDING_MODEL_ID],
            |row| row.get(0),
        )
        .map_err(|err| format!("Failed to load embedding for {sample_id}: {err}"))?;
    decode_f32_le_blob(&blob)
}
