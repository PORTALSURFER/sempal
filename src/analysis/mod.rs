//! Background analysis helpers (decoding, normalization, feature extraction).

pub mod ann_index;
pub(crate) mod audio;
pub(crate) mod audio_decode;
#[allow(dead_code)]
pub(crate) mod augment;
pub mod embedding;
pub(crate) mod features;
pub(crate) mod fft;
pub(crate) mod frequency_domain;
pub mod hdbscan;
#[allow(dead_code)]
pub(crate) mod panns_preprocess;
pub mod similarity;
pub(crate) mod time_domain;
pub mod umap;
pub mod vector;
pub(crate) mod version;

pub use vector::decode_f32_le_blob;
pub use vector::{FEATURE_VECTOR_LEN_V1, FEATURE_VERSION_V1};

use rusqlite::Connection;
use std::path::Path;

/// Lightweight DSP vector length (time-domain features only).
pub const LIGHT_DSP_VECTOR_LEN: usize = 9;

/// Decode an audio file and compute the V1 feature vector used by the analyzer.
pub fn compute_feature_vector_v1_for_path(path: &Path) -> Result<Vec<f32>, String> {
    let decoded = audio::decode_for_analysis(path)?;
    let time_domain =
        time_domain::extract_time_domain_features(&decoded.mono, decoded.sample_rate_used);
    let frequency_domain = frequency_domain::extract_frequency_domain_features(
        &decoded.mono,
        decoded.sample_rate_used,
    );
    let features = features::AnalysisFeaturesV1::new(time_domain, frequency_domain);
    Ok(vector::to_f32_vector_v1(&features))
}

/// Compute the similarity embedding for a file path using V1 DSP features.
pub fn compute_similarity_embedding_for_path(path: &Path) -> Result<Vec<f32>, String> {
    let features = compute_feature_vector_v1_for_path(path)?;
    similarity::embedding_from_features(&features)
}

/// Compute the V1 feature vector from mono samples without decoding from disk.
pub fn compute_feature_vector_v1_for_mono_samples(
    samples: &[f32],
    sample_rate: u32,
) -> Result<Vec<f32>, String> {
    if sample_rate == 0 {
        return Err("Sample rate must be greater than 0".to_string());
    }
    if samples.is_empty() {
        return Err("No samples provided".to_string());
    }
    let mut mono = samples.to_vec();
    audio::sanitize_samples_in_place(&mut mono);
    let prepared = audio::prepare_mono_for_analysis(mono, sample_rate);
    let time_domain =
        time_domain::extract_time_domain_features(&prepared.mono, prepared.sample_rate_used);
    let frequency_domain = frequency_domain::extract_frequency_domain_features(
        &prepared.mono,
        prepared.sample_rate_used,
    );
    let features = features::AnalysisFeaturesV1::new(time_domain, frequency_domain);
    Ok(vector::to_f32_vector_v1(&features))
}

/// Compute the similarity embedding from mono samples using V1 DSP features.
pub fn compute_similarity_embedding_for_mono_samples(
    samples: &[f32],
    sample_rate: u32,
) -> Result<Vec<f32>, String> {
    let features = compute_feature_vector_v1_for_mono_samples(samples, sample_rate)?;
    similarity::embedding_from_features(&features)
}

/// Preprocess mono audio for embedding inference (silence trim + normalization).
pub fn preprocess_mono_for_embedding(samples: &[f32], sample_rate: u32) -> Vec<f32> {
    audio::preprocess_mono_for_embedding(samples, sample_rate)
}

/// Infer the embedding for a mono sample buffer.
pub fn infer_embedding(samples: &[f32], sample_rate: u32) -> Result<Vec<f32>, String> {
    embedding::infer_embedding(samples, sample_rate)
}

/// Extract the lightweight DSP vector from a full V1 feature vector.
pub fn light_dsp_from_features_v1(features: &[f32]) -> Option<Vec<f32>> {
    if features.len() < LIGHT_DSP_VECTOR_LEN {
        return None;
    }
    Some(features[..LIGHT_DSP_VECTOR_LEN].to_vec())
}

/// Rebuild the ANN index from embeddings in the library database.
pub fn rebuild_ann_index(conn: &Connection) -> Result<(), String> {
    ann_index::rebuild_index(conn)
}

/// Flush any pending ANN insertions without forcing a rebuild.
pub fn flush_ann_index(conn: &Connection) -> Result<(), String> {
    ann_index::flush_pending_inserts(conn)
}

#[cfg(test)]
mod ann_index_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use hound::{SampleFormat, WavSpec, WavWriter};
    use tempfile::tempdir;

    #[test]
    fn computes_feature_vector_v1_for_wav() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.wav");
        let spec = WavSpec {
            channels: 1,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut writer = WavWriter::create(&path, spec).unwrap();
        for i in 0..44_100 {
            let t = i as f32 / 44_100.0;
            let sample = (t * 440.0 * std::f32::consts::TAU).sin() * 0.5;
            let sample_i16 = (sample * i16::MAX as f32) as i16;
            writer.write_sample(sample_i16).unwrap();
        }
        writer.finalize().unwrap();

        let vec = compute_feature_vector_v1_for_path(&path).unwrap();
        assert_eq!(vec.len(), FEATURE_VECTOR_LEN_V1);
    }
}
