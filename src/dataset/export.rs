//! Export feature vectors + labels for model training.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, create_dir_all};
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use blake3::Hasher;
use rusqlite::{Connection, params};
use thiserror::Error;

use crate::analysis::vector::{FEATURE_VECTOR_LEN_V1, FEATURE_VERSION_V1};
use crate::labeling::weak::WEAK_LABEL_RULESET_VERSION;
use crate::sample_sources::library::LIBRARY_DB_FILE_NAME;

/// Configuration options for `export_training_dataset`.
#[derive(Debug, Clone)]
pub struct ExportOptions {
    /// Output directory containing `manifest.json`, `samples.jsonl`, and `features.f32le`.
    pub out_dir: PathBuf,
    /// Optional override for the `library.db` path (defaults to the app data location).
    pub db_path: Option<PathBuf>,
    /// Minimum confidence required to include a weak label.
    pub min_confidence: f32,
    /// Number of leading folder components used to derive `pack_id`.
    pub pack_depth: usize,
    /// Seed used to assign packs to train/val/test splits deterministically.
    pub seed: String,
    /// Fraction of packs assigned to the test split.
    pub test_fraction: f64,
    /// Fraction of packs assigned to the validation split.
    pub val_fraction: f64,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            out_dir: PathBuf::from("dataset_out"),
            db_path: None,
            min_confidence: 0.85,
            pack_depth: 1,
            seed: "sempal-dataset-v1".to_string(),
            test_fraction: 0.1,
            val_fraction: 0.1,
        }
    }
}

#[derive(Debug, Clone)]
/// Summary of an export run.
pub struct ExportSummary {
    /// Total samples written to `samples.jsonl` and `features.f32le`.
    pub total_exported: usize,
    /// Total unique packs present in the export.
    pub total_packs: usize,
}

/// Errors returned when exporting a dataset.
#[derive(Debug, Error)]
pub enum ExportError {
    #[error("invalid split fractions: test={test_fraction}, val={val_fraction}")]
    InvalidSplitFractions { test_fraction: f64, val_fraction: f64 },
    #[error("missing database path (and no app root dir available)")]
    MissingDbPath,
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid sample_id: {0}")]
    InvalidSampleId(String),
}

/// A single label attached to an exported sample.
#[derive(Debug, Clone)]
pub struct ExportedLabel {
    /// Training class identifier.
    pub class_id: String,
    /// Confidence score in `[0, 1]`.
    pub confidence: f32,
    /// Identifier of the rule that produced this label.
    pub rule_id: String,
}

/// Dataset split for an exported sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatasetSplit {
    Train,
    Val,
    Test,
}

impl DatasetSplit {
    pub fn as_str(self) -> &'static str {
        match self {
            DatasetSplit::Train => "train",
            DatasetSplit::Val => "val",
            DatasetSplit::Test => "test",
        }
    }
}

/// Export a deterministic dataset built from `features` and `labels_weak` in the library database.
pub fn export_training_dataset(options: &ExportOptions) -> Result<ExportSummary, ExportError> {
    if options.test_fraction < 0.0
        || options.val_fraction < 0.0
        || options.test_fraction + options.val_fraction >= 1.0
    {
        return Err(ExportError::InvalidSplitFractions {
            test_fraction: options.test_fraction,
            val_fraction: options.val_fraction,
        });
    }

    let db_path = match options.db_path.clone() {
        Some(path) => path,
        None => {
            let root = crate::app_dirs::app_root_dir().map_err(|_| ExportError::MissingDbPath)?;
            root.join(LIBRARY_DB_FILE_NAME)
        }
    };

    create_dir_all(&options.out_dir)?;
    let conn = Connection::open(db_path)?;

    let labels = load_best_weak_labels(&conn, options.min_confidence)?;
    let mut packs: BTreeSet<String> = BTreeSet::new();
    for sample_id in labels.keys() {
        packs.insert(pack_id_for_sample_id(sample_id, options.pack_depth)?);
    }
    let pack_splits = build_pack_splits(
        &packs,
        &options.seed,
        options.test_fraction,
        options.val_fraction,
    );

    let features_path = options.out_dir.join("features.f32le");
    let mut features_writer = BufWriter::new(File::create(&features_path)?);

    let samples_path = options.out_dir.join("samples.jsonl");
    let mut samples_writer = BufWriter::new(File::create(&samples_path)?);

    let manifest_path = options.out_dir.join("manifest.json");
    let mut manifest_writer = BufWriter::new(File::create(&manifest_path)?);

    let mut exported = 0usize;
    let mut stmt = conn.prepare(
        "SELECT sample_id, feat_version, vec_blob
         FROM features
         WHERE feat_version = ?1
         ORDER BY sample_id ASC",
    )?;
    let mut rows = stmt.query(params![FEATURE_VERSION_V1])?;

    let mut offset_bytes: u64 = 0;
    while let Some(row) = rows.next()? {
        let sample_id: String = row.get(0)?;
        let feat_version: i64 = row.get(1)?;
        let vec_blob: Vec<u8> = row.get(2)?;

        let Some(label) = labels.get(&sample_id) else {
            continue;
        };
        let pack_id = pack_id_for_sample_id(&sample_id, options.pack_depth)?;
        let split = pack_splits
            .get(&pack_id)
            .copied()
            .unwrap_or(DatasetSplit::Train);

        if vec_blob.len() != FEATURE_VECTOR_LEN_V1 * std::mem::size_of::<f32>() {
            continue;
        }

        features_writer.write_all(&vec_blob)?;

        let record = serde_json::json!({
            "sample_id": sample_id,
            "pack_id": pack_id,
            "split": split.as_str(),
            "label": {
                "class_id": label.class_id,
                "confidence": label.confidence,
                "rule_id": label.rule_id,
                "ruleset_version": WEAK_LABEL_RULESET_VERSION,
            },
            "features": {
                "feat_version": feat_version,
                "offset_bytes": offset_bytes,
                "len_f32": FEATURE_VECTOR_LEN_V1,
                "encoding": "f32le",
            },
        });
        serde_json::to_writer(&mut samples_writer, &record)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))?;
        samples_writer.write_all(b"\n")?;

        offset_bytes += vec_blob.len() as u64;
        exported += 1;
    }
    features_writer.flush()?;
    samples_writer.flush()?;

    let mut pack_counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    for split in pack_splits.values() {
        *pack_counts.entry(split.as_str()).or_default() += 1;
    }
    let manifest = serde_json::json!({
        "format_version": 1,
        "feature_encoding": "f32le",
        "feat_version": FEATURE_VERSION_V1,
        "feature_len_f32": FEATURE_VECTOR_LEN_V1,
        "label_source": "labels_weak",
        "ruleset_version": WEAK_LABEL_RULESET_VERSION,
        "min_confidence": options.min_confidence,
        "pack_depth": options.pack_depth,
        "seed": options.seed,
        "splits": {
            "test_fraction": options.test_fraction,
            "val_fraction": options.val_fraction,
            "pack_counts": pack_counts,
        },
        "exported_samples": exported,
        "exported_packs": pack_splits.len(),
        "files": {
            "samples": "samples.jsonl",
            "features": "features.f32le",
        }
    });
    serde_json::to_writer_pretty(&mut manifest_writer, &manifest)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))?;
    manifest_writer.flush()?;

    Ok(ExportSummary {
        total_exported: exported,
        total_packs: pack_splits.len(),
    })
}

fn load_best_weak_labels(
    conn: &Connection,
    min_confidence: f32,
) -> Result<BTreeMap<String, ExportedLabel>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT sample_id, class_id, confidence, rule_id
         FROM labels_weak
         WHERE ruleset_version = ?1 AND confidence >= ?2
         ORDER BY sample_id ASC, confidence DESC, class_id ASC",
    )?;
    let mut rows = stmt.query(params![WEAK_LABEL_RULESET_VERSION, min_confidence as f64])?;
    let mut best: BTreeMap<String, ExportedLabel> = BTreeMap::new();
    while let Some(row) = rows.next()? {
        let sample_id: String = row.get(0)?;
        if best.contains_key(&sample_id) {
            continue;
        }
        best.insert(
            sample_id,
            ExportedLabel {
                class_id: row.get(1)?,
                confidence: row.get::<_, f64>(2)? as f32,
                rule_id: row.get(3)?,
            },
        );
    }
    Ok(best)
}

/// Derive a pack identifier from the sample id and leading folder hierarchy.
///
/// Pack ids include the `source_id` prefix so identical pack names across sources remain isolated.
pub fn pack_id_for_sample_id(sample_id: &str, depth: usize) -> Result<String, ExportError> {
    let (source_id, relative_path) = parse_sample_id(sample_id)?;
    let normalized = relative_path.replace('\\', "/");
    let segments: Vec<&str> = normalized
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    let folder_segments = if segments.len() <= 1 {
        &[][..]
    } else {
        &segments[..segments.len() - 1]
    };
    let mut out = Vec::new();
    for _ in 0..depth.max(1) {
        if let Some(component) = folder_segments.get(out.len()) {
            out.push(*component);
        } else {
            break;
        }
    }
    let pack = if out.is_empty() {
        "_root".to_string()
    } else {
        out.join("/")
    };
    Ok(format!("{source_id}/{pack}"))
}

fn parse_sample_id(sample_id: &str) -> Result<(String, String), ExportError> {
    let (source, path) = sample_id
        .split_once("::")
        .ok_or_else(|| ExportError::InvalidSampleId(sample_id.to_string()))?;
    if source.is_empty() || path.is_empty() {
        return Err(ExportError::InvalidSampleId(sample_id.to_string()));
    }
    Ok((source.to_string(), path.to_string()))
}

/// Assign a stable dataset split for a pack id.
pub fn split_for_pack_id(
    pack_id: &str,
    seed: &str,
    test_fraction: f64,
    val_fraction: f64,
) -> DatasetSplit {
    let mut hasher = Hasher::new();
    hasher.update(seed.as_bytes());
    hasher.update(b"\0");
    hasher.update(pack_id.as_bytes());
    let hash = hasher.finalize();
    let bytes: [u8; 8] = hash.as_bytes()[0..8]
        .try_into()
        .expect("slice length verified");
    let value = u64::from_le_bytes(bytes);
    let unit = (value as f64) / (u64::MAX as f64);
    if unit < test_fraction {
        DatasetSplit::Test
    } else if unit < test_fraction + val_fraction {
        DatasetSplit::Val
    } else {
        DatasetSplit::Train
    }
}

fn build_pack_splits(
    packs: &BTreeSet<String>,
    seed: &str,
    test_fraction: f64,
    val_fraction: f64,
) -> BTreeMap<String, DatasetSplit> {
    packs
        .iter()
        .map(|pack| {
            (
                pack.clone(),
                split_for_pack_id(pack, seed, test_fraction, val_fraction),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_id_uses_top_level_folder_and_source_id() {
        let pack = pack_id_for_sample_id("src1::PackA/Drums/Kick.wav", 1).unwrap();
        assert_eq!(pack, "src1/PackA");
    }

    #[test]
    fn pack_id_depth_joins_multiple_segments() {
        let pack = pack_id_for_sample_id("src1::PackA/Drums/Kick.wav", 2).unwrap();
        assert_eq!(pack, "src1/PackA/Drums");
    }

    #[test]
    fn pack_id_root_falls_back_to_root_marker() {
        let pack = pack_id_for_sample_id("src1::Kick.wav", 2).unwrap();
        assert_eq!(pack, "src1/_root");
    }

    #[test]
    fn split_is_deterministic_for_same_seed() {
        let a = split_for_pack_id("src1/PackA", "seed", 0.1, 0.1);
        let b = split_for_pack_id("src1/PackA", "seed", 0.1, 0.1);
        assert_eq!(a, b);
    }

    #[test]
    fn split_changes_with_seed() {
        fn hash_u64(pack_id: &str, seed: &str) -> u64 {
            let mut hasher = Hasher::new();
            hasher.update(seed.as_bytes());
            hasher.update(b"\0");
            hasher.update(pack_id.as_bytes());
            let hash = hasher.finalize();
            let bytes: [u8; 8] = hash.as_bytes()[0..8].try_into().unwrap();
            u64::from_le_bytes(bytes)
        }
        let a = hash_u64("src1/PackA", "seed-a");
        let b = hash_u64("src1/PackA", "seed-b");
        assert_ne!(a, b);
    }
}
