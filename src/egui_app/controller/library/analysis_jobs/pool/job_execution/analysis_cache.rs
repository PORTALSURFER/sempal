use crate::egui_app::controller::library::analysis_jobs::db;

use super::support::load_embedding_vec_optional;

pub(crate) struct CacheLookup {
    pub(crate) features: Option<db::CachedFeatures>,
    pub(crate) embedding: Option<db::CachedEmbedding>,
    pub(crate) embedding_vec: Option<Vec<f32>>,
}

pub(crate) fn lookup_cache_by_hash(
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
        crate::analysis::similarity::SIMILARITY_MODEL_ID,
    )?;
    let embedding_vec = embedding
        .as_ref()
        .and_then(|embedding| crate::analysis::decode_f32_le_blob(&embedding.vec_blob).ok())
        .filter(|vec| vec.len() == crate::analysis::similarity::SIMILARITY_DIM);
    Ok(CacheLookup {
        features,
        embedding,
        embedding_vec,
    })
}

pub(crate) fn load_existing_embedding(
    conn: &rusqlite::Connection,
    sample_id: &str,
) -> Result<Option<Vec<f32>>, String> {
    load_embedding_vec_optional(
        conn,
        sample_id,
        crate::analysis::similarity::SIMILARITY_MODEL_ID,
        crate::analysis::similarity::SIMILARITY_DIM,
    )
}
