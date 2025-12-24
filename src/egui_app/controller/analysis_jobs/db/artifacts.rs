use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

pub(in crate::egui_app::controller::analysis_jobs) struct CachedFeatures {
    pub(in crate::egui_app::controller::analysis_jobs) feat_version: i64,
    pub(in crate::egui_app::controller::analysis_jobs) vec_blob: Vec<u8>,
    pub(in crate::egui_app::controller::analysis_jobs) computed_at: i64,
    pub(in crate::egui_app::controller::analysis_jobs) duration_seconds: f32,
    pub(in crate::egui_app::controller::analysis_jobs) sr_used: u32,
}

pub(in crate::egui_app::controller::analysis_jobs) struct CachedEmbedding {
    pub(in crate::egui_app::controller::analysis_jobs) analysis_version: String,
    pub(in crate::egui_app::controller::analysis_jobs) model_id: String,
    pub(in crate::egui_app::controller::analysis_jobs) dim: i64,
    pub(in crate::egui_app::controller::analysis_jobs) dtype: String,
    pub(in crate::egui_app::controller::analysis_jobs) l2_normed: bool,
    pub(in crate::egui_app::controller::analysis_jobs) vec_blob: Vec<u8>,
    pub(in crate::egui_app::controller::analysis_jobs) created_at: i64,
}

pub(in crate::egui_app::controller::analysis_jobs) fn invalidate_analysis_artifacts(
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

pub(in crate::egui_app::controller::analysis_jobs) fn update_analysis_metadata(
    conn: &Connection,
    sample_id: &str,
    content_hash: Option<&str>,
    duration_seconds: f32,
    sr_used: u32,
    analysis_version: &str,
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
                analysis_version
            ],
        )
        .map_err(|err| format!("Failed to update analysis metadata: {err}"))?;
    if updated == 0 {
        return Err(format!("No sample row updated for sample_id={sample_id}"));
    }
    Ok(())
}

pub(in crate::egui_app::controller::analysis_jobs) fn upsert_analysis_features(
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

pub(in crate::egui_app::controller::analysis_jobs) fn upsert_embedding(
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
        params![
            sample_id, model_id, dim, dtype, l2_normed, vec_blob, created_at
        ],
    )
    .map_err(|err| format!("Failed to upsert embedding: {err}"))?;
    Ok(())
}

pub(in crate::egui_app::controller::analysis_jobs) fn cached_features_by_hash(
    conn: &Connection,
    content_hash: &str,
    analysis_version: &str,
    feat_version: i64,
) -> Result<Option<CachedFeatures>, String> {
    conn.query_row(
        "SELECT feat_version, vec_blob, computed_at, duration_seconds, sr_used
         FROM analysis_cache_features
         WHERE content_hash = ?1 AND analysis_version = ?2 AND feat_version = ?3",
        params![content_hash, analysis_version, feat_version],
        |row| {
            Ok(CachedFeatures {
                feat_version: row.get(0)?,
                vec_blob: row.get(1)?,
                computed_at: row.get(2)?,
                duration_seconds: row.get::<_, f64>(3)? as f32,
                sr_used: row.get::<_, i64>(4)? as u32,
            })
        },
    )
    .optional()
    .map_err(|err| format!("Failed to load cached features for {content_hash}: {err}"))
}

pub(in crate::egui_app::controller::analysis_jobs) fn cached_embedding_by_hash(
    conn: &Connection,
    content_hash: &str,
    analysis_version: &str,
    model_id: &str,
) -> Result<Option<CachedEmbedding>, String> {
    conn.query_row(
        "SELECT analysis_version, model_id, dim, dtype, l2_normed, vec, created_at
         FROM analysis_cache_embeddings
         WHERE content_hash = ?1 AND analysis_version = ?2 AND model_id = ?3",
        params![content_hash, analysis_version, model_id],
        |row| {
            Ok(CachedEmbedding {
                analysis_version: row.get(0)?,
                model_id: row.get(1)?,
                dim: row.get(2)?,
                dtype: row.get(3)?,
                l2_normed: row.get::<_, i64>(4)? != 0,
                vec_blob: row.get(5)?,
                created_at: row.get(6)?,
            })
        },
    )
    .optional()
    .map_err(|err| format!("Failed to load cached embedding for {content_hash}: {err}"))
}

pub(in crate::egui_app::controller::analysis_jobs) fn upsert_cached_features(
    conn: &Connection,
    content_hash: &str,
    analysis_version: &str,
    feat_version: i64,
    vec_blob: &[u8],
    computed_at: i64,
    duration_seconds: f32,
    sr_used: u32,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO analysis_cache_features
            (content_hash, analysis_version, feat_version, vec_blob, computed_at, duration_seconds, sr_used)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(content_hash) DO UPDATE SET
            analysis_version = excluded.analysis_version,
            feat_version = excluded.feat_version,
            vec_blob = excluded.vec_blob,
            computed_at = excluded.computed_at,
            duration_seconds = excluded.duration_seconds,
            sr_used = excluded.sr_used",
        params![
            content_hash,
            analysis_version,
            feat_version,
            vec_blob,
            computed_at,
            duration_seconds as f64,
            sr_used as i64
        ],
    )
    .map_err(|err| format!("Failed to upsert cached features: {err}"))?;
    Ok(())
}

pub(in crate::egui_app::controller::analysis_jobs) fn upsert_cached_embedding(
    conn: &Connection,
    content_hash: &str,
    analysis_version: &str,
    model_id: &str,
    dim: i64,
    dtype: &str,
    l2_normed: bool,
    vec_blob: &[u8],
    created_at: i64,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO analysis_cache_embeddings
            (content_hash, analysis_version, model_id, dim, dtype, l2_normed, vec, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(content_hash, model_id) DO UPDATE SET
            analysis_version = excluded.analysis_version,
            dim = excluded.dim,
            dtype = excluded.dtype,
            l2_normed = excluded.l2_normed,
            vec = excluded.vec,
            created_at = excluded.created_at",
        params![
            content_hash,
            analysis_version,
            model_id,
            dim,
            dtype,
            l2_normed,
            vec_blob,
            created_at
        ],
    )
    .map_err(|err| format!("Failed to upsert cached embedding: {err}"))?;
    Ok(())
}
