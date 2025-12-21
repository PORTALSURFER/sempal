//! Export curated folder datasets into manifest-based embedding datasets.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use tracing::warn;

use crate::analysis::embedding::EMBEDDING_DIM;
use crate::sample_sources::config::TrainingAugmentation;

use super::embeddings::{augment_rng, build_embedding_variants};
use super::progress::progress_tick;
use super::{collect_training_samples, filter_training_samples, stratified_split_map, TrainingProgress};

const DATASET_FORMAT_VERSION: i64 = 1;
const RULESET_VERSION: i64 = 1;
const FEATURES_FILE_NAME: &str = "features.f32le";
const SAMPLES_FILE_NAME: &str = "samples.jsonl";
const MANIFEST_FILE_NAME: &str = "manifest.json";

#[derive(Debug, Clone)]
pub struct CuratedExportOptions {
    pub dataset_dir: PathBuf,
    pub out_dir: PathBuf,
    pub min_class_samples: usize,
    pub augmentation: TrainingAugmentation,
    pub seed: String,
    pub test_fraction: f64,
    pub val_fraction: f64,
    pub pack_depth: usize,
    pub use_hybrid: bool,
}

impl Default for CuratedExportOptions {
    fn default() -> Self {
        Self {
            dataset_dir: PathBuf::new(),
            out_dir: PathBuf::new(),
            min_class_samples: 30,
            augmentation: TrainingAugmentation::default(),
            seed: "sempal-curated-dataset-v1".to_string(),
            test_fraction: 0.1,
            val_fraction: 0.1,
            pack_depth: 1,
            use_hybrid: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CuratedExportSummary {
    pub total_exported: usize,
    pub total_samples: usize,
    pub total_classes: usize,
    pub skipped: usize,
    pub class_counts: BTreeMap<String, usize>,
}

pub fn export_curated_embedding_dataset(
    options: &CuratedExportOptions,
) -> Result<CuratedExportSummary, String> {
    let mut progress = None;
    export_curated_embedding_dataset_with_progress(options, &mut progress)
}

pub fn export_curated_embedding_dataset_with_progress(
    options: &CuratedExportOptions,
    progress: &mut Option<&mut dyn FnMut(TrainingProgress)>,
) -> Result<CuratedExportSummary, String> {
    validate_options(options)?;
    std::fs::create_dir_all(&options.out_dir).map_err(|err| err.to_string())?;

    let samples = load_samples(options)?;
    let split_map =
        stratified_split_map(&samples, &options.seed, options.test_fraction, options.val_fraction)?;
    let total_classes = count_classes(&samples);

    let mut writers = DatasetWriters::new(&options.out_dir)?;
    let mut state = ExportState::new(options, samples.len());
    export_all_samples(
        options,
        &samples,
        &split_map,
        &mut writers,
        &mut state,
        progress,
    )?;
    writers.finish()?;
    write_manifest(&options.out_dir, feature_len(options.use_hybrid))?;
    state.warn_if_skipped();

    Ok(build_summary(state, samples.len(), total_classes))
}

fn validate_options(options: &CuratedExportOptions) -> Result<(), String> {
    if options.pack_depth == 0 {
        return Err("pack_depth must be >= 1".to_string());
    }
    if options.test_fraction + options.val_fraction > 1.0 + f64::EPSILON {
        return Err("Invalid split fractions".to_string());
    }
    if options.dataset_dir.as_os_str().is_empty() {
        return Err("dataset_dir is required".to_string());
    }
    if options.out_dir.as_os_str().is_empty() {
        return Err("out_dir is required".to_string());
    }
    Ok(())
}

fn load_samples(options: &CuratedExportOptions) -> Result<Vec<super::TrainingSample>, String> {
    let samples = collect_training_samples(&options.dataset_dir)?;
    if samples.is_empty() {
        return Err("Training dataset folder is empty".to_string());
    }
    let samples = filter_training_samples(samples, options.min_class_samples);
    if samples.is_empty() {
        return Err("Training dataset has no classes after hygiene filter".to_string());
    }
    Ok(samples)
}

fn count_classes(samples: &[super::TrainingSample]) -> usize {
    samples
        .iter()
        .map(|sample| sample.class_id.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .len()
}

fn export_all_samples(
    options: &CuratedExportOptions,
    samples: &[super::TrainingSample],
    split_map: &std::collections::HashMap<PathBuf, String>,
    writers: &mut DatasetWriters,
    state: &mut ExportState,
    progress: &mut Option<&mut dyn FnMut(TrainingProgress)>,
) -> Result<(), String> {
    for sample in samples {
        export_sample(
            options,
            sample,
            split_map,
            writers,
            state,
            progress,
        )?;
    }
    Ok(())
}

fn export_sample(
    options: &CuratedExportOptions,
    sample: &super::TrainingSample,
    split_map: &std::collections::HashMap<PathBuf, String>,
    writers: &mut DatasetWriters,
    state: &mut ExportState,
    progress: &mut Option<&mut dyn FnMut(TrainingProgress)>,
) -> Result<(), String> {
    state.processed += 1;
    let Some(embeddings) = decode_embeddings(options, sample, state, progress)? else {
        return Ok(());
    };
    let split = split_map
        .get(&sample.path)
        .map(|s| s.as_str())
        .unwrap_or("train");
    let rel = rel_path_string(&options.dataset_dir, &sample.path);
    let pack_id = pack_id_for_relpath(&rel, options.pack_depth);
    write_embeddings(
        writers,
        state,
        split,
        &sample.class_id,
        &rel,
        &pack_id,
        options.use_hybrid,
        embeddings,
    )?;
    progress_tick(progress, "embedding", state.processed, state.total, state.skipped);
    Ok(())
}

fn decode_embeddings(
    options: &CuratedExportOptions,
    sample: &super::TrainingSample,
    state: &mut ExportState,
    progress: &mut Option<&mut dyn FnMut(TrainingProgress)>,
) -> Result<Option<Vec<super::embeddings::EmbeddingVariant>>, String> {
    let decoded = match crate::analysis::audio::decode_for_analysis(&sample.path) {
        Ok(decoded) => decoded,
        Err(err) => return Ok(skip_sample(state, progress, err)),
    };
    let embeddings = match build_embedding_variants(&decoded, &options.augmentation, &mut state.rng)
    {
        Ok(values) => values,
        Err(err) => return Ok(skip_sample(state, progress, err)),
    };
    Ok(Some(embeddings))
}

fn skip_sample(
    state: &mut ExportState,
    progress: &mut Option<&mut dyn FnMut(TrainingProgress)>,
    err: String,
) -> Option<Vec<super::embeddings::EmbeddingVariant>> {
    state.skip(err);
    progress_tick(progress, "embedding", state.processed, state.total, state.skipped);
    None
}

fn write_embeddings(
    writers: &mut DatasetWriters,
    state: &mut ExportState,
    split: &str,
    class_id: &str,
    rel: &str,
    pack_id: &str,
    use_hybrid: bool,
    embeddings: Vec<super::embeddings::EmbeddingVariant>,
) -> Result<(), String> {
    for (idx, variant) in embeddings.into_iter().enumerate() {
        let feature_len = feature_len(use_hybrid);
        let row = if use_hybrid {
            let mut combined = variant.embedding;
            combined.extend_from_slice(&variant.light_features);
            combined
        } else {
            variant.embedding
        };
        let record = DatasetSampleRecord::new(
            sample_id_for_variant(rel, idx),
            pack_id.to_string(),
            split.to_string(),
            class_id.to_string(),
            state.offset_bytes,
            feature_len,
        );
        writers.write_record(&record, feature_len, &row)?;
        state.offset_bytes += (feature_len * 4) as u64;
        state.total_exported += 1;
        *state.class_counts.entry(class_id.to_string()).or_insert(0) += 1;
    }
    Ok(())
}

fn rel_path_string(root: &Path, full: &Path) -> String {
    let rel = full.strip_prefix(root).unwrap_or(full);
    rel.to_string_lossy().replace('\\', "/")
}

fn pack_id_for_relpath(rel: &str, pack_depth: usize) -> String {
    let rel_path = Path::new(rel);
    let mut parts = Vec::new();
    if let Some(parent) = rel_path.parent() {
        for component in parent.components() {
            if let std::path::Component::Normal(os) = component {
                if let Some(value) = os.to_str() {
                    parts.push(value.to_string());
                }
            }
            if parts.len() >= pack_depth {
                break;
            }
        }
    }
    if parts.is_empty() {
        "curated".to_string()
    } else {
        format!("curated/{}", parts.join("/"))
    }
}

fn sample_id_for_variant(rel: &str, variant_idx: usize) -> String {
    if variant_idx == 0 {
        format!("curated::{rel}")
    } else {
        format!("curated::{rel}#aug{variant_idx}")
    }
}

fn write_manifest(out_dir: &Path, feature_len: usize) -> Result<(), String> {
    let manifest = DatasetManifest {
        format_version: DATASET_FORMAT_VERSION,
        feature_encoding: "f32le".to_string(),
        feat_version: 0,
        feature_len_f32: feature_len,
        files: DatasetManifestFiles {
            samples: SAMPLES_FILE_NAME.to_string(),
            features: FEATURES_FILE_NAME.to_string(),
        },
    };
    let bytes = serde_json::to_vec_pretty(&manifest).map_err(|err| err.to_string())?;
    let path = out_dir.join(MANIFEST_FILE_NAME);
    std::fs::write(path, bytes).map_err(|err| err.to_string())
}

fn build_summary(
    state: ExportState,
    total_samples: usize,
    total_classes: usize,
) -> CuratedExportSummary {
    CuratedExportSummary {
        total_exported: state.total_exported,
        total_samples,
        total_classes,
        skipped: state.skipped,
        class_counts: state.class_counts,
    }
}

fn feature_len(use_hybrid: bool) -> usize {
    if use_hybrid {
        EMBEDDING_DIM + crate::analysis::LIGHT_DSP_VECTOR_LEN
    } else {
        EMBEDDING_DIM
    }
}

struct ExportState {
    total: usize,
    processed: usize,
    skipped: usize,
    total_exported: usize,
    offset_bytes: u64,
    class_counts: BTreeMap<String, usize>,
    skipped_errors: Vec<String>,
    rng: rand::rngs::StdRng,
}

impl ExportState {
    fn new(options: &CuratedExportOptions, total: usize) -> Self {
        Self {
            total,
            processed: 0,
            skipped: 0,
            total_exported: 0,
            offset_bytes: 0,
            class_counts: BTreeMap::new(),
            skipped_errors: Vec::new(),
            rng: augment_rng(&options.augmentation, seed_u64(&options.seed)),
        }
    }

    fn skip(&mut self, err: String) {
        self.skipped += 1;
        if self.skipped_errors.len() < 3 {
            self.skipped_errors.push(err);
        }
    }

    fn warn_if_skipped(&self) {
        if self.skipped > 0 {
            warn!(
                "Skipped {} samples during curated export; first errors: {:?}",
                self.skipped, self.skipped_errors
            );
        }
    }
}

fn seed_u64(seed: &str) -> u64 {
    let hash = blake3::hash(seed.as_bytes());
    u64::from_le_bytes(hash.as_bytes()[0..8].try_into().expect("slice size verified"))
}

struct DatasetWriters {
    features: BufWriter<File>,
    samples: BufWriter<File>,
}

impl DatasetWriters {
    fn new(out_dir: &Path) -> Result<Self, String> {
        let features = File::create(out_dir.join(FEATURES_FILE_NAME)).map_err(|err| err.to_string())?;
        let samples = File::create(out_dir.join(SAMPLES_FILE_NAME)).map_err(|err| err.to_string())?;
        Ok(Self {
            features: BufWriter::new(features),
            samples: BufWriter::new(samples),
        })
    }

    fn write_record(
        &mut self,
        record: &DatasetSampleRecord,
        feature_len: usize,
        embedding: &[f32],
    ) -> Result<(), String> {
        if embedding.len() != feature_len {
            return Err("Unexpected feature length".to_string());
        }
        for value in embedding {
            self.features
                .write_all(&value.to_le_bytes())
                .map_err(|err| err.to_string())?;
        }
        serde_json::to_writer(&mut self.samples, record).map_err(|err| err.to_string())?;
        self.samples
            .write_all(b"\n")
            .map_err(|err| err.to_string())?;
        Ok(())
    }

    fn finish(&mut self) -> Result<(), String> {
        self.features.flush().map_err(|err| err.to_string())?;
        self.samples.flush().map_err(|err| err.to_string())?;
        Ok(())
    }
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

#[derive(Debug, Clone, Serialize)]
struct DatasetFeaturesRef {
    feat_version: i64,
    offset_bytes: u64,
    len_f32: usize,
    encoding: String,
}

#[derive(Debug, Clone, Serialize)]
struct DatasetLabel {
    class_id: String,
    confidence: f32,
    rule_id: String,
    ruleset_version: i64,
}

#[derive(Debug, Clone, Serialize)]
struct DatasetSampleRecord {
    sample_id: String,
    pack_id: String,
    split: String,
    label: DatasetLabel,
    features: DatasetFeaturesRef,
}

impl DatasetSampleRecord {
    fn new(
        sample_id: String,
        pack_id: String,
        split: String,
        class_id: String,
        offset_bytes: u64,
        feature_len: usize,
    ) -> Self {
        Self {
            sample_id,
            pack_id,
            split,
            label: DatasetLabel {
                class_id,
                confidence: 1.0,
                rule_id: "curated".to_string(),
                ruleset_version: RULESET_VERSION,
            },
            features: DatasetFeaturesRef {
                feat_version: 0,
                offset_bytes,
                len_f32: feature_len,
                encoding: "f32le".to_string(),
            },
        }
    }
}
