//! HDBSCAN clustering helpers for embeddings and 2D layouts.

use hdbscan::{Hdbscan, HdbscanHyperParams};
use rusqlite::{Connection, Transaction, params};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

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
    let (sample_ids, data) = load_cluster_data(conn, model_id, method, umap_version)?;
    ensure_non_empty(&data)?;
    let labels = run_hdbscan(&data, config)?;
    let stats = summarize_labels(&labels);
    let version = umap_version.unwrap_or("");
    write_clusters(conn, &sample_ids, &labels, model_id, method.as_str(), version)?;
    Ok(stats)
}

fn load_cluster_data(
    conn: &Connection,
    model_id: &str,
    method: HdbscanMethod,
    umap_version: Option<&str>,
) -> Result<(Vec<String>, Vec<Vec<f32>>), String> {
    match method {
        HdbscanMethod::Embedding => load_embeddings(conn, model_id),
        HdbscanMethod::Umap => {
            let version = umap_version.ok_or_else(|| "Layout version required".to_string())?;
            load_umap_points(conn, model_id, version)
        }
    }
}

fn ensure_non_empty(data: &[Vec<f32>]) -> Result<(), String> {
    if data.is_empty() {
        Err("No data points found for clustering".to_string())
    } else {
        Ok(())
    }
}

fn run_hdbscan(data: &[Vec<f32>], config: HdbscanConfig) -> Result<Vec<i32>, String> {
    let hyper = build_hyperparams(config);
    let clusterer = Hdbscan::new(data, hyper);
    clusterer
        .cluster()
        .map_err(|err| format!("HDBSCAN clustering failed: {err}"))
}

fn build_hyperparams(config: HdbscanConfig) -> HdbscanHyperParams {
    let mut builder = HdbscanHyperParams::builder().min_cluster_size(config.min_cluster_size);
    if let Some(min_samples) = config.min_samples {
        builder = builder.min_samples(min_samples);
    }
    if config.allow_single_cluster {
        builder = builder.allow_single_cluster(true);
    }
    builder.build()
}

fn load_embeddings(
    conn: &Connection,
    model_id: &str,
) -> Result<(Vec<String>, Vec<Vec<f32>>), String> {
    let mut stmt = conn
        .prepare(
            "SELECT sample_id, dim, vec
             FROM embeddings
             WHERE model_id = ?1
             ORDER BY sample_id ASC",
        )
        .map_err(|err| format!("Prepare embedding query failed: {err}"))?;
    let rows = stmt
        .query_map(params![model_id], |row| {
            let sample_id: String = row.get(0)?;
            let dim: i64 = row.get(1)?;
            let blob: Vec<u8> = row.get(2)?;
            Ok((sample_id, dim as usize, blob))
        })
        .map_err(|err| format!("Query embeddings failed: {err}"))?;
    decode_embedding_rows(rows)
}

fn decode_embedding_rows<I>(rows: I) -> Result<(Vec<String>, Vec<Vec<f32>>), String>
where
    I: Iterator<Item = Result<(String, usize, Vec<u8>), rusqlite::Error>>,
{
    let mut sample_ids = Vec::new();
    let mut data = Vec::new();
    let mut expected_dim: Option<usize> = None;
    for row in rows {
        let (sample_id, dim, blob) =
            row.map_err(|err| format!("Read embedding row failed: {err}"))?;
        let vec = crate::analysis::decode_f32_le_blob(&blob)?;
        validate_embedding_dim(&sample_id, dim, vec.len(), expected_dim)?;
        expected_dim = Some(dim);
        sample_ids.push(sample_id);
        data.push(vec);
    }
    Ok((sample_ids, data))
}

fn validate_embedding_dim(
    sample_id: &str,
    expected: usize,
    actual: usize,
    previous: Option<usize>,
) -> Result<(), String> {
    if actual != expected {
        return Err(format!(
            "Embedding dim mismatch for {sample_id}: expected {expected}, got {actual}"
        ));
    }
    if let Some(prev) = previous {
        if expected != prev {
            return Err(format!(
                "Embedding dim mismatch: expected {prev}, got {expected} for {sample_id}"
            ));
        }
    }
    Ok(())
}

fn load_umap_points(
    conn: &Connection,
    model_id: &str,
    umap_version: &str,
) -> Result<(Vec<String>, Vec<Vec<f32>>), String> {
    let mut stmt = conn
        .prepare(
            "SELECT sample_id, x, y
             FROM layout_umap
             WHERE model_id = ?1 AND umap_version = ?2
             ORDER BY sample_id ASC",
        )
        .map_err(|err| format!("Prepare layout query failed: {err}"))?;
    let rows = stmt
        .query_map(params![model_id, umap_version], |row| {
            let sample_id: String = row.get(0)?;
            let x: f64 = row.get(1)?;
            let y: f64 = row.get(2)?;
            Ok((sample_id, x as f32, y as f32))
        })
        .map_err(|err| format!("Query layout failed: {err}"))?;
    decode_umap_rows(rows)
}

fn decode_umap_rows<I>(rows: I) -> Result<(Vec<String>, Vec<Vec<f32>>), String>
where
    I: Iterator<Item = Result<(String, f32, f32), rusqlite::Error>>,
{
    let mut sample_ids = Vec::new();
    let mut data = Vec::new();
    for row in rows {
        let (sample_id, x, y) = row.map_err(|err| format!("Read layout row failed: {err}"))?;
        sample_ids.push(sample_id);
        data.push(vec![x, y]);
    }
    Ok((sample_ids, data))
}

fn summarize_labels(labels: &[i32]) -> HdbscanStats {
    let mut cluster_counts: HashMap<i32, usize> = HashMap::new();
    let mut noise = 0usize;
    for label in labels {
        if *label < 0 {
            noise += 1;
        } else {
            *cluster_counts.entry(*label).or_insert(0) += 1;
        }
    }
    let total = labels.len().max(1) as f32;
    let (min_cluster_size, max_cluster_size) = min_max_cluster_size(&cluster_counts);
    HdbscanStats {
        cluster_count: cluster_counts.len(),
        noise_count: noise,
        noise_ratio: noise as f32 / total,
        min_cluster_size,
        max_cluster_size,
    }
}

fn min_max_cluster_size(cluster_counts: &HashMap<i32, usize>) -> (usize, usize) {
    if cluster_counts.is_empty() {
        return (0, 0);
    }
    let mut min_size = usize::MAX;
    let mut max_size = 0usize;
    for size in cluster_counts.values() {
        min_size = min_size.min(*size);
        max_size = max_size.max(*size);
    }
    (min_size, max_size)
}

fn write_clusters(
    conn: &mut Connection,
    sample_ids: &[String],
    labels: &[i32],
    model_id: &str,
    method: &str,
    umap_version: &str,
) -> Result<(), String> {
    let now = now_epoch_seconds()?;
    let tx = start_cluster_tx(conn)?;
    {
        let mut stmt = prepare_cluster_insert(&tx)?;
        insert_cluster_rows(
            &mut stmt,
            sample_ids,
            labels,
            model_id,
            method,
            umap_version,
            now,
        )?;
    }
    tx.commit()
        .map_err(|err| format!("Commit clusters failed: {err}"))?;
    Ok(())
}

fn start_cluster_tx(conn: &mut Connection) -> Result<Transaction<'_>, String> {
    conn.transaction()
        .map_err(|err| format!("Start transaction failed: {err}"))
}

fn prepare_cluster_insert<'a>(
    tx: &'a Transaction<'a>,
) -> Result<rusqlite::Statement<'a>, String> {
    tx.prepare(
        "INSERT INTO hdbscan_clusters (
            sample_id,
            model_id,
            method,
            umap_version,
            cluster_id,
            created_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        ON CONFLICT(sample_id, model_id, method, umap_version) DO UPDATE SET
            cluster_id = excluded.cluster_id,
            created_at = excluded.created_at",
    )
    .map_err(|err| format!("Prepare cluster insert failed: {err}"))
}

fn insert_cluster_rows(
    stmt: &mut rusqlite::Statement<'_>,
    sample_ids: &[String],
    labels: &[i32],
    model_id: &str,
    method: &str,
    umap_version: &str,
    now: i64,
) -> Result<(), String> {
    for (idx, sample_id) in sample_ids.iter().enumerate() {
        let label = labels
            .get(idx)
            .ok_or_else(|| "Cluster label length mismatch".to_string())?;
        stmt.execute(params![
            sample_id,
            model_id,
            method,
            umap_version,
            label,
            now
        ])
        .map_err(|err| format!("Insert cluster failed: {err}"))?;
    }
    Ok(())
}

fn now_epoch_seconds() -> Result<i64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "Invalid system time".to_string())
        .map(|time| time.as_secs() as i64)
}
