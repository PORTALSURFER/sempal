//! Dataset export helpers for training pipelines.

mod stats;

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use thiserror::Error;

use crate::analysis::{FEATURE_VECTOR_LEN_V1, FEATURE_VERSION_V1};

pub use stats::{ExportDiagnostics, diagnose_export};
use stats::load_export_rows_filtered;

const DATASET_FORMAT_VERSION: i64 = 1;
const DEFAULT_RULESET_VERSION: i64 = 1;
const FEATURES_FILE_NAME: &str = "features.f32le";
const SAMPLES_FILE_NAME: &str = "samples.jsonl";
const MANIFEST_FILE_NAME: &str = "manifest.json";

#[derive(Debug, Clone)]
pub struct ExportOptions {
    /// Output directory (created if missing).
    pub out_dir: PathBuf,
    /// Optional explicit library DB path.
    pub db_path: Option<PathBuf>,
    /// Minimum weak-label confidence to include a sample.
    pub min_confidence: f32,
    /// Number of relative-path folder components used to compute pack_id.
    pub pack_depth: usize,
    /// Include user override labels when exporting.
    pub use_user_labels: bool,
    /// Seed used for deterministic pack split assignment.
    pub seed: String,
    /// Fraction of packs assigned to `test`.
    pub test_fraction: f64,
    /// Fraction of packs assigned to `val`.
    pub val_fraction: f64,
    /// Split strategy for train/val/test.
    pub split_mode: SplitMode,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            out_dir: PathBuf::new(),
            db_path: None,
            min_confidence: 0.85,
            pack_depth: 1,
            use_user_labels: true,
            seed: "sempal-dataset-v1".to_string(),
            test_fraction: 0.1,
            val_fraction: 0.1,
            split_mode: SplitMode::Pack,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitMode {
    /// Keep all samples from a pack together.
    Pack,
    /// Stratify by class label across train/val/test.
    Stratified,
}

impl ExportOptions {
    pub fn resolved_db_path(&self) -> Result<PathBuf, ExportError> {
        if let Some(path) = &self.db_path {
            return Ok(path.clone());
        }
        let _ = crate::sample_sources::library::load()?;
        let root = crate::app_dirs::app_root_dir()?;
        Ok(root.join(crate::sample_sources::library::LIBRARY_DB_FILE_NAME))
    }
}

#[derive(Debug, Clone)]
pub struct ExportSummary {
    pub total_exported: usize,
    pub total_packs: usize,
    pub db_path: PathBuf,
}

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("invalid dataset split fractions (val+test must be <= 1.0)")]
    InvalidSplitFractions,
    #[error("invalid min_confidence {0} (expected 0..=1)")]
    InvalidMinConfidence(f32),
    #[error("invalid pack_depth 0 (expected >= 1)")]
    InvalidPackDepth,
    #[error("library error: {0}")]
    Library(#[from] crate::sample_sources::library::LibraryError),
    #[error("app dirs error: {0}")]
    AppDirs(#[from] crate::app_dirs::AppDirError),
    #[error("sql error: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("export query failed: {0}")]
    Query(String),
}

/// Export a dataset directory suitable for training (`manifest.json`, `samples.jsonl`, `features.f32le`).
pub fn export_training_dataset(options: &ExportOptions) -> Result<ExportSummary, ExportError> {
    validate_options(options)?;

    let db_path = options.resolved_db_path()?;
    let conn = stats::open_db(&db_path)?;
    let rows = stats::load_export_rows(
        &conn,
        options.min_confidence,
        DEFAULT_RULESET_VERSION,
        options.use_user_labels,
    )?;
    export_rows_to_dir(
        rows,
        options,
        &db_path,
        FEATURE_VECTOR_LEN_V1,
        FEATURE_VERSION_V1,
    )
}

/// Export a dataset directory using embedding vectors instead of feature vectors.
pub fn export_embedding_dataset(options: &ExportOptions) -> Result<ExportSummary, ExportError> {
    validate_options(options)?;

    let db_path = options.resolved_db_path()?;
    let conn = stats::open_db(&db_path)?;
    let rows = stats::load_embedding_export_rows_filtered(
        &conn,
        options.min_confidence,
        DEFAULT_RULESET_VERSION,
        None,
        options.use_user_labels,
        crate::analysis::embedding::EMBEDDING_MODEL_ID,
    )?;
    export_rows_to_dir(
        rows,
        options,
        &db_path,
        crate::analysis::embedding::EMBEDDING_DIM,
        0,
    )
}

/// Export a dataset directory suitable for training (`manifest.json`, `samples.jsonl`, `features.f32le`)
/// restricted to the provided `source_ids`.
pub fn export_training_dataset_for_sources(
    options: &ExportOptions,
    source_ids: &[String],
) -> Result<ExportSummary, ExportError> {
    validate_options(options)?;

    let db_path = options.resolved_db_path()?;
    let conn = stats::open_db(&db_path)?;
    let rows = load_export_rows_filtered(
        &conn,
        options.min_confidence,
        DEFAULT_RULESET_VERSION,
        Some(source_ids),
        options.use_user_labels,
    )?;
    export_rows_to_dir(
        rows,
        options,
        &db_path,
        FEATURE_VECTOR_LEN_V1,
        FEATURE_VERSION_V1,
    )
}

/// Export a dataset directory using embedding vectors instead of feature vectors.
pub fn export_embedding_dataset_for_sources(
    options: &ExportOptions,
    source_ids: &[String],
) -> Result<ExportSummary, ExportError> {
    validate_options(options)?;

    let db_path = options.resolved_db_path()?;
    let conn = stats::open_db(&db_path)?;
    let rows = stats::load_embedding_export_rows_filtered(
        &conn,
        options.min_confidence,
        DEFAULT_RULESET_VERSION,
        Some(source_ids),
        options.use_user_labels,
        crate::analysis::embedding::EMBEDDING_MODEL_ID,
    )?;
    export_rows_to_dir(
        rows,
        options,
        &db_path,
        crate::analysis::embedding::EMBEDDING_DIM,
        0,
    )
}

fn validate_options(options: &ExportOptions) -> Result<(), ExportError> {
    if options.pack_depth == 0 {
        return Err(ExportError::InvalidPackDepth);
    }
    if !(0.0..=1.0).contains(&options.min_confidence) {
        return Err(ExportError::InvalidMinConfidence(options.min_confidence));
    }
    if options.test_fraction + options.val_fraction > 1.0 + f64::EPSILON {
        return Err(ExportError::InvalidSplitFractions);
    }
    Ok(())
}

fn export_rows_to_dir(
    rows: Vec<stats::ExportRow>,
    options: &ExportOptions,
    db_path: &PathBuf,
    vector_len_f32: usize,
    feat_version: i64,
) -> Result<ExportSummary, ExportError> {
    std::fs::create_dir_all(&options.out_dir)?;

    let mut packs = BTreeSet::new();
    let stratified_splits = if options.split_mode == SplitMode::Stratified {
        Some(assign_stratified_splits(
            &rows,
            &options.seed,
            options.test_fraction,
            options.val_fraction,
        )?)
    } else {
        None
    };
    let mut exported: Vec<ExportedSample> = Vec::with_capacity(rows.len());
    for row in rows {
        let Some(pack_id) = pack_id_for_sample_id(&row.sample_id, options.pack_depth) else {
            continue;
        };
        let split = if let Some(map) = stratified_splits.as_ref() {
            map.get(&row.sample_id)
                .cloned()
                .unwrap_or_else(|| "train".to_string())
        } else {
            split_for_pack_id(
                &pack_id,
                &options.seed,
                options.test_fraction,
                options.val_fraction,
            )?
        };
        packs.insert(pack_id.clone());
        exported.push(ExportedSample {
            sample_id: row.sample_id,
            pack_id,
            split,
            label: ExportedLabel {
                class_id: row.class_id,
                confidence: row.confidence,
                rule_id: row.rule_id,
                ruleset_version: row.ruleset_version,
            },
            vec_blob: row.vec_blob,
        });
    }

    exported.sort_by(|a, b| a.pack_id.cmp(&b.pack_id).then_with(|| a.sample_id.cmp(&b.sample_id)));

    let features_path = options.out_dir.join(FEATURES_FILE_NAME);
    let samples_path = options.out_dir.join(SAMPLES_FILE_NAME);
    let manifest_path = options.out_dir.join(MANIFEST_FILE_NAME);

    let mut features_writer = BufWriter::new(File::create(&features_path)?);
    let mut samples_writer = BufWriter::new(File::create(&samples_path)?);

    let mut offset_bytes: u64 = 0;
    let mut written = 0usize;
    for sample in exported {
        if sample.vec_blob.len() != vector_len_f32 * 4 {
            continue;
        }
        features_writer.write_all(&sample.vec_blob)?;
        let record = DatasetSampleRecord {
            sample_id: sample.sample_id,
            pack_id: sample.pack_id,
            split: sample.split,
            label: sample.label,
            features: DatasetFeaturesRef {
                feat_version,
                offset_bytes,
                len_f32: vector_len_f32,
                encoding: "f32le".to_string(),
            },
        };
        serde_json::to_writer(&mut samples_writer, &record)?;
        samples_writer.write_all(b"\n")?;
        offset_bytes += (vector_len_f32 * 4) as u64;
        written += 1;
    }
    features_writer.flush()?;
    samples_writer.flush()?;

    let manifest = DatasetManifest {
        format_version: DATASET_FORMAT_VERSION,
        feature_encoding: "f32le".to_string(),
        feat_version,
        feature_len_f32: vector_len_f32,
        files: DatasetManifestFiles {
            samples: SAMPLES_FILE_NAME.to_string(),
            features: FEATURES_FILE_NAME.to_string(),
        },
    };
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    std::fs::write(&manifest_path, manifest_bytes)?;

    Ok(ExportSummary {
        total_exported: written,
        total_packs: packs.len(),
        db_path: db_path.clone(),
    })
}

#[derive(Debug, Clone)]
struct ExportedSample {
    sample_id: String,
    pack_id: String,
    split: String,
    label: ExportedLabel,
    vec_blob: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExportedLabel {
    pub class_id: String,
    pub confidence: f32,
    pub rule_id: String,
    pub ruleset_version: i64,
}

#[derive(Debug, Clone, Serialize)]
struct DatasetFeaturesRef {
    feat_version: i64,
    offset_bytes: u64,
    len_f32: usize,
    encoding: String,
}

#[derive(Debug, Clone, Serialize)]
struct DatasetSampleRecord {
    sample_id: String,
    pack_id: String,
    split: String,
    label: ExportedLabel,
    features: DatasetFeaturesRef,
}

#[derive(Debug, Clone, Serialize)]
struct DatasetManifestFiles {
    samples: String,
    features: String,
}

#[derive(Debug, Clone, Serialize)]
struct DatasetManifest {
    format_version: i64,
    feature_encoding: String,
    feat_version: i64,
    feature_len_f32: usize,
    files: DatasetManifestFiles,
}

fn pack_id_for_sample_id(sample_id: &str, pack_depth: usize) -> Option<String> {
    let (source_id, rel) = sample_id.split_once("::")?;
    let rel = rel.replace('\\', "/");
    let rel_path = Path::new(&rel);
    let parent = rel_path.parent()?;
    let mut parts = Vec::new();
    for component in parent.components().filter_map(|c| match c {
        std::path::Component::Normal(os) => os.to_str().map(|s| s.to_string()),
        _ => None,
    }) {
        parts.push(component);
        if parts.len() >= pack_depth {
            break;
        }
    }
    if parts.is_empty() {
        return Some(source_id.to_string());
    }
    Some(format!("{}/{}", source_id, parts.join("/")))
}

fn split_for_pack_id(
    pack_id: &str,
    seed: &str,
    test_fraction: f64,
    val_fraction: f64,
) -> Result<String, ExportError> {
    if test_fraction + val_fraction > 1.0 + f64::EPSILON {
        return Err(ExportError::InvalidSplitFractions);
    }
    let hash = blake3::hash(format!("{seed}|{pack_id}").as_bytes());
    let bytes = hash.as_bytes();
    let u = u64::from_le_bytes(bytes[0..8].try_into().expect("slice size verified"));
    let frac = (u as f64) / (u64::MAX as f64);
    let split = if frac < test_fraction {
        "test"
    } else if frac < test_fraction + val_fraction {
        "val"
    } else {
        "train"
    };
    Ok(split.to_string())
}

fn assign_stratified_splits(
    rows: &[stats::ExportRow],
    seed: &str,
    test_fraction: f64,
    val_fraction: f64,
) -> Result<std::collections::HashMap<String, String>, ExportError> {
    if test_fraction + val_fraction > 1.0 + f64::EPSILON {
        return Err(ExportError::InvalidSplitFractions);
    }
    let mut by_class: BTreeMap<String, Vec<(u128, String)>> = BTreeMap::new();
    for row in rows {
        let hash = blake3::hash(format!("{seed}|{}|{}", row.class_id, row.sample_id).as_bytes());
        let key = u128::from_le_bytes(hash.as_bytes()[0..16].try_into().expect("slice size"));
        by_class
            .entry(row.class_id.clone())
            .or_default()
            .push((key, row.sample_id.clone()));
    }

    let mut splits = std::collections::HashMap::new();
    for (_class_id, mut entries) in by_class {
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        let n = entries.len();
        if n == 0 {
            continue;
        }
        let mut test_n = ((n as f64) * test_fraction).round() as usize;
        let mut val_n = ((n as f64) * val_fraction).round() as usize;
        if n == 1 {
            test_n = 0;
            val_n = 0;
        } else {
            while test_n + val_n >= n {
                if val_n > 0 {
                    val_n -= 1;
                } else if test_n > 0 {
                    test_n -= 1;
                } else {
                    break;
                }
            }
        }
        for (idx, (_hash, sample_id)) in entries.into_iter().enumerate() {
            let split = if idx < test_n {
                "test"
            } else if idx < test_n + val_n {
                "val"
            } else {
                "train"
            };
            splits.insert(sample_id, split.to_string());
        }
    }
    Ok(splits)
}

pub fn pack_split_counts(samples: &[ExportDiagnosticsSample]) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for sample in samples {
        *out.entry(sample.split.clone()).or_insert(0) += 1;
    }
    out
}

#[derive(Debug, Clone)]
pub struct ExportDiagnosticsSample {
    pub sample_id: String,
    pub pack_id: String,
    pub split: String,
}

#[cfg(test)]
mod tests;
