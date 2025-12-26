use rusqlite::OptionalExtension;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(super) fn load_embedding_vec_optional(
    conn: &rusqlite::Connection,
    sample_id: &str,
    model_id: &str,
    expected_dim: usize,
) -> Result<Option<Vec<f32>>, String> {
    let row: Option<Vec<u8>> = conn
        .query_row(
            "SELECT vec FROM embeddings WHERE sample_id = ?1 AND model_id = ?2",
            rusqlite::params![sample_id, model_id],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()
        .map_err(|err| format!("Failed to load embedding blob for {sample_id}: {err}"))?;
    let Some(blob) = row else {
        return Ok(None);
    };
    let vec = crate::analysis::decode_f32_le_blob(&blob)?;
    if vec.len() != expected_dim {
        return Ok(None);
    }
    Ok(Some(vec))
}

pub(super) fn now_epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs() as i64
}
