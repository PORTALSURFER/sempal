//! HDBSCAN clustering helpers for embeddings and 2D layouts.

mod engine;
mod mapping;
mod validation;

use rusqlite::Connection;

use self::engine::load_cluster_data;
use self::mapping::{
    assign_all_points_to_clusters, remap_labels_deterministic, summarize_labels, write_clusters,
};
use self::validation::{ensure_non_empty, validate_request};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HdbscanMethod {
    Embedding,
    Umap,
}

impl HdbscanMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            HdbscanMethod::Embedding => "embedding",
            HdbscanMethod::Umap => "umap",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct HdbscanConfig {
    pub min_cluster_size: usize,
    pub min_samples: Option<usize>,
    pub allow_single_cluster: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct HdbscanStats {
    pub cluster_count: usize,
    pub noise_count: usize,
    pub noise_ratio: f32,
    pub min_cluster_size: usize,
    pub max_cluster_size: usize,
}

pub fn build_hdbscan_clusters(
    conn: &mut Connection,
    model_id: &str,
    method: HdbscanMethod,
    umap_version: Option<&str>,
    config: HdbscanConfig,
) -> Result<HdbscanStats, String> {
    build_hdbscan_clusters_for_sample_id_prefix(conn, model_id, method, umap_version, None, config)
}

pub fn build_hdbscan_clusters_for_sample_id_prefix(
    conn: &mut Connection,
    model_id: &str,
    method: HdbscanMethod,
    umap_version: Option<&str>,
    sample_id_prefix: Option<&str>,
    config: HdbscanConfig,
) -> Result<HdbscanStats, String> {
    validate_request(method, umap_version, config)?;
    let (sample_ids, data) =
        load_cluster_data(conn, model_id, method, umap_version, sample_id_prefix)?;
    ensure_non_empty(&data)?;
    let mut labels = engine::run_hdbscan(&data, config)?;
    assign_all_points_to_clusters(&data, &mut labels);
    remap_labels_deterministic(&sample_ids, &mut labels)?;
    let stats = summarize_labels(&labels);
    let version = umap_version.unwrap_or("");
    write_clusters(
        conn,
        &sample_ids,
        &labels,
        model_id,
        method.as_str(),
        version,
    )?;
    Ok(stats)
}
