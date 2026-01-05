use super::container;
use super::state::{
    AnnIndexMetaRow, AnnIndexParams, AnnIndexState, build_id_lookup, default_params,
};
use super::storage::{
    default_index_path, hnsw_dump_paths, legacy_id_map_path_for, load_legacy_id_map, read_meta,
};
use crate::analysis::decode_f32_le_blob;
use hnsw_rs::hnswio::HnswIo;
use hnsw_rs::prelude::*;
use rusqlite::{Connection, params};
use std::path::PathBuf;
use std::time::Instant;
use tempfile::Builder;

const TEMP_UNPACK_BASENAME: &str = "ann_unpack";

/// Load an ANN index from disk or build a new one from embeddings.
pub(crate) fn load_or_build_index(conn: &Connection) -> Result<AnnIndexState, String> {
    let params = default_params();
    let meta = read_meta(conn, &params.model_id)?;
    if let Some(meta_row) = meta.as_ref() {
        if meta_row.params == params {
            if let Some(outcome) = load_index_from_disk(conn, meta_row)? {
                let mut state = outcome.state;
                if outcome.needs_migration {
                    super::update::flush_index(conn, &mut state)?;
                } else if outcome.needs_meta_update {
                    super::storage::upsert_meta(conn, &state)?;
                }
                return Ok(state);
            }
        }
    }
    let index_path = default_index_path(conn)?;
    let mut state = build_index_from_db(conn, params, index_path)?;
    super::update::flush_index(conn, &mut state)?;
    Ok(state)
}

/// Build a fresh ANN index from the embeddings table.
pub(crate) fn build_index_from_db(
    conn: &Connection,
    params: AnnIndexParams,
    index_path: PathBuf,
) -> Result<AnnIndexState, String> {
    let count = count_embeddings(conn, &params.model_id)?;
    let mut hnsw = build_hnsw(&params, count);
    let mut id_map = Vec::with_capacity(count.max(0) as usize);
    insert_embeddings(conn, &params, &mut hnsw, &mut id_map)?;
    Ok(build_state(params, hnsw, id_map, index_path))
}

/// Attempt to load an ANN index from disk using stored metadata.
pub(crate) fn load_index_from_disk(
    conn: &Connection,
    meta: &AnnIndexMetaRow,
) -> Result<Option<LoadOutcome>, String> {
    let container_path = default_index_path(conn)?;
    if let Some(outcome) = load_container_outcome(meta, &container_path)? {
        return Ok(Some(outcome));
    }
    load_legacy_outcome(conn, meta, &container_path)
}

fn count_embeddings(conn: &Connection, model_id: &str) -> Result<i64, String> {
    conn.query_row(
        "SELECT COUNT(*) FROM embeddings WHERE model_id = ?1",
        params![model_id],
        |row| row.get(0),
    )
    .map_err(|err| format!("Failed to count embeddings: {err}"))
}

fn build_hnsw(params: &AnnIndexParams, count: i64) -> Hnsw<'static, f32, DistCosine> {
    let max_elements = (count.max(1) as usize).max(1024);
    Hnsw::new(
        params.max_nb_connection,
        max_elements,
        params.max_layer,
        params.ef_construction,
        DistCosine {},
    )
}

fn insert_embeddings(
    conn: &Connection,
    params: &AnnIndexParams,
    hnsw: &mut Hnsw<f32, DistCosine>,
    id_map: &mut Vec<String>,
) -> Result<(), String> {
    let mut stmt = conn
        .prepare(
            "SELECT sample_id, vec
             FROM embeddings
             WHERE model_id = ?1
             ORDER BY sample_id ASC",
        )
        .map_err(|err| format!("Failed to query embeddings: {err}"))?;
    let mut rows = stmt
        .query(params![params.model_id])
        .map_err(|err| format!("Failed to iterate embeddings: {err}"))?;
    while let Some(row) = rows.next().map_err(|err| err.to_string())? {
        let sample_id: String = row.get(0).map_err(|err| err.to_string())?;
        let blob: Vec<u8> = row.get(1).map_err(|err| err.to_string())?;
        let embedding = decode_f32_le_blob(&blob)?;
        if embedding.len() != params.dim {
            continue;
        }
        let id = id_map.len();
        id_map.push(sample_id);
        hnsw.insert((embedding.as_slice(), id));
    }
    Ok(())
}

fn build_state(
    params: AnnIndexParams,
    hnsw: Hnsw<'static, f32, DistCosine>,
    id_map: Vec<String>,
    index_path: PathBuf,
) -> AnnIndexState {
    let id_lookup = build_id_lookup(&id_map);
    AnnIndexState {
        hnsw,
        id_map,
        id_lookup,
        params,
        index_path,
        last_flush: Instant::now(),
        dirty_inserts: 0,
    }
}

/// Result of loading an ANN index from disk.
pub(crate) struct LoadOutcome {
    pub(crate) state: AnnIndexState,
    pub(crate) needs_migration: bool,
    pub(crate) needs_meta_update: bool,
}

impl LoadOutcome {
    fn new(state: AnnIndexState, needs_migration: bool, needs_meta_update: bool) -> Self {
        Self {
            state,
            needs_migration,
            needs_meta_update,
        }
    }
}

fn load_container_index(
    index_path: &PathBuf,
    params: &AnnIndexParams,
) -> Result<Option<AnnIndexState>, String> {
    if !index_path.is_file() {
        return Ok(None);
    }
    let temp_dir = temp_unpack_dir()?;
    let unpack = match container::unpack_container(
        index_path,
        temp_dir.path(),
        TEMP_UNPACK_BASENAME,
    ) {
        Ok(unpack) => unpack,
        Err(_) => return Ok(None),
    };
    if unpack.model_id != params.model_id {
        return Ok(None);
    }
    let hnsw = match load_hnsw(temp_dir.path(), TEMP_UNPACK_BASENAME) {
        Ok(hnsw) => hnsw,
        Err(_) => return Ok(None),
    };
    build_loaded_state(hnsw, unpack.id_map, params, index_path.clone())
}

fn load_legacy_index(
    index_path: &PathBuf,
    params: &AnnIndexParams,
) -> Result<Option<AnnIndexState>, String> {
    let (graph_path, data_path) = hnsw_dump_paths(index_path)?;
    if !graph_path.is_file() || !data_path.is_file() {
        return Ok(None);
    }
    let id_map_path = legacy_id_map_path_for(index_path);
    if !id_map_path.is_file() {
        return Ok(None);
    }
    let id_map = match load_legacy_id_map(&id_map_path) {
        Ok(id_map) => id_map,
        Err(_) => return Ok(None),
    };
    let hnsw = match load_hnsw_from_path(index_path) {
        Ok(hnsw) => hnsw,
        Err(_) => return Ok(None),
    };
    build_loaded_state(hnsw, id_map, params, index_path.clone())
}

fn load_hnsw(
    dir: &std::path::Path,
    basename: &str,
) -> Result<Hnsw<'static, f32, DistCosine>, String> {
    let hnsw_io = Box::new(HnswIo::new(dir, basename));
    let hnsw_io = Box::leak(hnsw_io);
    hnsw_io
        .load_hnsw::<f32, DistCosine>()
        .map_err(|_| "Failed to read ANN index".to_string())
}

fn load_hnsw_from_path(index_path: &PathBuf) -> Result<Hnsw<'static, f32, DistCosine>, String> {
    let basename = index_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Index path missing basename".to_string())?;
    let dir = index_path
        .parent()
        .ok_or_else(|| "Index path missing parent".to_string())?;
    load_hnsw(dir, basename)
}

fn load_container_outcome(
    meta: &AnnIndexMetaRow,
    container_path: &PathBuf,
) -> Result<Option<LoadOutcome>, String> {
    if let Some(state) = load_container_index(&meta.index_path, &meta.params)? {
        let needs_migration = meta.index_path != *container_path;
        let state = update_index_path(state, container_path, needs_migration);
        return Ok(Some(LoadOutcome::new(
            state,
            needs_migration,
            needs_migration,
        )));
    }
    if meta.index_path != *container_path {
        if let Some(state) = load_container_index(container_path, &meta.params)? {
            return Ok(Some(LoadOutcome::new(state, false, true)));
        }
    }
    Ok(None)
}

fn load_legacy_outcome(
    conn: &Connection,
    meta: &AnnIndexMetaRow,
    container_path: &PathBuf,
) -> Result<Option<LoadOutcome>, String> {
    if let Some(state) = load_legacy_index(&meta.index_path, &meta.params)? {
        return Ok(Some(LoadOutcome::new(
            update_index_path(state, container_path, true),
            true,
            true,
        )));
    }
    if meta.index_path != *container_path {
        let legacy_path = super::storage::legacy_index_path(conn)?;
        if let Some(state) = load_legacy_index(&legacy_path, &meta.params)? {
            return Ok(Some(LoadOutcome::new(
                update_index_path(state, container_path, true),
                true,
                true,
            )));
        }
    }
    Ok(None)
}

fn update_index_path(
    mut state: AnnIndexState,
    index_path: &PathBuf,
    force: bool,
) -> AnnIndexState {
    if force {
        state.index_path = index_path.clone();
    }
    state
}

fn build_loaded_state(
    hnsw: Hnsw<'static, f32, DistCosine>,
    id_map: Vec<String>,
    params: &AnnIndexParams,
    index_path: PathBuf,
) -> Result<Option<AnnIndexState>, String> {
    if hnsw.get_nb_point() != id_map.len() {
        return Ok(None);
    }
    Ok(Some(build_state(
        params.clone(),
        hnsw,
        id_map,
        index_path,
    )))
}

fn temp_unpack_dir() -> Result<tempfile::TempDir, String> {
    Builder::new()
        .prefix("ann_unpack")
        .tempdir()
        .map_err(|err| format!("Failed to create ANN temp dir: {err}"))
}
