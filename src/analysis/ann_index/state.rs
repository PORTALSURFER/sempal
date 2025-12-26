use crate::analysis::{embedding, version};
use hnsw_rs::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct AnnIndexParams {
    pub(crate) analysis_version: String,
    pub(crate) model_id: String,
    pub(crate) metric: String,
    pub(crate) dim: usize,
    pub(crate) max_nb_connection: usize,
    pub(crate) ef_construction: usize,
    pub(crate) ef_search: usize,
    pub(crate) max_layer: usize,
}

pub(crate) struct AnnIndexState {
    pub(crate) hnsw: Hnsw<'static, f32, DistCosine>,
    pub(crate) id_map: Vec<String>,
    pub(crate) id_lookup: HashMap<String, usize>,
    pub(crate) params: AnnIndexParams,
    pub(crate) index_path: PathBuf,
    pub(crate) id_map_path: PathBuf,
    pub(crate) last_flush: Instant,
    pub(crate) dirty_inserts: usize,
}

pub(crate) struct AnnIndexMetaRow {
    pub(crate) index_path: PathBuf,
    pub(crate) params: AnnIndexParams,
}

pub(crate) fn default_params() -> AnnIndexParams {
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

pub(crate) fn build_id_lookup(id_map: &[String]) -> HashMap<String, usize> {
    let mut lookup = HashMap::with_capacity(id_map.len());
    for (idx, sample_id) in id_map.iter().enumerate() {
        lookup.insert(sample_id.clone(), idx);
    }
    lookup
}
