use super::container;
use super::state::AnnIndexState;
use super::storage::{hnsw_dump_paths, upsert_meta};
use hnsw_rs::api::AnnT;
use rusqlite::Connection;
use std::time::Duration;
use tempfile::Builder;

const ANN_FLUSH_INTERVAL: Duration = Duration::from_secs(30);
const ANN_FLUSH_MIN_INSERTS: usize = 64;
const ANN_TEMP_DUMP_BASENAME: &str = "ann_dump";

/// Insert a single embedding into the ANN index, flushing if needed.
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

/// Insert a batch of embeddings into the ANN index.
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

/// Force a flush of pending ANN inserts to disk.
pub(crate) fn flush_pending_inserts(
    conn: &Connection,
    state: &mut AnnIndexState,
) -> Result<(), String> {
    if state.dirty_inserts == 0 {
        return Ok(());
    }
    flush_index(conn, state)
}

/// Flush the ANN index if time or insert thresholds are exceeded.
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

/// Persist the ANN index to the single-file container format.
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
    let temp_dir = Builder::new()
        .prefix("ann_dump")
        .tempdir_in(dir)
        .map_err(|err| format!("Failed to create ANN dump dir: {err}"))?;
    dump_hnsw(state, temp_dir.path())?;
    let (graph_path, data_path) =
        hnsw_dump_paths(&temp_dir.path().join(ANN_TEMP_DUMP_BASENAME))?;
    container::write_container(
        &index_path,
        &state.params.model_id,
        &graph_path,
        &data_path,
        &state.id_map,
    )?;
    upsert_meta(conn, state)?;
    state.last_flush = std::time::Instant::now();
    state.dirty_inserts = 0;
    Ok(())
}

fn dump_hnsw(state: &AnnIndexState, dir: &std::path::Path) -> Result<(), String> {
    state
        .hnsw
        .file_dump(dir, ANN_TEMP_DUMP_BASENAME)
        .map_err(|err| format!("Failed to save ANN index: {err}"))?;
    Ok(())
}
