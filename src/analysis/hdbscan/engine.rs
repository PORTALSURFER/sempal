use hdbscan::{Hdbscan, HdbscanHyperParams};
use rusqlite::types::Value;
use rusqlite::{Connection, params_from_iter};

use super::{HdbscanConfig, HdbscanMethod};

pub fn load_cluster_data(
    conn: &Connection,
    model_id: &str,
    method: HdbscanMethod,
    umap_version: Option<&str>,
    sample_id_prefix: Option<&str>,
) -> Result<(Vec<String>, Vec<Vec<f32>>), String> {
    match method {
        HdbscanMethod::Embedding => load_embeddings(conn, model_id, sample_id_prefix),
        HdbscanMethod::Umap => {
            let version = umap_version.ok_or_else(|| "Layout version required".to_string())?;
            load_umap_points(conn, model_id, version, sample_id_prefix)
        }
    }
}

pub fn run_hdbscan(data: &[Vec<f32>], config: HdbscanConfig) -> Result<Vec<i32>, String> {
    let min_required = config
        .min_samples
        .unwrap_or(1)
        .max(config.min_cluster_size)
        .max(2);
    if data.len() < min_required {
        // HDBSCAN panics on tiny datasets; treat them as a single cluster.
        return Ok(vec![0; data.len()]);
    }
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
    sample_id_prefix: Option<&str>,
) -> Result<(Vec<String>, Vec<Vec<f32>>), String> {
    let (sql, params) = if let Some(prefix) = sample_id_prefix {
        (
            "SELECT sample_id, dim, vec
             FROM embeddings
             WHERE model_id = ?1 AND sample_id LIKE ?2
             ORDER BY sample_id ASC",
            vec![
                Value::Text(model_id.to_string()),
                Value::Text(prefix.to_string()),
            ],
        )
    } else {
        (
            "SELECT sample_id, dim, vec
             FROM embeddings
             WHERE model_id = ?1
             ORDER BY sample_id ASC",
            vec![Value::Text(model_id.to_string())],
        )
    };
    let mut stmt = conn
        .prepare(sql)
        .map_err(|err| format!("Prepare embedding query failed: {err}"))?;
    let rows = stmt
        .query_map(params_from_iter(params), |row| {
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
    sample_id_prefix: Option<&str>,
) -> Result<(Vec<String>, Vec<Vec<f32>>), String> {
    let (sql, params) = if let Some(prefix) = sample_id_prefix {
        (
            "SELECT sample_id, x, y
             FROM layout_umap
             WHERE model_id = ?1 AND umap_version = ?2 AND sample_id LIKE ?3
             ORDER BY sample_id ASC",
            vec![
                Value::Text(model_id.to_string()),
                Value::Text(umap_version.to_string()),
                Value::Text(prefix.to_string()),
            ],
        )
    } else {
        (
            "SELECT sample_id, x, y
             FROM layout_umap
             WHERE model_id = ?1 AND umap_version = ?2
             ORDER BY sample_id ASC",
            vec![
                Value::Text(model_id.to_string()),
                Value::Text(umap_version.to_string()),
            ],
        )
    };
    let mut stmt = conn
        .prepare(sql)
        .map_err(|err| format!("Prepare layout query failed: {err}"))?;
    let rows = stmt
        .query_map(params_from_iter(params), |row| {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assigns_single_cluster_for_tiny_datasets() {
        let data = vec![vec![0.0, 1.0], vec![2.0, 3.0]];
        let config = HdbscanConfig {
            min_cluster_size: 5,
            min_samples: None,
            allow_single_cluster: true,
        };
        let labels = run_hdbscan(&data, config).unwrap();
        assert_eq!(labels, vec![0, 0]);
    }
}
