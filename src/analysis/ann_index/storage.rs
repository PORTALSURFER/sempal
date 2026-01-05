use super::state::{AnnIndexMetaRow, AnnIndexState};
use crate::app_dirs;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::{Path, PathBuf};

const ANN_DIR: &str = "ann";
const ANN_BASENAME: &str = "similarity_hnsw";
const ANN_ID_MAP_SUFFIX: &str = "idmap.json";

pub(crate) fn read_meta(
    conn: &Connection,
    model_id: &str,
) -> Result<Option<AnnIndexMetaRow>, String> {
    let row = conn
        .query_row(
            "SELECT index_path, params_json FROM ann_index_meta WHERE model_id = ?1",
            params![model_id],
            |row| {
                let path: String = row.get(0)?;
                let params_json: String = row.get(1)?;
                Ok((path, params_json))
            },
        )
        .optional()
        .map_err(|err| format!("Failed to read ann_index_meta: {err}"))?;
    let Some((path, params_json)) = row else {
        return Ok(None);
    };
    let params: super::state::AnnIndexParams =
        serde_json::from_str(&params_json).map_err(|err| format!("{err}"))?;
    let index_path = PathBuf::from(path);
    Ok(Some(AnnIndexMetaRow { index_path, params }))
}

pub(crate) fn upsert_meta(conn: &Connection, state: &AnnIndexState) -> Result<(), String> {
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

pub(crate) fn index_key(conn: &Connection) -> Result<String, String> {
    let params = super::state::default_params();
    let meta = read_meta(conn, &params.model_id)?;
    let index_path = meta
        .map(|meta| meta.index_path)
        .unwrap_or(default_index_path(conn)?);
    Ok(index_path.to_string_lossy().to_string())
}

pub(crate) fn id_map_path_for(index_path: &Path) -> PathBuf {
    let basename = index_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(ANN_BASENAME);
    let parent = index_path.parent().unwrap_or_else(|| Path::new("."));
    parent.join(format!("{basename}.{ANN_ID_MAP_SUFFIX}"))
}

pub(crate) fn save_id_map(path: &Path, id_map: &[String]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("Failed to create ANN dir: {err}"))?;
    }
    let data = serde_json::to_vec_pretty(id_map)
        .map_err(|err| format!("Failed to encode id map: {err}"))?;
    std::fs::write(path, data).map_err(|err| format!("Failed to write id map: {err}"))?;
    Ok(())
}

pub(crate) fn load_id_map(path: &Path) -> Result<Vec<String>, String> {
    let bytes = std::fs::read(path).map_err(|err| format!("Failed to read id map: {err}"))?;
    serde_json::from_slice(&bytes).map_err(|err| format!("Failed to decode id map: {err}"))
}

pub(crate) fn default_index_path(conn: &Connection) -> Result<PathBuf, String> {
    let root = match database_root_dir(conn) {
        Ok(dir) => dir,
        Err(_) => app_dirs::app_root_dir().map_err(|err| err.to_string())?,
    };
    let dir = root.join(ANN_DIR);
    std::fs::create_dir_all(&dir).map_err(|err| format!("Failed to create ANN dir: {err}"))?;
    Ok(dir.join(ANN_BASENAME))
}

pub(crate) fn database_root_dir(conn: &Connection) -> Result<PathBuf, String> {
    let mut stmt = conn
        .prepare("PRAGMA database_list")
        .map_err(|err| format!("Failed to read database_list: {err}"))?;
    let mut rows = stmt
        .query([])
        .map_err(|err| format!("Failed to read database_list: {err}"))?;
    let Some(row) = rows
        .next()
        .map_err(|err| format!("Failed to read database_list: {err}"))?
    else {
        return Err("Missing database_list row".to_string());
    };
    let path: Option<String> = row.get(2).map_err(|err| err.to_string())?;
    let path = path.filter(|value| !value.is_empty());
    let path = path.ok_or_else(|| "Database path missing".to_string())?;
    let path = PathBuf::from(path);
    let root = path
        .parent()
        .ok_or_else(|| "Database path missing parent".to_string())?;
    Ok(root.to_path_buf())
}

pub(crate) fn hnsw_dump_paths(index_path: &Path) -> Result<(PathBuf, PathBuf), String> {
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

fn chrono_now_epoch_seconds() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
