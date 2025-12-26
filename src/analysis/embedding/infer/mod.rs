//! Embedding inference entry points and orchestration.

use burn::tensor::TensorData;

use super::backend::{embedding_batch_max, panns_batch_enabled};
use super::logmel::{PANNS_INPUT_FRAMES, PANNS_LOGMEL_LEN};
use super::model::{with_panns_model, PannsModelInner};
use super::query::query_window_ranges;
use super::EMBEDDING_DIM;
use crate::analysis::panns_preprocess::PANNS_MEL_BANDS;

mod backend_io;
mod preprocess;
mod schedule;

pub(in crate::analysis::embedding) use backend_io::run_panns_inference_for_model;

/// Input metadata for batch embedding inference.
pub(crate) struct EmbeddingBatchInput<'a> {
    pub(crate) samples: &'a [f32],
    pub(crate) sample_rate: u32,
}

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

/// Run PANNs inference for a batch of audio buffers.
pub(crate) fn infer_embeddings_batch(
    inputs: &[EmbeddingBatchInput<'_>],
) -> Result<Vec<Vec<f32>>, String> {
    if inputs.is_empty() {
        return Ok(Vec::new());
    }
    let batch_enabled = panns_batch_enabled();
    let batch_plan = schedule::plan_batch_slices(
        inputs.len(),
        embedding_batch_max(),
        batch_enabled,
    );
    with_panns_model(|model| {
        let mut outputs = Vec::with_capacity(inputs.len());
        for plan in batch_plan {
            let chunk = &inputs[plan.start..plan.end()];
            if !batch_enabled {
                for input in chunk {
                    outputs.push(preprocess::infer_embedding_with_model(
                        model,
                        input.samples,
                        input.sample_rate,
                    )?);
                }
                continue;
            }
            let embeddings = preprocess::infer_embeddings_with_model(model, chunk)?;
            outputs.extend(embeddings);
        }
        Ok(outputs)
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

/// Run PANNs inference for a flattened log-mel batch.
pub(crate) fn infer_embeddings_from_logmel_batch(
    logmel: Vec<f32>,
    batch: usize,
) -> Result<Vec<Vec<f32>>, String> {
    if batch == 0 {
        return Ok(Vec::new());
    }
    let expected = PANNS_LOGMEL_LEN.saturating_mul(batch);
    if logmel.len() != expected {
        return Err(format!(
            "PANNs log-mel batch buffer has wrong length: expected {expected}, got {}",
            logmel.len()
        ));
    }
    with_panns_model(|model| {
        let data = TensorData::new(logmel, [batch, 1, PANNS_INPUT_FRAMES, PANNS_MEL_BANDS]);
        backend_io::run_panns_inference_from_data_for_model(model, data, batch)
    })
}

/// Run PANNs inference for log-mel inputs with manual chunking.
pub(crate) fn infer_embeddings_from_logmel_batch_chunked(
    logmels: &[Vec<f32>],
    micro_batch: usize,
) -> Vec<Result<Vec<f32>, String>> {
    if logmels.is_empty() {
        return Vec::new();
    }
    let expected = PANNS_LOGMEL_LEN;
    if logmels.iter().any(|item| item.len() != expected) {
        let err = format!("PANNs log-mel buffer has wrong length: expected {expected}");
        return logmels.iter().map(|_| Err(err.clone())).collect();
    }
    let mut outputs = Vec::with_capacity(logmels.len());
    for slice in schedule::chunk_ranges(logmels.len(), micro_batch.max(1)) {
        let chunk = &logmels[slice.start..slice.end()];
        let mut batch_input = Vec::with_capacity(chunk.len() * PANNS_LOGMEL_LEN);
        for logmel in chunk {
            batch_input.extend_from_slice(logmel.as_slice());
        }
        match infer_embeddings_from_logmel_batch(batch_input, chunk.len()) {
            Ok(embeddings) => outputs.extend(embeddings.into_iter().map(Ok)),
            Err(err) => outputs.extend(chunk.iter().map(|_| Err(err.clone()))),
        }
    }
    outputs
}

/// Run PANNs inference for log-mel inputs with pipelined GPU readback.
pub(crate) fn infer_embeddings_from_logmel_batch_pipelined(
    logmels: &[Vec<f32>],
    micro_batch: usize,
    inflight: usize,
) -> Vec<Result<Vec<f32>, String>> {
    if logmels.is_empty() {
        return Vec::new();
    }
    let expected = PANNS_LOGMEL_LEN;
    if logmels.iter().any(|item| item.len() != expected) {
        let err = format!("PANNs log-mel buffer has wrong length: expected {expected}");
        return logmels.iter().map(|_| Err(err.clone())).collect();
    }
    let result = with_panns_model(|model| match &model.inner {
        PannsModelInner::Wgpu { model, device } => Ok(
            backend_io::infer_embeddings_from_logmel_batch_pipelined_with_backend(
                model,
                device,
                logmels,
                micro_batch,
                inflight,
            ),
        ),
        #[cfg(feature = "panns-cuda")]
        PannsModelInner::Cuda { model, device } => Ok(
            backend_io::infer_embeddings_from_logmel_batch_pipelined_with_backend(
                model,
                device,
                logmels,
                micro_batch,
                inflight,
            ),
        ),
    });
    match result {
        Ok(results) => results,
        Err(err) => logmels.iter().map(|_| Err(err.clone())).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::schedule::{chunk_ranges, plan_batch_slices};

    #[test]
    fn batch_plan_respects_max_batch() {
        let plan = plan_batch_slices(10, 4, true);
        let sizes: Vec<usize> = plan.iter().map(|slice| slice.len).collect();
        assert_eq!(sizes, vec![4, 4, 2]);
    }

    #[test]
    fn batch_plan_falls_back_to_singletons() {
        let plan = plan_batch_slices(3, 8, false);
        let sizes: Vec<usize> = plan.iter().map(|slice| slice.len).collect();
        assert_eq!(sizes, vec![1, 1, 1]);
    }

    #[test]
    fn chunk_ranges_cover_all_items() {
        let plan = chunk_ranges(5, 2);
        let sizes: Vec<usize> = plan.iter().map(|slice| slice.len).collect();
        assert_eq!(sizes, vec![2, 2, 1]);
    }
}
