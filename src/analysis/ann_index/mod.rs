#[cfg(test)]
pub(crate) mod build;
#[cfg(not(test))]
mod build;
mod container;
#[cfg(test)]
pub(crate) mod state;
#[cfg(not(test))]
mod state;
#[cfg(test)]
pub(crate) mod storage;
#[cfg(not(test))]
mod storage;
#[cfg(test)]
pub(crate) mod update;
#[cfg(not(test))]
mod update;

use crate::analysis::{decode_f32_le_blob, similarity};
use rusqlite::Connection;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, RwLock};

/// Neighbor result returned by ANN similarity search.
#[derive(Debug)]
pub struct SimilarNeighbor {
    /// Sample identifier for the neighbor.
    pub sample_id: String,
    /// Distance between the query and the neighbor (lower is more similar).
    pub distance: f32,
}

static ANN_INDEX: LazyLock<RwLock<HashMap<String, Arc<RwLock<state::AnnIndexState>>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Get the index state wrapper, loading it if necessary.
/// This minimizes the time the global lock is held.
fn get_index_entry(
    conn: &Connection,
) -> Result<Arc<RwLock<state::AnnIndexState>>, String> {
    let key = storage::index_key(conn)?;
    
    // Fast path: check with read lock
    {
        let guard = ANN_INDEX.read().map_err(|_| "ANN index lock poisoned")?;
        if let Some(state) = guard.get(&key) {
            return Ok(state.clone());
        }
    }

    // Slow path: load or build index without holding global lock
    // Note: This might do redundant work if multiple threads race here,
    // but better than blocking the world.
    let loaded_state = build::load_or_build_index(conn)?;
    let loaded_state = Arc::new(RwLock::new(loaded_state));

    // Write lock to insert
    let mut guard = ANN_INDEX.write().map_err(|_| "ANN index lock poisoned")?;
    // Double-check in case another thread won the race
    if let Some(state) = guard.get(&key) {
        return Ok(state.clone());
    }
    
    guard.insert(key, loaded_state.clone());
    Ok(loaded_state)
}

fn with_index_state_mut<R>(
    conn: &Connection,
    f: impl FnOnce(&mut state::AnnIndexState) -> Result<R, String>,
) -> Result<R, String> {
    let state_arc = get_index_entry(conn)?;
    let mut guard = state_arc.write().map_err(|_| "ANN index state lock poisoned")?;
    f(&mut guard)
}

fn with_index_state_read<R>(
    conn: &Connection,
    f: impl FnOnce(&state::AnnIndexState) -> Result<R, String>,
) -> Result<R, String> {
    let state_arc = get_index_entry(conn)?;
    let guard = state_arc.read().map_err(|_| "ANN index state lock poisoned")?;
    f(&guard)
}

/// Insert or update a single embedding in the ANN index.
pub fn upsert_embedding(
    conn: &Connection,
    sample_id: &str,
    embedding: &[f32],
) -> Result<(), String> {
    with_index_state_mut(conn, |state| {
        update::upsert_embedding(conn, state, sample_id, embedding)
    })
}

/// Insert or update a batch of embeddings in the ANN index.
pub fn upsert_embeddings_batch<'a, I>(conn: &Connection, items: I) -> Result<(), String>
where
    I: IntoIterator<Item = (&'a str, &'a [f32])>,
{
    let mut iter = items.into_iter().peekable();
    if iter.peek().is_none() {
        return Ok(());
    }
    with_index_state_mut(conn, |state| {
        update::upsert_embeddings_batch(conn, state, iter)
    })
}

/// Flush any buffered ANN insertions to the on-disk index.
pub fn flush_pending_inserts(conn: &Connection) -> Result<(), String> {
    with_index_state_mut(conn, |state| update::flush_pending_inserts(conn, state))
}

/// Find the `k` nearest neighbors for a stored sample id.
pub fn find_similar(
    conn: &Connection,
    sample_id: &str,
    k: usize,
) -> Result<Vec<SimilarNeighbor>, String> {
    if k == 0 {
        return Ok(Vec::new());
    }
    let embedding = load_embedding(conn, sample_id)?;

    // Optimistic read
    {
        let state_arc = get_index_entry(conn)?;
        let state = state_arc.read().map_err(|_| "ANN index state lock poisoned")?;
        
        // If the ID is already known, we can search with just the read lock
        if state.id_lookup.contains_key(sample_id) {
            let results = perform_search(&state, &embedding, k, Some(sample_id))?;
            if results.len() >= k {
                return Ok(results);
            }
        }
        // If not found, drop read lock and fall through to write path
    }

    // Write path: update index with missing ID then search
    with_index_state_mut(conn, |state| {
        if !state.id_lookup.contains_key(sample_id) {
             update::upsert_embedding(conn, state, sample_id, embedding.as_slice())?;
        }
        let results = perform_search(state, &embedding, k, Some(sample_id))?;
        if results.len() >= k {
            return Ok(results);
        }
        fallback_neighbors(conn, &embedding, k, Some(sample_id))
    })
}

/// Find the `k` nearest neighbors for an in-memory embedding.
pub fn find_similar_for_embedding(
    conn: &Connection,
    embedding: &[f32],
    k: usize,
) -> Result<Vec<SimilarNeighbor>, String> {
    if k == 0 {
        return Ok(Vec::new());
    }
    if embedding.len() != similarity::SIMILARITY_DIM {
        return Err(format!(
            "Embedding dim mismatch: expected {}, got {}",
            similarity::SIMILARITY_DIM,
            embedding.len()
        ));
    }
    with_index_state_read(conn, |state| {
         if state.id_map.is_empty() {
            return Err("ANN index has no embeddings".to_string());
        }
        let results = perform_search(state, embedding, k, None)?;
        if results.len() >= k {
            return Ok(results);
        }
        fallback_neighbors(conn, embedding, k, None)
    })
}

fn perform_search(
    state: &state::AnnIndexState,
    embedding: &[f32],
    k: usize,
    skip_id: Option<&str>,
) -> Result<Vec<SimilarNeighbor>, String> {
    let ef = state.params.ef_search.max(k + 1);
    let total = state.id_map.len();
    let mut requested = k + if skip_id.is_some() { 1 } else { 0 };
    if requested > total {
        requested = total;
    }
    let neighbours = state.hnsw.search(embedding, requested, ef);
    let mut results = Vec::with_capacity(neighbours.len());
    for neighbour in neighbours {
        if let Some(candidate) = state.id_map.get(neighbour.d_id) {
            if let Some(skip) = skip_id {
                if candidate == skip {
                    continue;
                }
            }
            results.push(SimilarNeighbor {
                sample_id: candidate.clone(),
                distance: neighbour.distance,
            });
        }
    }
    results.sort_by(|a, b| {
        a.distance
            .partial_cmp(&b.distance)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(k);
    Ok(results)
}

fn fallback_neighbors(
    conn: &Connection,
    embedding: &[f32],
    k: usize,
    skip_id: Option<&str>,
) -> Result<Vec<SimilarNeighbor>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT sample_id, vec
             FROM embeddings
             WHERE model_id = ?1",
        )
        .map_err(|err| format!("Failed to query embeddings: {err}"))?;
    let mut rows = stmt
        .query(rusqlite::params![similarity::SIMILARITY_MODEL_ID])
        .map_err(|err| format!("Failed to iterate embeddings: {err}"))?;
    let mut scored: Vec<SimilarNeighbor> = Vec::new();
    while let Some(row) = rows.next().map_err(|err| err.to_string())? {
        let sample_id: String = row.get(0).map_err(|err| err.to_string())?;
        if skip_id == Some(sample_id.as_str()) {
            continue;
        }
        let blob: Vec<u8> = row.get(1).map_err(|err| err.to_string())?;
        let candidate = decode_f32_le_blob(&blob)?;
        if candidate.len() != embedding.len() {
            continue;
        }
        let distance = cosine_distance(embedding, &candidate);
        scored.push(SimilarNeighbor { sample_id, distance });
    }
    scored.sort_by(|a, b| {
        a.distance
            .partial_cmp(&b.distance)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored.truncate(k);
    Ok(scored)
}

fn cosine_distance(a: &[f32], b: &[f32]) -> f32 {
    let len = a.len().min(b.len());
    let mut dot = 0.0;
    for i in 0..len {
        dot += a[i] * b[i];
    }
    1.0 - dot
}

/// Rebuild the ANN index from the embeddings stored in the database.
pub fn rebuild_index(conn: &Connection) -> Result<(), String> {
    let params = state::default_params();
    let index_path = storage::default_index_path(conn)?;
    let mut state = build::build_index_from_db(conn, params, index_path)?;
    update::flush_index(conn, &mut state)?;
    let key = storage::index_key(conn)?;
    
    let wrapped_state = Arc::new(RwLock::new(state));
    let mut guard = ANN_INDEX
        .write()
        .map_err(|_| "ANN index lock poisoned".to_string())?;
    guard.insert(key, wrapped_state);
    Ok(())
}

fn load_embedding(conn: &Connection, sample_id: &str) -> Result<Vec<f32>, String> {
    let blob: Vec<u8> = conn
        .query_row(
            "SELECT vec FROM embeddings WHERE sample_id = ?1 AND model_id = ?2",
            rusqlite::params![sample_id, similarity::SIMILARITY_MODEL_ID],
            |row| row.get(0),
        )
        .map_err(|err| format!("Failed to load embedding for {sample_id}: {err}"))?;
    decode_f32_le_blob(&blob)
}
