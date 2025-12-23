use rusqlite::{Connection, TransactionBehavior, params};

pub(super) fn invalidate_analysis_artifacts(
    conn: &mut Connection,
    sample_ids: &[String],
) -> Result<(), String> {
    if sample_ids.is_empty() {
        return Ok(());
    }
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|err| format!("Failed to start analysis invalidation transaction: {err}"))?;
    let mut stmt_features = tx
        .prepare("DELETE FROM features WHERE sample_id = ?1")
        .map_err(|err| format!("Failed to prepare analysis invalidation statement: {err}"))?;
    let mut stmt_embeddings = tx
        .prepare("DELETE FROM embeddings WHERE sample_id = ?1")
        .map_err(|err| format!("Failed to prepare analysis invalidation statement: {err}"))?;
    let mut stmt_legacy_features = tx
        .prepare("DELETE FROM analysis_features WHERE sample_id = ?1")
        .map_err(|err| format!("Failed to prepare analysis invalidation statement: {err}"))?;
    for sample_id in sample_ids {
        stmt_features
            .execute(params![sample_id])
            .map_err(|err| format!("Failed to invalidate analysis features: {err}"))?;
        stmt_embeddings
            .execute(params![sample_id])
            .map_err(|err| format!("Failed to invalidate embeddings: {err}"))?;
        stmt_legacy_features
            .execute(params![sample_id])
            .map_err(|err| format!("Failed to invalidate analysis features: {err}"))?;
    }
    drop(stmt_features);
    drop(stmt_embeddings);
    drop(stmt_legacy_features);
    tx.commit()
        .map_err(|err| format!("Failed to commit analysis invalidation transaction: {err}"))?;
    Ok(())
}

pub(super) fn update_analysis_metadata(
    conn: &Connection,
    sample_id: &str,
    content_hash: Option<&str>,
    duration_seconds: f32,
    sr_used: u32,
) -> Result<(), String> {
    let updated = conn
        .execute(
            "UPDATE samples
             SET duration_seconds = ?3, sr_used = ?4, analysis_version = ?5
             WHERE sample_id = ?1 AND content_hash = COALESCE(?2, content_hash)",
            params![
                sample_id,
                content_hash,
                duration_seconds as f64,
                sr_used as i64,
                crate::analysis::version::analysis_version()
            ],
        )
        .map_err(|err| format!("Failed to update analysis metadata: {err}"))?;
    if updated == 0 {
        return Err(format!("No sample row updated for sample_id={sample_id}"));
    }
    Ok(())
}

pub(super) fn upsert_analysis_features(
    conn: &Connection,
    sample_id: &str,
    vec_blob: &[u8],
    feat_version: i64,
    computed_at: i64,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO features (sample_id, feat_version, vec_blob, computed_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(sample_id) DO UPDATE SET
            feat_version = excluded.feat_version,
            vec_blob = excluded.vec_blob,
            computed_at = excluded.computed_at",
        params![sample_id, feat_version, vec_blob, computed_at],
    )
    .map_err(|err| format!("Failed to upsert analysis features: {err}"))?;
    Ok(())
}

pub(super) fn upsert_embedding(
    conn: &Connection,
    sample_id: &str,
    model_id: &str,
    dim: i64,
    dtype: &str,
    l2_normed: bool,
    vec_blob: &[u8],
    created_at: i64,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO embeddings (sample_id, model_id, dim, dtype, l2_normed, vec, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(sample_id) DO UPDATE SET
            model_id = excluded.model_id,
            dim = excluded.dim,
            dtype = excluded.dtype,
            l2_normed = excluded.l2_normed,
            vec = excluded.vec,
            created_at = excluded.created_at",
        params![sample_id, model_id, dim, dtype, l2_normed, vec_blob, created_at],
    )
    .map_err(|err| format!("Failed to upsert embedding: {err}"))?;
    Ok(())
}
