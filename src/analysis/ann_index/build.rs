use super::state::{AnnIndexMetaRow, AnnIndexParams, AnnIndexState, build_id_lookup, default_params};
use super::storage::{
    default_index_path, hnsw_dump_paths, id_map_path_for, load_id_map, read_meta,
};
use crate::analysis::decode_f32_le_blob;
use hnsw_rs::hnswio::HnswIo;
use hnsw_rs::prelude::*;
use rusqlite::{Connection, params};
use std::path::PathBuf;
use std::time::Instant;

pub(crate) fn load_or_build_index(conn: &Connection) -> Result<AnnIndexState, String> {
    let params = default_params();
    let meta = read_meta(conn, &params.model_id)?;
    if let Some(meta_row) = meta.as_ref() {
        if meta_row.params == params {
            if let Some(state) = load_index_from_disk(meta_row)? {
                return Ok(state);
            }
        }
    }
    let index_path = meta
        .map(|meta| meta.index_path)
        .unwrap_or(default_index_path(conn)?);
    let mut state = build_index_from_db(conn, params, index_path)?;
    super::update::flush_index(conn, &mut state)?;
    Ok(state)
}

pub(crate) fn build_index_from_db(
    conn: &Connection,
    params: AnnIndexParams,
    index_path: PathBuf,
) -> Result<AnnIndexState, String> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM embeddings WHERE model_id = ?1",
            params![params.model_id],
            |row| row.get(0),
        )
        .map_err(|err| format!("Failed to count embeddings: {err}"))?;
    let max_elements = (count.max(1) as usize).max(1024);
    let hnsw = Hnsw::new(
        params.max_nb_connection,
        max_elements,
        params.max_layer,
        params.ef_construction,
        DistCosine {},
    );
    let mut id_map = Vec::with_capacity(count.max(0) as usize);
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
    let id_map_path = id_map_path_for(&index_path);
    let id_lookup = build_id_lookup(&id_map);
    Ok(AnnIndexState {
        hnsw,
        id_map,
        id_lookup,
        params,
        index_path,
        id_map_path,
        last_flush: Instant::now(),
        dirty_inserts: 0,
    })
}

pub(crate) fn load_index_from_disk(meta: &AnnIndexMetaRow) -> Result<Option<AnnIndexState>, String> {
    let index_path = meta.index_path.clone();
    let (graph_path, data_path) = hnsw_dump_paths(&index_path)?;
    if !graph_path.is_file() || !data_path.is_file() {
        return Ok(None);
    }
    let id_map_path = id_map_path_for(&index_path);
    if !id_map_path.is_file() {
        return Ok(None);
    }
    let id_map = match load_id_map(&id_map_path) {
        Ok(id_map) => id_map,
        Err(_) => return Ok(None),
    };
    let id_lookup = build_id_lookup(&id_map);
    let basename = index_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Index path missing basename".to_string())?;
    let dir = index_path
        .parent()
        .ok_or_else(|| "Index path missing parent".to_string())?;
    let hnsw_io = Box::new(HnswIo::new(dir, basename));
    let hnsw_io = Box::leak(hnsw_io);
    let hnsw: Hnsw<f32, DistCosine> = match hnsw_io.load_hnsw::<f32, DistCosine>() {
        Ok(hnsw) => hnsw,
        Err(_) => return Ok(None),
    };
    if hnsw.get_nb_point() != id_map.len() {
        return Ok(None);
    }
    Ok(Some(AnnIndexState {
        hnsw,
        id_map,
        id_lookup,
        params: meta.params.clone(),
        index_path,
        id_map_path,
        last_flush: Instant::now(),
        dirty_inserts: 0,
    }))
}
