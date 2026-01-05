//! Descriptor-based similarity embedding helpers.

use super::vector::FEATURE_VECTOR_LEN_V1;

/// Model identifier for similarity embeddings derived from DSP features.
pub const SIMILARITY_MODEL_ID: &str = "features_v1__len183__l2";
/// Output dimension for similarity embeddings.
pub const SIMILARITY_DIM: usize = FEATURE_VECTOR_LEN_V1;
/// Data type label for stored similarity embeddings.
pub const SIMILARITY_DTYPE_F32: &str = "f32";
/// Default analysis batch size for similarity feature extraction.
pub const SIMILARITY_BATCH_MAX: usize = 8;

/// Normalize a vector in-place and return whether the norm was non-zero.
pub fn normalize_l2_in_place(values: &mut [f32]) -> bool {
    let mut sum = 0.0_f32;
    for value in values.iter() {
        sum += value * value;
    }
    if !sum.is_finite() || sum <= 0.0 {
        return false;
    }
    let norm = sum.sqrt();
    if norm <= 0.0 {
        return false;
    }
    for value in values {
        *value /= norm;
    }
    true
}

/// Build a normalized similarity embedding from a feature vector.
pub fn embedding_from_features(features: &[f32]) -> Result<Vec<f32>, String> {
    if features.len() != SIMILARITY_DIM {
        return Err(format!(
            "Similarity features length mismatch: expected {SIMILARITY_DIM}, got {}",
            features.len()
        ));
    }
    let mut embedding = features.to_vec();
    normalize_l2_in_place(&mut embedding);
    Ok(embedding)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedding_from_features_normalizes_l2() {
        let mut features = vec![0.0_f32; SIMILARITY_DIM];
        features[0] = 3.0;
        features[1] = 4.0;
        let embedding = embedding_from_features(&features).expect("embedding");
        let norm = embedding.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5);
    }

    #[test]
    fn embedding_from_features_rejects_wrong_length() {
        let err = embedding_from_features(&[0.0_f32; 3]).unwrap_err();
        assert!(err.to_ascii_lowercase().contains("length mismatch"));
    }
}
