use super::state::AnnIndexState;
use super::storage::{save_id_map, upsert_meta};
use rusqlite::Connection;
use std::time::Duration;

const ANN_FLUSH_INTERVAL: Duration = Duration::from_secs(30);
const ANN_FLUSH_MIN_INSERTS: usize = 64;

pub(crate) fn upsert_embedding(
    conn: &Connection,
    state: &mut AnnIndexState,
    sample_id: &str,
    embedding: &[f32],
) -> Result<(), String> {
    if state.id_lookup.contains_key(sample_id) {
        return Ok(());
    }
    if embedding.len() != state.params.dim {
        return Err(format!(
            "Embedding dim mismatch: expected {}, got {}",
            state.params.dim,
            embedding.len()
        ));
    }
    let id = state.id_map.len();
    state.id_map.push(sample_id.to_string());
    state.id_lookup.insert(sample_id.to_string(), id);
    state.hnsw.insert((embedding, id));
    state.dirty_inserts += 1;
    maybe_flush(conn, state)?;
    Ok(())
}

pub(crate) fn upsert_embeddings_batch<'a, I>(
    conn: &Connection,
    state: &mut AnnIndexState,
    items: I,
) -> Result<(), String>
where
    I: IntoIterator<Item = (&'a str, &'a [f32])>,
{
    for (sample_id, embedding) in items {
        if state.id_lookup.contains_key(sample_id) {
            continue;
        }
        if embedding.len() != state.params.dim {
            return Err(format!(
                "Embedding dim mismatch: expected {}, got {}",
                state.params.dim,
                embedding.len()
            ));
        }
        let id = state.id_map.len();
        state.id_map.push(sample_id.to_string());
        state.id_lookup.insert(sample_id.to_string(), id);
        state.hnsw.insert((embedding, id));
        state.dirty_inserts += 1;
    }
    maybe_flush(conn, state)?;
    Ok(())
}

pub(crate) fn flush_pending_inserts(
    conn: &Connection,
    state: &mut AnnIndexState,
) -> Result<(), String> {
    if state.dirty_inserts == 0 {
        return Ok(());
    }
    flush_index(conn, state)
}

pub(crate) fn maybe_flush(conn: &Connection, state: &mut AnnIndexState) -> Result<(), String> {
    let elapsed = state.last_flush.elapsed();
    if state.dirty_inserts == 0 {
        return Ok(());
    }
    if state.dirty_inserts < ANN_FLUSH_MIN_INSERTS && elapsed < ANN_FLUSH_INTERVAL {
        return Ok(());
    }
    flush_index(conn, state)
}

pub(crate) fn flush_index(conn: &Connection, state: &mut AnnIndexState) -> Result<(), String> {
    if state.id_map.is_empty() {
        upsert_meta(conn, state)?;
        state.last_flush = std::time::Instant::now();
        state.dirty_inserts = 0;
        return Ok(());
    }
    let index_path = state.index_path.clone();
    let dir = index_path
        .parent()
        .ok_or_else(|| "Index path missing parent".to_string())?;
    std::fs::create_dir_all(dir).map_err(|err| format!("Failed to create ANN dir: {err}"))?;
    let basename = index_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Index path missing basename".to_string())?;
    state
        .hnsw
        .file_dump(dir, basename)
        .map_err(|err| format!("Failed to save ANN index: {err}"))?;
    save_id_map(&state.id_map_path, &state.id_map)?;
    upsert_meta(conn, state)?;
    state.last_flush = std::time::Instant::now();
    state.dirty_inserts = 0;
    Ok(())
}
