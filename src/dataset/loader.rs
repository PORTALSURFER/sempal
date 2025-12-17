//! Dataset loader for `dataset_out` exports.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DatasetLoadError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid manifest")]
    InvalidManifest,
    #[error("invalid samples.jsonl: {0}")]
    InvalidSamples(String),
    #[error("missing features file")]
    MissingFeatures,
    #[error("feature blob size mismatch")]
    FeatureBlobSizeMismatch,
}

/// Fully loaded dataset export (features + sample metadata).
#[derive(Debug, Clone)]
pub struct LoadedDataset {
    /// Parsed `manifest.json`.
    pub manifest: Manifest,
    /// Parsed `samples.jsonl`.
    pub samples: Vec<SampleRecord>,
    /// Feature file expanded into `f32` values (little-endian encoding).
    pub features_f32: Vec<f32>,
}

/// Parsed contents of `manifest.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    /// Dataset format version.
    pub format_version: i64,
    /// Feature version for each row.
    pub feat_version: i64,
    /// Feature vector length in `f32` values.
    pub feature_len_f32: usize,
    /// Dataset file paths (relative to dataset directory).
    pub files: ManifestFiles,
}

/// File names referenced by the manifest.
#[derive(Debug, Clone, Deserialize)]
pub struct ManifestFiles {
    /// JSONL file containing sample metadata records.
    pub samples: String,
    /// Raw feature blob file containing `f32` values.
    pub features: String,
}

/// Per-sample metadata record from `samples.jsonl`.
#[derive(Debug, Clone, Deserialize)]
pub struct SampleRecord {
    /// Sample identifier (`source_id::relative_path`).
    pub sample_id: String,
    /// Pack identifier used for leakage-free splitting.
    pub pack_id: String,
    /// Dataset split (`train`, `val`, `test`).
    pub split: String,
    /// Label associated with the sample.
    pub label: SampleLabel,
    /// Feature reference into the feature blob.
    pub features: SampleFeatures,
}

/// Weak label record attached to a dataset sample.
#[derive(Debug, Clone, Deserialize)]
pub struct SampleLabel {
    /// Training class identifier.
    pub class_id: String,
    /// Confidence in `[0, 1]`.
    pub confidence: f32,
    /// Rule identifier that produced the label.
    pub rule_id: String,
    /// Weak-label ruleset version.
    pub ruleset_version: i64,
}

/// Feature reference record attached to a dataset sample.
#[derive(Debug, Clone, Deserialize)]
pub struct SampleFeatures {
    /// Feature version for the row.
    pub feat_version: i64,
    /// Byte offset into the feature file.
    pub offset_bytes: u64,
    /// Feature vector length in `f32` values.
    pub len_f32: usize,
    /// Encoding identifier (currently `f32le`).
    pub encoding: String,
}

impl LoadedDataset {
    /// Enumerate unique class ids in deterministic order.
    pub fn class_ids(&self) -> Vec<String> {
        let mut set = std::collections::BTreeSet::new();
        for sample in &self.samples {
            set.insert(sample.label.class_id.clone());
        }
        set.into_iter().collect()
    }

    pub fn class_index_map(&self) -> BTreeMap<String, usize> {
        self.class_ids()
            .into_iter()
            .enumerate()
            .map(|(idx, class_id)| (class_id, idx))
            .collect()
    }

    /// Borrow the feature row slice for a sample record.
    pub fn feature_row(&self, sample: &SampleRecord) -> Option<&[f32]> {
        if sample.features.encoding != "f32le" {
            return None;
        }
        let offset_f32 = (sample.features.offset_bytes / 4) as usize;
        let len = sample.features.len_f32;
        self.features_f32.get(offset_f32..offset_f32 + len)
    }
}

/// Load a dataset export directory produced by `sempal-dataset-export`.
pub fn load_dataset(dir: &Path) -> Result<LoadedDataset, DatasetLoadError> {
    let manifest_path = dir.join("manifest.json");
    let mut manifest_bytes = Vec::new();
    File::open(&manifest_path)?.read_to_end(&mut manifest_bytes)?;
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)?;
    if manifest.format_version != 1 || manifest.feature_len_f32 == 0 {
        return Err(DatasetLoadError::InvalidManifest);
    }

    let samples_path = dir.join(&manifest.files.samples);
    let samples = load_samples_jsonl(&samples_path)?;

    let features_path = dir.join(&manifest.files.features);
    if !features_path.is_file() {
        return Err(DatasetLoadError::MissingFeatures);
    }
    let features_f32 = load_f32le(&features_path)?;
    if features_f32.len() < samples.len().saturating_mul(manifest.feature_len_f32) {
        // Allow missing rows when some samples were skipped during export, but basic sanity should hold.
        return Err(DatasetLoadError::FeatureBlobSizeMismatch);
    }

    Ok(LoadedDataset {
        manifest,
        samples,
        features_f32,
    })
}

fn load_samples_jsonl(path: &PathBuf) -> Result<Vec<SampleRecord>, DatasetLoadError> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut out = Vec::new();
    for (idx, line) in reader.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let record: SampleRecord = serde_json::from_str(&line)
            .map_err(|err| DatasetLoadError::InvalidSamples(format!("line {}: {err}", idx + 1)))?;
        out.push(record);
    }
    Ok(out)
}

fn load_f32le(path: &Path) -> Result<Vec<f32>, DatasetLoadError> {
    let mut bytes = Vec::new();
    File::open(path)?.read_to_end(&mut bytes)?;
    if bytes.len() % 4 != 0 {
        return Err(DatasetLoadError::FeatureBlobSizeMismatch);
    }
    let mut out = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        out.push(f32::from_le_bytes(chunk.try_into().expect("chunk size verified")));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn loads_minimal_dataset() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        std::fs::write(
            root.join("features.f32le"),
            [0.0f32, 1.0f32, 2.0f32, 3.0f32]
                .into_iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<u8>>(),
        )
        .unwrap();

        std::fs::write(
            root.join("samples.jsonl"),
            r#"{"sample_id":"s::a.wav","pack_id":"s/Pack","split":"train","label":{"class_id":"kick","confidence":1.0,"rule_id":"x","ruleset_version":1},"features":{"feat_version":1,"offset_bytes":0,"len_f32":2,"encoding":"f32le"}}
{"sample_id":"s::b.wav","pack_id":"s/Pack","split":"test","label":{"class_id":"snare","confidence":1.0,"rule_id":"y","ruleset_version":1},"features":{"feat_version":1,"offset_bytes":8,"len_f32":2,"encoding":"f32le"}}
"#,
        )
        .unwrap();

        std::fs::write(
            root.join("manifest.json"),
            r#"{"format_version":1,"feature_encoding":"f32le","feat_version":1,"feature_len_f32":2,"files":{"samples":"samples.jsonl","features":"features.f32le"}}"#,
        )
        .unwrap();

        let loaded = load_dataset(root).unwrap();
        assert_eq!(loaded.samples.len(), 2);
        assert_eq!(loaded.features_f32.len(), 4);
        assert_eq!(
            loaded.feature_row(&loaded.samples[0]).unwrap(),
            &[0.0, 1.0]
        );
        assert_eq!(
            loaded.feature_row(&loaded.samples[1]).unwrap(),
            &[2.0, 3.0]
        );
    }
}
