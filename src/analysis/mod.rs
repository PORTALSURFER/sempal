//! Background analysis helpers (decoding, normalization, feature extraction).

pub(crate) mod audio;
pub(crate) mod audio_decode;
pub(crate) mod augment;
pub(crate) mod ann_index;
pub(crate) mod anchor_match;
pub(crate) mod anchor_scoring;
pub(crate) mod clap_preprocess;
pub mod embedding;
pub(crate) mod features;
pub(crate) mod fft;
pub(crate) mod frequency_domain;
pub(crate) mod time_domain;
pub(crate) mod vector;
pub(crate) mod version;

pub use vector::{FEATURE_VECTOR_LEN_V1, FEATURE_VERSION_V1};
pub use vector::decode_f32_le_blob;

use std::path::Path;
use rusqlite::Connection;

/// Lightweight DSP vector length (time-domain features only).
pub const LIGHT_DSP_VECTOR_LEN: usize = 9;

/// Decode an audio file and compute the V1 feature vector used by the analyzer.
pub fn compute_feature_vector_v1_for_path(path: &Path) -> Result<Vec<f32>, String> {
    let decoded = audio::decode_for_analysis(path)?;
    let time_domain = time_domain::extract_time_domain_features(&decoded.mono, decoded.sample_rate_used);
    let frequency_domain =
        frequency_domain::extract_frequency_domain_features(&decoded.mono, decoded.sample_rate_used);
    let features = features::AnalysisFeaturesV1::new(time_domain, frequency_domain);
    Ok(vector::to_f32_vector_v1(&features))
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
