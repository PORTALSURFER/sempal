use crate::analysis::{decode_f32_le_blob, embedding, version};
use crate::app_dirs;
use hnsw_rs::prelude::*;
use hnsw_rs::hnswio::HnswIo;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

const ANN_DIR: &str = "ann";
const ANN_BASENAME: &str = "clap_hnsw";
const ANN_ID_MAP_SUFFIX: &str = "idmap.json";
const ANN_FLUSH_INTERVAL: Duration = Duration::from_secs(30);
const ANN_FLUSH_MIN_INSERTS: usize = 64;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct AnnIndexParams {
    analysis_version: String,
    model_id: String,
    metric: String,
    dim: usize,
    max_nb_connection: usize,
    ef_construction: usize,
    ef_search: usize,
    max_layer: usize,
}

struct AnnIndexState {
    hnsw: Hnsw<'static, f32, DistCosine>,
    id_map: Vec<String>,
    id_lookup: HashMap<String, usize>,
    params: AnnIndexParams,
    index_path: PathBuf,
    id_map_path: PathBuf,
    last_flush: Instant,
    dirty_inserts: usize,
}

#[derive(Debug)]
pub struct SimilarNeighbor {
    pub sample_id: String,
    pub distance: f32,
}

static ANN_INDEX: LazyLock<Mutex<Option<AnnIndexState>>> = LazyLock::new(|| Mutex::new(None));

pub fn upsert_embedding(
    conn: &Connection,
    sample_id: &str,
    embedding: &[f32],
) -> Result<(), String> {
    let mut guard = ANN_INDEX
        .lock()
        .map_err(|_| "ANN index lock poisoned".to_string())?;
    if guard.is_none() {
        let state = load_or_build_index(conn)?;
        *guard = Some(state);
    }
    let Some(state) = guard.as_mut() else {
        return Ok(());
    };
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

pub fn find_similar(
    conn: &Connection,
    sample_id: &str,
    k: usize,
) -> Result<Vec<SimilarNeighbor>, String> {
    if k == 0 {
        return Ok(Vec::new());
    }
    let embedding = load_embedding(conn, sample_id)?;
    let mut guard = ANN_INDEX
        .lock()
        .map_err(|_| "ANN index lock poisoned".to_string())?;
    if guard.is_none() {
        let state = load_or_build_index(conn)?;
        *guard = Some(state);
    }
    let Some(state) = guard.as_mut() else {
        return Ok(Vec::new());
    };
    if !state.id_lookup.contains_key(sample_id) {
        let id = state.id_map.len();
        state.id_map.push(sample_id.to_string());
        state.id_lookup.insert(sample_id.to_string(), id);
        state.hnsw.insert((embedding.as_slice(), id));
        state.dirty_inserts += 1;
        maybe_flush(conn, state)?;
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
    let mut guard = ANN_INDEX
        .lock()
        .map_err(|_| "ANN index lock poisoned".to_string())?;
    if guard.is_none() {
        let state = load_or_build_index(conn)?;
        *guard = Some(state);
    }
    let Some(state) = guard.as_mut() else {
        return Ok(Vec::new());
    };
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
}

fn load_embedding(conn: &Connection, sample_id: &str) -> Result<Vec<f32>, String> {
    let blob: Vec<u8> = conn
        .query_row(
            "SELECT vec FROM embeddings WHERE sample_id = ?1 AND model_id = ?2",
            params![sample_id, embedding::EMBEDDING_MODEL_ID],
            |row| row.get(0),
        )
        .map_err(|err| format!("Failed to load embedding for {sample_id}: {err}"))?;
    decode_f32_le_blob(&blob)
}

fn load_or_build_index(conn: &Connection) -> Result<AnnIndexState, String> {
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
        .unwrap_or(default_index_path()?);
    let mut state = build_index_from_db(conn, params, index_path)?;
    flush_index(conn, &mut state)?;
    Ok(state)
}

fn default_params() -> AnnIndexParams {
    AnnIndexParams {
        analysis_version: version::analysis_version().to_string(),
        model_id: embedding::EMBEDDING_MODEL_ID.to_string(),
        metric: "cosine".to_string(),
        dim: embedding::EMBEDDING_DIM,
        max_nb_connection: 16,
        ef_construction: 200,
        ef_search: 64,
        max_layer: 16,
    }
}

struct AnnIndexMetaRow {
    index_path: PathBuf,
    params: AnnIndexParams,
    count: usize,
}

fn read_meta(conn: &Connection, model_id: &str) -> Result<Option<AnnIndexMetaRow>, String> {
    let row = conn
        .query_row(
            "SELECT index_path, params_json, count FROM ann_index_meta WHERE model_id = ?1",
            params![model_id],
            |row| {
                let path: String = row.get(0)?;
                let params_json: String = row.get(1)?;
                let count: i64 = row.get(2)?;
                Ok((path, params_json, count))
            },
        )
        .optional()
        .map_err(|err| format!("Failed to read ann_index_meta: {err}"))?;
    let Some((path, params_json, count)) = row else {
        return Ok(None);
    };
    let params: AnnIndexParams =
        serde_json::from_str(&params_json).map_err(|err| format!("{err}"))?;
    let index_path = PathBuf::from(path);
    Ok(Some(AnnIndexMetaRow {
        index_path,
        params,
        count: count.max(0) as usize,
    }))
}

fn id_map_path_for(index_path: &Path) -> PathBuf {
    let basename = index_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(ANN_BASENAME);
    let parent = index_path.parent().unwrap_or_else(|| Path::new("."));
    parent.join(format!("{basename}.{ANN_ID_MAP_SUFFIX}"))
}

fn build_index_from_db(
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

fn load_index_from_disk(meta: &AnnIndexMetaRow) -> Result<Option<AnnIndexState>, String> {
    let index_path = meta.index_path.clone();
    let (graph_path, data_path) = hnsw_dump_paths(&index_path)?;
    if !graph_path.is_file() || !data_path.is_file() {
        return Ok(None);
    }
    let id_map_path = id_map_path_for(&index_path);
    if !id_map_path.is_file() {
        return Ok(None);
    }
    let id_map = load_id_map(&id_map_path)?;
    let id_lookup = build_id_lookup(&id_map);
    let basename = index_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Index path missing basename".to_string())?;
    let dir = index_path
        .parent()
        .ok_or_else(|| "Index path missing parent".to_string())?;
    // Leak the reloader to satisfy Hnsw's lifetime requirements for reloaded data.
    let hnsw_io = Box::new(HnswIo::new(dir, basename));
    let hnsw_io = Box::leak(hnsw_io);
    let hnsw: Hnsw<f32, DistCosine> = hnsw_io
        .load_hnsw::<f32, DistCosine>()
        .map_err(|err| format!("Failed to reload ANN index: {err}"))?;
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

fn build_id_lookup(id_map: &[String]) -> HashMap<String, usize> {
    let mut lookup = HashMap::with_capacity(id_map.len());
    for (idx, sample_id) in id_map.iter().enumerate() {
        lookup.insert(sample_id.clone(), idx);
    }
    lookup
}

fn maybe_flush(conn: &Connection, state: &mut AnnIndexState) -> Result<(), String> {
    let elapsed = state.last_flush.elapsed();
    if state.dirty_inserts == 0 {
        return Ok(());
    }
    if state.dirty_inserts < ANN_FLUSH_MIN_INSERTS && elapsed < ANN_FLUSH_INTERVAL {
        return Ok(());
    }
    flush_index(conn, state)
}

fn flush_index(conn: &Connection, state: &mut AnnIndexState) -> Result<(), String> {
    if state.id_map.is_empty() {
        upsert_meta(conn, state)?;
        state.last_flush = Instant::now();
        state.dirty_inserts = 0;
        return Ok(());
    }
    let index_path = state.index_path.clone();
    let dir = index_path
        .parent()
        .ok_or_else(|| "Index path missing parent".to_string())?;
    std::fs::create_dir_all(dir)
        .map_err(|err| format!("Failed to create ANN dir: {err}"))?;
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
    state.last_flush = Instant::now();
    state.dirty_inserts = 0;
    Ok(())
}

fn upsert_meta(conn: &Connection, state: &AnnIndexState) -> Result<(), String> {
    let params_json =
        serde_json::to_string(&state.params).map_err(|err| format!("{err}"))?;
    let now = chrono_now_epoch_seconds();
    conn.execute(
        "INSERT INTO ann_index_meta (model_id, index_path, count, params_json, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(model_id) DO UPDATE SET
           index_path = excluded.index_path,
           count = excluded.count,
           params_json = excluded.params_json,
           updated_at = excluded.updated_at",
        params![
            state.params.model_id.as_str(),
            state.index_path.to_string_lossy(),
            state.id_map.len() as i64,
            params_json,
            now
        ],
    )
    .map_err(|err| format!("Failed to update ann_index_meta: {err}"))?;
    Ok(())
}

fn save_id_map(path: &Path, id_map: &[String]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("Failed to create ANN dir: {err}"))?;
    }
    let data = serde_json::to_vec_pretty(id_map)
        .map_err(|err| format!("Failed to encode id map: {err}"))?;
    std::fs::write(path, data).map_err(|err| format!("Failed to write id map: {err}"))?;
    Ok(())
}

fn load_id_map(path: &Path) -> Result<Vec<String>, String> {
    let bytes = std::fs::read(path).map_err(|err| format!("Failed to read id map: {err}"))?;
    serde_json::from_slice(&bytes).map_err(|err| format!("Failed to decode id map: {err}"))
}

fn default_index_path() -> Result<PathBuf, String> {
    let root = app_dirs::app_root_dir().map_err(|err| err.to_string())?;
    let dir = root.join(ANN_DIR);
    std::fs::create_dir_all(&dir).map_err(|err| format!("Failed to create ANN dir: {err}"))?;
    Ok(dir.join(ANN_BASENAME))
}

fn hnsw_dump_paths(index_path: &Path) -> Result<(PathBuf, PathBuf), String> {
    let basename = index_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Index path missing basename".to_string())?;
    let dir = index_path
        .parent()
        .ok_or_else(|| "Index path missing parent".to_string())?;
    let graph = dir.join(format!("{basename}.hnsw.graph"));
    let data = dir.join(format!("{basename}.hnsw.data"));
    Ok((graph, data))
}

pub fn rebuild_index(conn: &Connection) -> Result<(), String> {
    let params = default_params();
    let index_path = read_meta(conn, &params.model_id)?
        .map(|meta| meta.index_path)
        .unwrap_or(default_index_path()?);
    let mut state = build_index_from_db(conn, params, index_path)?;
    flush_index(conn, &mut state)?;
    let mut guard = ANN_INDEX
        .lock()
        .map_err(|_| "ANN index lock poisoned".to_string())?;
    *guard = Some(state);
    Ok(())
}

fn chrono_now_epoch_seconds() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
