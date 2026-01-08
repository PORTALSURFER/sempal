//! Embedding inference entry points and orchestration.

use super::EMBEDDING_DIM;
use super::logmel::PANNS_LOGMEL_LEN;
use super::model::with_panns_model;
use super::query::query_window_ranges;

mod backend_io;
mod preprocess;

pub(in crate::analysis::embedding) use backend_io::run_panns_inference_for_model;

/// Run PANNs inference for a single audio buffer.
pub(crate) fn infer_embedding(samples: &[f32], sample_rate: u32) -> Result<Vec<f32>, String> {
    if samples.is_empty() {
        return Err("PANNs inference requires non-empty samples".into());
    }
    with_panns_model(|model| preprocess::infer_embedding_with_model(model, samples, sample_rate))
}

/// Run PANNs inference over sliding windows and average the embeddings.
pub(crate) fn infer_embedding_query(samples: &[f32], sample_rate: u32) -> Result<Vec<f32>, String> {
    if samples.is_empty() {
        return Err("PANNs inference requires non-empty samples".into());
    }
    let ranges = query_window_ranges(samples.len(), sample_rate);
    if ranges.len() <= 1 {
        return infer_embedding(samples, sample_rate);
    }
    with_panns_model(|model| {
        let count = ranges.len().max(1) as f32;
        let mut sum = vec![0.0_f32; EMBEDDING_DIM];
        for (start, end) in ranges {
            let embedding =
                preprocess::infer_embedding_with_model(model, &samples[start..end], sample_rate)?;
            for (acc, value) in sum.iter_mut().zip(embedding.iter()) {
                *acc += value;
            }
        }
        let scale = 1.0 / count;
        for value in &mut sum {
            *value *= scale;
        }
        backend_io::normalize_l2_in_place(&mut sum);
        Ok(sum)
    })
}

/// Build a log-mel buffer and run PANNs inference for it.
pub(crate) fn infer_embedding_from_logmel(logmel: &[f32]) -> Result<Vec<f32>, String> {
    if logmel.len() != PANNS_LOGMEL_LEN {
        return Err(format!(
            "PANNs log-mel buffer has wrong length: expected {PANNS_LOGMEL_LEN}, got {}",
            logmel.len()
        ));
    }
    with_panns_model(|model| {
        let mut embeddings = backend_io::run_panns_inference_for_model(model, logmel, 1)?;
        embeddings
            .pop()
            .ok_or_else(|| "PANNs embedding output missing".to_string())
    })
}
