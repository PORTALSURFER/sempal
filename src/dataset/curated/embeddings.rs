use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::sample_sources::config::TrainingAugmentation;

/// Embedding + lightweight feature vector derived from an audio sample.
#[derive(Debug, Clone)]
pub(super) struct EmbeddingVariant {
    pub(super) embedding: Vec<f32>,
    pub(super) light_features: Vec<f32>,
}

const EMBEDDING_CACHE_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
struct CachedEmbedding {
    version: u32,
    model_id: String,
    preprocess: bool,
    modified_ns: u64,
    file_len: u64,
    embedding: Vec<f32>,
    light_features: Vec<f32>,
}

/// Build a deterministic augmentation RNG for curated dataset ingestion.
pub(super) fn augment_rng(
    augmentation: &TrainingAugmentation,
    seed: u64,
) -> rand::rngs::StdRng {
    crate::analysis::augment::AugmentOptions {
        enabled: augmentation.enabled,
        copies_per_sample: augmentation.copies_per_sample,
        gain_jitter_db: augmentation.gain_jitter_db,
        noise_std: augmentation.noise_std,
        pitch_semitones: augmentation.pitch_semitones,
        time_stretch_pct: augmentation.time_stretch_pct,
        seed,
    }
    .rng()
}

/// Build embedding variants (base + optional augmentations) for a decoded sample.
pub(super) fn build_embedding_variants(
    sample_path: &Path,
    decoded: &crate::analysis::audio::AnalysisAudio,
    augmentation: &TrainingAugmentation,
    rng: &mut rand::rngs::StdRng,
    cache_dir: Option<&Path>,
) -> Result<Vec<EmbeddingVariant>, String> {
    let mut variants = Vec::new();
    let cached = if let Some(cache_dir) = cache_dir {
        load_cached_embedding(sample_path, cache_dir, augmentation.preprocess)?
    } else {
        None
    };
    if let Some(cached) = cached {
        variants.push(cached);
    } else {
        let base_samples = if augmentation.preprocess {
            crate::analysis::audio::preprocess_mono_for_embedding(
                &decoded.mono,
                decoded.sample_rate_used,
            )
        } else {
            decoded.mono.clone()
        };
        let base =
            crate::analysis::embedding::infer_embedding(&base_samples, decoded.sample_rate_used)?;
        let base_light = time_domain_vector(&base_samples, decoded.sample_rate_used);
        let variant = EmbeddingVariant {
            embedding: base,
            light_features: base_light,
        };
        if let Some(cache_dir) = cache_dir {
            let _ = write_cached_embedding(
                sample_path,
                cache_dir,
                augmentation.preprocess,
                &variant,
            );
        }
        variants.push(variant);
    }

    if augmentation.enabled && augmentation.copies_per_sample > 0 {
        let options = crate::analysis::augment::AugmentOptions {
            enabled: true,
            copies_per_sample: augmentation.copies_per_sample,
            gain_jitter_db: augmentation.gain_jitter_db,
            noise_std: augmentation.noise_std,
            pitch_semitones: augmentation.pitch_semitones,
            time_stretch_pct: augmentation.time_stretch_pct,
            seed: 0,
        };
        for _ in 0..augmentation.copies_per_sample {
            let augmented = crate::analysis::augment::augment_waveform(
                &decoded.mono,
                rng,
                &options,
            );
            let processed = if augmentation.preprocess {
                crate::analysis::audio::preprocess_mono_for_embedding(
                    &augmented,
                    decoded.sample_rate_used,
                )
            } else {
                augmented
            };
            let embedding = crate::analysis::embedding::infer_embedding(
                &processed,
                decoded.sample_rate_used,
            )?;
            let light = time_domain_vector(&processed, decoded.sample_rate_used);
            variants.push(EmbeddingVariant {
                embedding,
                light_features: light,
            });
        }
    }

    Ok(variants)
}

fn time_domain_vector(samples: &[f32], sample_rate: u32) -> Vec<f32> {
    let feats = crate::analysis::time_domain::extract_time_domain_features(samples, sample_rate);
    vec![
        feats.duration_seconds,
        feats.peak,
        feats.rms,
        feats.crest_factor,
        feats.zero_crossing_rate,
        feats.attack_seconds,
        feats.decay_20db_seconds,
        feats.decay_40db_seconds,
        feats.onset_count as f32,
    ]
}

fn load_cached_embedding(
    sample_path: &Path,
    cache_dir: &Path,
    preprocess: bool,
) -> Result<Option<EmbeddingVariant>, String> {
    let cache_path = cache_path_for_sample(cache_dir, sample_path, preprocess);
    if !cache_path.is_file() {
        return Ok(None);
    }
    let bytes = match std::fs::read(&cache_path) {
        Ok(bytes) => bytes,
        Err(_) => return Ok(None),
    };
    let entry: CachedEmbedding = match serde_json::from_slice(&bytes) {
        Ok(entry) => entry,
        Err(_) => return Ok(None),
    };
    if !cache_entry_matches(sample_path, preprocess, &entry) {
        return Ok(None);
    }
    Ok(Some(EmbeddingVariant {
        embedding: entry.embedding,
        light_features: entry.light_features,
    }))
}

fn write_cached_embedding(
    sample_path: &Path,
    cache_dir: &Path,
    preprocess: bool,
    variant: &EmbeddingVariant,
) -> Result<(), String> {
    let metadata = match std::fs::metadata(sample_path) {
        Ok(metadata) => metadata,
        Err(_) => return Ok(()),
    };
    let modified_ns = modified_ns(&metadata);
    let file_len = metadata.len();
    std::fs::create_dir_all(cache_dir).map_err(|err| err.to_string())?;
    let entry = CachedEmbedding {
        version: EMBEDDING_CACHE_VERSION,
        model_id: crate::analysis::embedding::EMBEDDING_MODEL_ID.to_string(),
        preprocess,
        modified_ns,
        file_len,
        embedding: variant.embedding.clone(),
        light_features: variant.light_features.clone(),
    };
    let payload = serde_json::to_vec(&entry).map_err(|err| err.to_string())?;
    let cache_path = cache_path_for_sample(cache_dir, sample_path, preprocess);
    std::fs::write(cache_path, payload).map_err(|err| err.to_string())?;
    Ok(())
}

fn cache_entry_matches(sample_path: &Path, preprocess: bool, entry: &CachedEmbedding) -> bool {
    if entry.version != EMBEDDING_CACHE_VERSION {
        return false;
    }
    if entry.model_id != crate::analysis::embedding::EMBEDDING_MODEL_ID {
        return false;
    }
    if entry.preprocess != preprocess {
        return false;
    }
    if entry.embedding.len() != crate::analysis::embedding::EMBEDDING_DIM {
        return false;
    }
    if entry.light_features.len() != crate::analysis::LIGHT_DSP_VECTOR_LEN {
        return false;
    }
    let metadata = match std::fs::metadata(sample_path) {
        Ok(metadata) => metadata,
        Err(_) => return false,
    };
    if metadata.len() != entry.file_len {
        return false;
    }
    entry.modified_ns == modified_ns(&metadata)
}

fn cache_path_for_sample(cache_dir: &Path, sample_path: &Path, preprocess: bool) -> PathBuf {
    let key = format!(
        "{}|{}",
        sample_path.to_string_lossy(),
        if preprocess { "pre" } else { "raw" }
    );
    let hash = blake3::hash(key.as_bytes());
    cache_dir.join(format!("{}.json", hash.to_hex()))
}

fn modified_ns(metadata: &std::fs::Metadata) -> u64 {
    let modified = match metadata.modified() {
        Ok(value) => value,
        Err(_) => return 0,
    };
    let duration = match modified.duration_since(std::time::UNIX_EPOCH) {
        Ok(value) => value,
        Err(_) => return 0,
    };
    duration
        .as_nanos()
        .try_into()
        .unwrap_or(u64::MAX)
}
