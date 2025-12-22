//! Developer utility to build HDBSCAN clusters from embeddings or UMAP coordinates.

use hdbscan::{Hdbscan, HdbscanHyperParams};
use rusqlite::{Connection, params};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let Some(options) = parse_args(std::env::args().skip(1).collect())? else {
        return Ok(());
    };
    let db_path = resolve_db_path(options.db_path.as_ref())?;
    let conn = Connection::open(&db_path).map_err(|err| format!("Open DB failed: {err}"))?;
    let (sample_ids, data) = match options.method {
        ClusterMethod::Embedding => load_embeddings(&conn, &options.model_id)?,
        ClusterMethod::Umap => {
            let version = options
                .umap_version
                .as_ref()
                .ok_or_else(|| "--umap-version is required when method=umap".to_string())?;
            load_umap_points(&conn, &options.model_id, version)?
        }
    };

    if data.is_empty() {
        return Err("No data points found for clustering".to_string());
    }

    println!(
        "Loaded {} samples for HDBSCAN (method={})",
        data.len(),
        options.method.as_str()
    );

    let mut builder = HdbscanHyperParams::builder().min_cluster_size(options.min_cluster_size);
    if let Some(min_samples) = options.min_samples {
        builder = builder.min_samples(min_samples);
    }
    if options.allow_single_cluster {
        builder = builder.allow_single_cluster(true);
    }
    let hyper_params = builder.build();
    let clusterer = Hdbscan::new(&data, hyper_params);
    let labels = clusterer
        .cluster()
        .map_err(|err| format!("HDBSCAN clustering failed: {err}"))?;
    if labels.len() != sample_ids.len() {
        return Err("HDBSCAN output length mismatch".to_string());
    }

    let stats = summarize_labels(&labels);
    println!(
        "Clusters: {} (noise: {} / {:.2}%, size min/max: {}/{})",
        stats.cluster_count,
        stats.noise_count,
        stats.noise_ratio * 100.0,
        stats.min_cluster_size,
        stats.max_cluster_size
    );
    options
        .noise_policy
        .handle_ratio(stats.noise_ratio, options.min_noise_ratio, options.max_noise_ratio)?;

    let mut conn = conn;
    let umap_version = options
        .umap_version
        .as_ref()
        .map(String::as_str)
        .unwrap_or("");
    let inserted = write_clusters(
        &mut conn,
        &sample_ids,
        &labels,
        &options.model_id,
        options.method.as_str(),
        umap_version,
    )?;
    println!(
        "Wrote {} cluster assignments for method={}",
        inserted,
        options.method.as_str()
    );
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum ClusterMethod {
    Embedding,
    Umap,
}

impl ClusterMethod {
    fn as_str(&self) -> &'static str {
        match self {
            ClusterMethod::Embedding => "embedding",
            ClusterMethod::Umap => "umap",
        }
    }
}

#[derive(Debug, Clone)]
struct Options {
    db_path: Option<PathBuf>,
    model_id: String,
    method: ClusterMethod,
    umap_version: Option<String>,
    min_cluster_size: usize,
    min_samples: Option<usize>,
    allow_single_cluster: bool,
    min_noise_ratio: f32,
    max_noise_ratio: f32,
    noise_policy: NoisePolicy,
}

fn parse_args(args: Vec<String>) -> Result<Option<Options>, String> {
    let mut options = Options {
        db_path: None,
        model_id: String::new(),
        method: ClusterMethod::Embedding,
        umap_version: None,
        min_cluster_size: 5,
        min_samples: None,
        allow_single_cluster: false,
        min_noise_ratio: 0.0,
        max_noise_ratio: 0.95,
        noise_policy: NoisePolicy::Warn,
    };

    let mut idx = 0usize;
    while idx < args.len() {
        match args[idx].as_str() {
            "-h" | "--help" => {
                println!("{}", help_text());
                return Ok(None);
            }
            "--db" => {
                idx += 1;
                let value = args.get(idx).ok_or_else(|| "--db requires a value".to_string())?;
                options.db_path = Some(PathBuf::from(value));
            }
            "--model-id" => {
                idx += 1;
                let value =
                    args.get(idx).ok_or_else(|| "--model-id requires a value".to_string())?;
                options.model_id = value.to_string();
            }
            "--method" => {
                idx += 1;
                let value = args
                    .get(idx)
                    .ok_or_else(|| "--method requires a value".to_string())?;
                options.method = match value.as_str() {
                    "embedding" => ClusterMethod::Embedding,
                    "umap" => ClusterMethod::Umap,
                    _ => {
                        return Err(format!(
                            "Invalid --method {value}. Use embedding or umap."
                        ))
                    }
                };
            }
            "--umap-version" => {
                idx += 1;
                let value =
                    args.get(idx).ok_or_else(|| "--umap-version requires a value".to_string())?;
                options.umap_version = Some(value.to_string());
            }
            "--min-cluster-size" => {
                idx += 1;
                let value = args
                    .get(idx)
                    .ok_or_else(|| "--min-cluster-size requires a value".to_string())?;
                options.min_cluster_size = value
                    .parse::<usize>()
                    .map_err(|_| format!("Invalid --min-cluster-size value: {value}"))?;
            }
            "--min-samples" => {
                idx += 1;
                let value =
                    args.get(idx).ok_or_else(|| "--min-samples requires a value".to_string())?;
                options.min_samples = Some(
                    value
                        .parse::<usize>()
                        .map_err(|_| format!("Invalid --min-samples value: {value}"))?,
                );
            }
            "--allow-single-cluster" => {
                options.allow_single_cluster = true;
            }
            "--min-noise" => {
                idx += 1;
                let value =
                    args.get(idx).ok_or_else(|| "--min-noise requires a value".to_string())?;
                options.min_noise_ratio = value
                    .parse::<f32>()
                    .map_err(|_| format!("Invalid --min-noise value: {value}"))?;
            }
            "--max-noise" => {
                idx += 1;
                let value =
                    args.get(idx).ok_or_else(|| "--max-noise requires a value".to_string())?;
                options.max_noise_ratio = value
                    .parse::<f32>()
                    .map_err(|_| format!("Invalid --max-noise value: {value}"))?;
            }
            "--noise-policy" => {
                idx += 1;
                let value = args
                    .get(idx)
                    .ok_or_else(|| "--noise-policy requires a value".to_string())?;
                options.noise_policy = match value.as_str() {
                    "warn" => NoisePolicy::Warn,
                    "error" => NoisePolicy::Error,
                    _ => {
                        return Err(format!(
                            "Invalid --noise-policy {value}. Use warn or error."
                        ))
                    }
                };
            }
            unknown => {
                return Err(format!("Unknown argument: {unknown}\n\n{}", help_text()));
            }
        }
        idx += 1;
    }

    if options.model_id.trim().is_empty() {
        return Err("--model-id is required".to_string());
    }
    if !(0.0..=1.0).contains(&options.min_noise_ratio) {
        return Err("--min-noise must be between 0.0 and 1.0".to_string());
    }
    if !(0.0..=1.0).contains(&options.max_noise_ratio) {
        return Err("--max-noise must be between 0.0 and 1.0".to_string());
    }
    if options.min_noise_ratio > options.max_noise_ratio {
        return Err("--min-noise must be <= --max-noise".to_string());
    }

    Ok(Some(options))
}

fn help_text() -> String {
    [
        "sempal-hdbscan",
        "",
        "Build HDBSCAN clusters for embeddings or UMAP coordinates.",
        "",
        "Usage:",
        "  sempal-hdbscan --model-id <id> --method <embedding|umap> [options]",
        "",
        "Options:",
        "  --db <path>              Path to library.db (defaults to app data location).",
        "  --model-id <id>          Embedding model id to read (required).",
        "  --method <embedding|umap> Source for clustering (required).",
        "  --umap-version <v>       UMAP layout version (required for method=umap).",
        "  --min-cluster-size <n>   Minimum cluster size (default: 5).",
        "  --min-samples <n>        Min samples for core distance (default: min_cluster_size).",
        "  --allow-single-cluster   Allow a single giant cluster.",
        "  --min-noise <f>          Warn/error if noise ratio below (default: 0.0).",
        "  --max-noise <f>          Warn/error if noise ratio above (default: 0.95).",
        "  --noise-policy <mode>    warn or error (default: warn).",
    ]
    .join("\n")
}

fn resolve_db_path(db_path: Option<&PathBuf>) -> Result<PathBuf, String> {
    if let Some(path) = db_path {
        return Ok(path.clone());
    }
    let root = sempal::app_dirs::app_root_dir().map_err(|err| err.to_string())?;
    Ok(root.join(sempal::sample_sources::library::LIBRARY_DB_FILE_NAME))
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
    let mut sample_ids = Vec::new();
    let mut data = Vec::new();
    let mut expected_dim: Option<usize> = None;
    for row in rows {
        let (sample_id, dim, blob) =
            row.map_err(|err| format!("Read embedding row failed: {err}"))?;
        let vec = sempal::analysis::decode_f32_le_blob(&blob)?;
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
        data.push(vec);
    }
    Ok((sample_ids, data))
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
        .map_err(|err| format!("Prepare UMAP query failed: {err}"))?;
    let rows = stmt
        .query_map(params![model_id, umap_version], |row| {
            let sample_id: String = row.get(0)?;
            let x: f64 = row.get(1)?;
            let y: f64 = row.get(2)?;
            Ok((sample_id, x as f32, y as f32))
        })
        .map_err(|err| format!("Query UMAP layout failed: {err}"))?;
    let mut sample_ids = Vec::new();
    let mut data = Vec::new();
    for row in rows {
        let (sample_id, x, y) = row.map_err(|err| format!("Read UMAP row failed: {err}"))?;
        sample_ids.push(sample_id);
        data.push(vec![x, y]);
    }
    Ok((sample_ids, data))
}

struct LabelStats {
    cluster_count: usize,
    noise_count: usize,
    noise_ratio: f32,
    min_cluster_size: usize,
    max_cluster_size: usize,
}

fn summarize_labels(labels: &[i32]) -> LabelStats {
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
    let (min_cluster_size, max_cluster_size) = if cluster_counts.is_empty() {
        (0, 0)
    } else {
        let mut min_size = usize::MAX;
        let mut max_size = 0usize;
        for size in cluster_counts.values() {
            min_size = min_size.min(*size);
            max_size = max_size.max(*size);
        }
        (min_size, max_size)
    };
    LabelStats {
        cluster_count: cluster_counts.len(),
        noise_count: noise,
        noise_ratio: noise as f32 / total,
        min_cluster_size,
        max_cluster_size,
    }
}

#[derive(Debug, Clone, Copy)]
enum NoisePolicy {
    Warn,
    Error,
}

impl NoisePolicy {
    fn handle_ratio(&self, ratio: f32, min: f32, max: f32) -> Result<(), String> {
        if ratio < min || ratio > max {
            let message = format!(
                "Noise ratio {:.2}% outside [{:.2}%, {:.2}%]",
                ratio * 100.0,
                min * 100.0,
                max * 100.0
            );
            return match self {
                NoisePolicy::Warn => {
                    eprintln!("Warning: {message}");
                    Ok(())
                }
                NoisePolicy::Error => Err(message),
            };
        }
        Ok(())
    }
}

fn write_clusters(
    conn: &mut Connection,
    sample_ids: &[String],
    labels: &[i32],
    model_id: &str,
    method: &str,
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
        .map_err(|err| format!("Prepare cluster insert failed: {err}"))?;
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
    drop(stmt);
    tx.commit()
        .map_err(|err| format!("Commit clusters failed: {err}"))?;
    Ok(sample_ids.len())
}
