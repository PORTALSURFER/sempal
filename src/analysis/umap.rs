use crate::analysis::decode_f32_le_blob;
use hnsw_rs::prelude::*;
use ndarray::Array2;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rusqlite::{Connection, params};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_NEIGHBORS: usize = 8;
const DEFAULT_MIN_DIST: f32 = 0.0;
const DEFAULT_N_COMPONENTS: usize = 2;

#[derive(Debug, Serialize)]
pub struct UmapReport {
    pub total: usize,
    pub valid: usize,
    pub invalid: usize,
    pub coverage_ratio: f32,
    pub x_min: f32,
    pub x_max: f32,
    pub y_min: f32,
    pub y_max: f32,
}

pub fn build_umap_layout(
    conn: &mut Connection,
    model_id: &str,
    umap_version: &str,
    seed: u64,
    min_coverage: f32,
) -> Result<UmapReport, String> {
    let (sample_ids, vectors, _dim) = load_embeddings(conn, model_id)?;
    if vectors.is_empty() {
        return Err(format!("No embeddings found for model_id {model_id}"));
    }
    let layout = compute_umap(&vectors, seed)?;
    if layout.len() != sample_ids.len() {
        return Err("UMAP output length mismatch".to_string());
    }
    let inserted = write_layout(conn, &sample_ids, &layout, model_id, umap_version)?;
    if inserted != sample_ids.len() {
        return Err("UMAP insert count mismatch".to_string());
    }
    validate_layout(&layout, min_coverage)
}

pub fn default_report_path(db_path: &PathBuf, umap_version: &str) -> PathBuf {
    let parent = db_path.parent().unwrap_or_else(|| Path::new("."));
    parent.join(format!("umap_report_{}.json", umap_version))
}

pub fn write_report(path: &PathBuf, report: &UmapReport) -> Result<(), String> {
    let data = serde_json::to_vec_pretty(report)
        .map_err(|err| format!("Serialize report failed: {err}"))?;
    std::fs::write(path, data).map_err(|err| format!("Write report failed: {err}"))?;
    Ok(())
}

fn load_embeddings(
    conn: &Connection,
    model_id: &str,
) -> Result<(Vec<String>, Vec<Vec<f32>>, usize), String> {
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
    let mut sample_ids = Vec::new();
    let mut vectors = Vec::new();
    let mut expected_dim: Option<usize> = None;
    for row in rows {
        let (sample_id, dim, blob) =
            row.map_err(|err| format!("Read embedding row failed: {err}"))?;
        let vec = decode_f32_le_blob(&blob)?;
        if vec.len() != dim {
            return Err(format!(
                "Embedding dim mismatch for {sample_id}: expected {dim}, got {}",
                vec.len()
            ));
        }
        if let Some(expected) = expected_dim {
            if dim != expected {
                return Err(format!(
                    "Embedding dim mismatch: expected {expected}, got {dim} for {sample_id}"
                ));
            }
        } else {
            expected_dim = Some(dim);
        }
        sample_ids.push(sample_id);
        vectors.push(vec);
    }
    let dim = expected_dim.unwrap_or(0);
    if dim == 0 {
        return Err("No embeddings found for model".to_string());
    }
    Ok((sample_ids, vectors, dim))
}

fn compute_umap(vectors: &[Vec<f32>], seed: u64) -> Result<Vec<[f32; 2]>, String> {
    let mut data = Vec::new();
    for vec in vectors {
        data.extend_from_slice(vec);
    }
    let n_samples = vectors.len();
    let dim = vectors.first().map(|v| v.len()).unwrap_or(0);
    if n_samples < 2 {
        return Err("Need at least 2 embeddings to build UMAP".to_string());
    }
    let n_neighbors = DEFAULT_NEIGHBORS.min(n_samples.saturating_sub(1)).max(1);
    let matrix = Array2::from_shape_vec((n_samples, dim), data)
        .map_err(|err| format!("Build embedding matrix failed: {err}"))?;
    let (knn_indices, knn_dists) =
        build_knn_graph(&matrix, n_neighbors, n_neighbors * 2)?;
    let init = random_init(n_samples, DEFAULT_N_COMPONENTS, seed);

    let mut config = umap_rs::UmapConfig::default();
    config.n_components = DEFAULT_N_COMPONENTS;
    config.graph.n_neighbors = n_neighbors;
    config.manifold.min_dist = DEFAULT_MIN_DIST;
    let umap = umap_rs::Umap::new(config.clone());
    let fitted = umap.fit(
        matrix.view(),
        knn_indices.view(),
        knn_dists.view(),
        init.view(),
    );
    let coords = fitted.embedding();
    if coords.ncols() != 2 {
        return Err(format!(
            "UMAP returned {} columns, expected 2",
            coords.ncols()
        ));
    }
    let mut out = Vec::with_capacity(n_samples);
    for row in coords.rows() {
        out.push([row[0], row[1]]);
    }
    Ok(out)
}

fn build_knn_graph(
    matrix: &Array2<f32>,
    n_neighbors: usize,
    ef_search: usize,
) -> Result<(Array2<u32>, Array2<f32>), String> {
    let n_samples = matrix.nrows();
    let max_elements = n_samples.max(1024);
    let hnsw = Hnsw::new(16, max_elements, 16, 200, DistCosine {});
    for (idx, row) in matrix.rows().into_iter().enumerate() {
        hnsw.insert((row.as_slice().ok_or_else(|| "Embedding not contiguous".to_string())?, idx));
    }

    let mut knn_indices = Array2::<u32>::zeros((n_samples, n_neighbors));
    let mut knn_dists = Array2::<f32>::zeros((n_samples, n_neighbors));
    for (row_idx, row) in matrix.rows().into_iter().enumerate() {
        let neighbours = hnsw.search(
            row.as_slice().ok_or_else(|| "Embedding not contiguous".to_string())?,
            n_neighbors + 1,
            ef_search.max(n_neighbors + 1),
        );
        let mut filled = 0usize;
        for neighbour in neighbours {
            if neighbour.d_id == row_idx {
                continue;
            }
            if filled >= n_neighbors {
                break;
            }
            knn_indices[(row_idx, filled)] = neighbour.d_id as u32;
            knn_dists[(row_idx, filled)] = neighbour.distance;
            filled += 1;
        }
        if filled < n_neighbors {
            return Err("ANN search returned insufficient neighbors".to_string());
        }
    }
    Ok((knn_indices, knn_dists))
}

fn random_init(n_samples: usize, n_components: usize, seed: u64) -> Array2<f32> {
    let mut rng = StdRng::seed_from_u64(seed);
    Array2::from_shape_fn((n_samples, n_components), |_| rng.random::<f32>() * 10.0)
}

fn validate_layout(layout: &[[f32; 2]], min_coverage: f32) -> Result<UmapReport, String> {
    let total = layout.len();
    let mut valid = 0usize;
    let mut x_min = f32::INFINITY;
    let mut x_max = f32::NEG_INFINITY;
    let mut y_min = f32::INFINITY;
    let mut y_max = f32::NEG_INFINITY;
    for coords in layout {
        let x = coords[0];
        let y = coords[1];
        if x.is_finite() && y.is_finite() {
            valid += 1;
            x_min = x_min.min(x);
            x_max = x_max.max(x);
            y_min = y_min.min(y);
            y_max = y_max.max(y);
        }
    }
    let invalid = total.saturating_sub(valid);
    let coverage_ratio = if total == 0 {
        0.0
    } else {
        valid as f32 / total as f32
    };
    if coverage_ratio < min_coverage {
        return Err(format!(
            "UMAP coverage {:.2}% below threshold {:.2}%",
            coverage_ratio * 100.0,
            min_coverage * 100.0
        ));
    }
    if valid == 0 {
        return Err("UMAP produced no valid coordinates".to_string());
    }
    Ok(UmapReport {
        total,
        valid,
        invalid,
        coverage_ratio,
        x_min,
        x_max,
        y_min,
        y_max,
    })
}

fn write_layout(
    conn: &mut Connection,
    sample_ids: &[String],
    layout: &[[f32; 2]],
    model_id: &str,
    umap_version: &str,
) -> Result<usize, String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "Invalid system time".to_string())?
        .as_secs() as i64;
    let tx = conn
        .transaction()
        .map_err(|err| format!("Start transaction failed: {err}"))?;
    let mut stmt = tx
        .prepare(
            "INSERT INTO layout_umap (sample_id, model_id, umap_version, x, y, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(sample_id) DO UPDATE SET
                model_id = excluded.model_id,
                umap_version = excluded.umap_version,
                x = excluded.x,
                y = excluded.y,
                created_at = excluded.created_at",
        )
        .map_err(|err| format!("Prepare layout insert failed: {err}"))?;
    for (idx, sample_id) in sample_ids.iter().enumerate() {
        let coords = layout
            .get(idx)
            .ok_or_else(|| "Layout length mismatch".to_string())?;
        stmt.execute(params![
            sample_id,
            model_id,
            umap_version,
            coords[0] as f64,
            coords[1] as f64,
            now
        ])
        .map_err(|err| format!("Insert layout failed: {err}"))?;
    }
    drop(stmt);
    tx.commit()
        .map_err(|err| format!("Commit layout failed: {err}"))?;
    Ok(sample_ids.len())
}
