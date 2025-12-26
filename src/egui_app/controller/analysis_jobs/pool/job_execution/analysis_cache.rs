use crate::egui_app::controller::analysis_jobs::db;

use super::support::load_embedding_vec_optional;

pub(super) struct CacheLookup {
    pub(super) features: Option<db::CachedFeatures>,
    pub(super) embedding: Option<db::CachedEmbedding>,
    pub(super) embedding_vec: Option<Vec<f32>>,
}

pub(super) fn lookup_cache_by_hash(
    conn: &rusqlite::Connection,
    content_hash: &str,
    analysis_version: &str,
) -> Result<CacheLookup, String> {
    let features = db::cached_features_by_hash(
        conn,
        content_hash,
        analysis_version,
        crate::analysis::vector::FEATURE_VERSION_V1,
    )?;
    let embedding = db::cached_embedding_by_hash(
        conn,
        content_hash,
        analysis_version,
        crate::analysis::embedding::EMBEDDING_MODEL_ID,
    )?;
    let embedding_vec = embedding
        .as_ref()
        .and_then(|embedding| crate::analysis::decode_f32_le_blob(&embedding.vec_blob).ok())
        .filter(|vec| vec.len() == crate::analysis::embedding::EMBEDDING_DIM);
    Ok(CacheLookup {
        features,
        embedding,
        embedding_vec,
    })
}

pub(super) fn load_existing_embedding(
    conn: &rusqlite::Connection,
    sample_id: &str,
) -> Result<Option<Vec<f32>>, String> {
    load_embedding_vec_optional(
        conn,
        sample_id,
        crate::analysis::embedding::EMBEDDING_MODEL_ID,
        crate::analysis::embedding::EMBEDDING_DIM,
    )
}
