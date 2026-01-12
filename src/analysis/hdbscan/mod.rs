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

/// Input space to use when clustering with HDBSCAN.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HdbscanMethod {
    /// Cluster directly over similarity embeddings.
    Embedding,
    /// Cluster over 2D UMAP projections.
    Umap,
}

impl HdbscanMethod {
    /// Return the stable string identifier used in the database.
    pub fn as_str(self) -> &'static str {
        match self {
            HdbscanMethod::Embedding => "embedding",
            HdbscanMethod::Umap => "umap",
        }
    }
}

/// Parameters controlling the HDBSCAN clustering pass.
#[derive(Debug, Clone, Copy)]
pub struct HdbscanConfig {
    /// Minimum number of points required to form a cluster.
    pub min_cluster_size: usize,
    /// Optional minimum number of samples for a core point.
    pub min_samples: Option<usize>,
    /// Whether to allow a single cluster containing all points.
    pub allow_single_cluster: bool,
}

/// Summary statistics returned after clustering.
#[derive(Debug, Clone, Copy)]
pub struct HdbscanStats {
    /// Count of non-noise clusters.
    pub cluster_count: usize,
    /// Number of points labeled as noise.
    pub noise_count: usize,
    /// Ratio of noise points to total points.
    pub noise_ratio: f32,
    /// Smallest cluster size observed.
    pub min_cluster_size: usize,
    /// Largest cluster size observed.
    pub max_cluster_size: usize,
}

/// Build clusters and persist assignments for the full dataset.
pub fn build_hdbscan_clusters(
    conn: &mut Connection,
    model_id: &str,
    method: HdbscanMethod,
    umap_version: Option<&str>,
    config: HdbscanConfig,
) -> Result<HdbscanStats, String> {
    build_hdbscan_clusters_for_sample_id_prefix(conn, model_id, method, umap_version, None, config)
}

/// Build clusters for samples matching an optional ID prefix.
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
