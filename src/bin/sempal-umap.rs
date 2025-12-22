//! Developer utility to build a UMAP layout from stored embeddings.

use rusqlite::{Connection, params};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_NEIGHBORS: usize = 15;
const DEFAULT_MIN_DIST: f32 = 0.1;
const DEFAULT_N_COMPONENTS: usize = 2;

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
    let (sample_ids, vectors, dim) = load_embeddings(&conn, &options.model_id)?;
    if vectors.is_empty() {
        return Err(format!(
            "No embeddings found for model_id {}",
            options.model_id
        ));
    }
    println!(
        "Loaded {} embeddings (dim={}) from {}",
        vectors.len(),
        dim,
        db_path.display()
    );
    let layout = compute_umap(&vectors, options.seed)?;
    if layout.len() != sample_ids.len() {
        return Err("UMAP output length mismatch".to_string());
    }
    let inserted = write_layout(
        &conn,
        &sample_ids,
        &layout,
        &options.model_id,
        &options.umap_version,
    )?;
    println!(
        "Wrote {} layout rows for umap_version {}",
        inserted, options.umap_version
    );
    Ok(())
}

#[derive(Debug, Clone)]
struct Options {
    db_path: Option<PathBuf>,
    model_id: String,
    umap_version: String,
    seed: u64,
}

fn parse_args(args: Vec<String>) -> Result<Option<Options>, String> {
    let mut options = Options {
        db_path: None,
        model_id: String::new(),
        umap_version: String::new(),
        seed: 0,
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
            "--umap-version" => {
                idx += 1;
                let value =
                    args.get(idx).ok_or_else(|| "--umap-version requires a value".to_string())?;
                options.umap_version = value.to_string();
            }
            "--seed" => {
                idx += 1;
                let value = args.get(idx).ok_or_else(|| "--seed requires a value".to_string())?;
                options.seed = value
                    .parse::<u64>()
                    .map_err(|_| format!("Invalid --seed value: {value}"))?;
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
    if options.umap_version.trim().is_empty() {
        return Err("--umap-version is required".to_string());
    }

    Ok(Some(options))
}

fn help_text() -> String {
    [
        "sempal-umap",
        "",
        "Build a UMAP layout for stored embeddings.",
        "",
        "Usage:",
        "  sempal-umap --model-id <id> --umap-version <version> [--db <path>] [--seed <u64>]",
        "",
        "Options:",
        "  --db <path>          Path to library.db (defaults to app data location).",
        "  --model-id <id>      Embedding model id to read (required).",
        "  --umap-version <v>   Layout version tag to store (required).",
        "  --seed <u64>         Seed for deterministic layouts (default: 0).",
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
    let dataset = umap_rs::Dataset::new(data, n_samples, dim);
    let umap = umap_rs::Umap::new(
        DEFAULT_NEIGHBORS,
        DEFAULT_N_COMPONENTS,
        DEFAULT_MIN_DIST,
        seed,
    );
    let embedding = umap
        .fit_transform(&dataset)
        .map_err(|err| format!("UMAP fit failed: {err}"))?;
    if embedding.ncols() != 2 {
        return Err(format!(
            "UMAP returned {} columns, expected 2",
            embedding.ncols()
        ));
    }
    let mut out = Vec::with_capacity(n_samples);
    for row in embedding.rows() {
        out.push([row[0], row[1]]);
    }
    Ok(out)
}

fn write_layout(
    conn: &Connection,
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
    tx.commit()
        .map_err(|err| format!("Commit layout failed: {err}"))?;
    Ok(sample_ids.len())
}
