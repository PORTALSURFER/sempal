use crate::analysis::decode_f32_le_blob;
use linfa::dataset::DatasetBase;
use linfa::traits::{Fit, Transformer};
use linfa_reduction::Pca;
use linfa_tsne::TSneParams;
use ndarray::Array2;
use rand_08::SeedableRng;
use rand_08::rngs::SmallRng;
use rusqlite::{Connection, params};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_PERPLEXITY: f64 = 35.0;
const DEFAULT_APPROX_THRESHOLD: f64 = 0.5;
const DEFAULT_MAX_ITER: usize = 1500;
const DEFAULT_N_COMPONENTS: usize = 2;
const DEFAULT_PCA_COMPONENTS: usize = 50;

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
    let layout = compute_tsne(&vectors, seed)?;
    if layout.len() != sample_ids.len() {
        return Err("t-SNE output length mismatch".to_string());
    }
    let inserted = write_layout(conn, &sample_ids, &layout, model_id, umap_version)?;
    if inserted != sample_ids.len() {
        return Err("t-SNE insert count mismatch".to_string());
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

fn compute_tsne(vectors: &[Vec<f32>], seed: u64) -> Result<Vec<[f32; 2]>, String> {
    let n_samples = vectors.len();
    let dim = vectors.first().map(|v| v.len()).unwrap_or(0);
    if n_samples < 2 {
        return Err("Need at least 2 embeddings to build t-SNE".to_string());
    }
    let max_perplexity = ((n_samples as f64) - 1.0).max(1.0) / 3.0;
    let perplexity = DEFAULT_PERPLEXITY.min(max_perplexity).max(1.0);
    let mut data = Vec::with_capacity(n_samples * dim);
    for vec in vectors {
        for value in vec {
            data.push(*value as f64);
        }
    }
    let mut matrix = Array2::from_shape_vec((n_samples, dim), data)
        .map_err(|err| format!("Build embedding matrix failed: {err}"))?;
    if dim > DEFAULT_PCA_COMPONENTS {
        let dataset = DatasetBase::from(matrix);
        let pca = Pca::params(DEFAULT_PCA_COMPONENTS)
            .fit(&dataset)
            .map_err(|err| format!("PCA fit failed: {err}"))?;
        let reduced = pca.transform(dataset);
        matrix = reduced.records;
    }
    let rng = SmallRng::seed_from_u64(seed);
    let embedding = TSneParams::embedding_size_with_rng(DEFAULT_N_COMPONENTS, rng)
        .perplexity(perplexity)
        .approx_threshold(DEFAULT_APPROX_THRESHOLD)
        .max_iter(DEFAULT_MAX_ITER)
        .transform(matrix)
        .map_err(|err| format!("t-SNE failed: {err}"))?;
    let mut out = Vec::with_capacity(n_samples);
    for row in embedding.rows() {
        out.push([row[0] as f32, row[1] as f32]);
    }
    Ok(out)
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
            "t-SNE coverage {:.2}% below threshold {:.2}%",
            coverage_ratio * 100.0,
            min_coverage * 100.0
        ));
    }
    if valid == 0 {
        return Err("t-SNE produced no valid coordinates".to_string());
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
