use burn::tensor::backend::Backend;
use burn::tensor::{Tensor, TensorData};

use super::backend::{embedding_batch_max, embedding_pipeline_enabled, panns_batch_enabled};
use super::logmel::{prepare_panns_logmel, PANNS_INPUT_FRAMES, PANNS_LOGMEL_LEN};
use super::model::{with_panns_model, PannsModel, PannsModelInner};
use super::query::query_window_ranges;
use super::EMBEDDING_DIM;
use crate::analysis::panns_preprocess::PANNS_MEL_BANDS;

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
    with_panns_model(|model| infer_embedding_with_model(model, samples, sample_rate))
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
            let embedding = infer_embedding_with_model(model, &samples[start..end], sample_rate)?;
            for (acc, value) in sum.iter_mut().zip(embedding.iter()) {
                *acc += value;
            }
        }
        let scale = 1.0 / count;
        for value in &mut sum {
            *value *= scale;
        }
        normalize_l2_in_place(&mut sum);
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
    if !panns_batch_enabled() {
        return with_panns_model(|model| {
            let mut outputs = Vec::with_capacity(inputs.len());
            for input in inputs {
                outputs.push(infer_embedding_with_model(model, input.samples, input.sample_rate)?);
            }
            Ok(outputs)
        });
    }
    with_panns_model(|model| {
        let mut outputs = Vec::with_capacity(inputs.len());
        for chunk in inputs.chunks(embedding_batch_max()) {
            let embeddings = infer_embeddings_with_model(model, chunk)?;
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
        let mut embeddings = run_panns_inference_for_model(model, logmel, 1)?;
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
        run_panns_inference_from_data_for_model(model, data, batch)
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
    let micro_batch = micro_batch.max(1);
    let mut outputs = Vec::with_capacity(logmels.len());
    for chunk in logmels.chunks(micro_batch) {
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
            infer_embeddings_from_logmel_batch_pipelined_with_backend(
                model,
                device,
                logmels,
                micro_batch,
                inflight,
            ),
        ),
        #[cfg(feature = "panns-cuda")]
        PannsModelInner::Cuda { model, device } => Ok(
            infer_embeddings_from_logmel_batch_pipelined_with_backend(
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

fn infer_embeddings_from_logmel_batch_pipelined_with_backend<B: Backend>(
    model: &super::panns_burn::Model<B>,
    device: &B::Device,
    logmels: &[Vec<f32>],
    micro_batch: usize,
    inflight: usize,
) -> Vec<Result<Vec<f32>, String>> {
    let micro_batch = micro_batch.max(1);
    let inflight = inflight.max(1);
    let results = std::sync::Arc::new(std::sync::Mutex::new(vec![None; logmels.len()]));
    let errors = std::sync::Arc::new(std::sync::Mutex::new(vec![None; logmels.len()]));
    let (tx, rx) = std::sync::mpsc::sync_channel::<(usize, usize, Tensor<B, 2>)>(inflight);
    let results_handle = std::sync::Arc::clone(&results);
    let errors_handle = std::sync::Arc::clone(&errors);
    let readback = std::thread::spawn(move || {
        while let Ok((offset, batch, output)) = rx.recv() {
            match extract_embeddings_from_data(output.into_data(), batch) {
                Ok(embeddings) => {
                    let mut guard = results_handle.lock().unwrap_or_else(|err| err.into_inner());
                    for (idx, embedding) in embeddings.into_iter().enumerate() {
                        if let Some(slot) = guard.get_mut(offset + idx) {
                            *slot = Some(embedding);
                        }
                    }
                }
                Err(err) => {
                    let mut guard = errors_handle.lock().unwrap_or_else(|err| err.into_inner());
                    for idx in 0..batch {
                        if let Some(slot) = guard.get_mut(offset + idx) {
                            *slot = Some(err.clone());
                        }
                    }
                }
            }
        }
    });
    let submit_tx = tx.clone();
    let mut submit_error = None;
    for (offset, chunk) in logmels.chunks(micro_batch).enumerate() {
        let start = offset * micro_batch;
        let mut batch_input = Vec::with_capacity(chunk.len() * PANNS_LOGMEL_LEN);
        for logmel in chunk {
            batch_input.extend_from_slice(logmel.as_slice());
        }
        let data = TensorData::new(
            batch_input,
            [chunk.len(), 1, PANNS_INPUT_FRAMES, PANNS_MEL_BANDS],
        );
        let output = run_panns_forward_from_data(model, device, data);
        if submit_tx.send((start, chunk.len(), output)).is_err() {
            submit_error = Some("PANNs readback channel closed".to_string());
            break;
        }
    }
    drop(tx);
    let _ = readback.join();
    if let Some(err) = submit_error {
        let mut guard = errors.lock().unwrap_or_else(|err| err.into_inner());
        for slot in guard.iter_mut() {
            if slot.is_none() {
                *slot = Some(err.clone());
            }
        }
    }
    let guard = results.lock().unwrap_or_else(|err| err.into_inner());
    let err_guard = errors.lock().unwrap_or_else(|err| err.into_inner());
    guard
        .iter()
        .zip(err_guard.iter())
        .map(|(value, err)| {
            if let Some(err) = err {
                return Err(err.clone());
            }
            value
                .clone()
                .ok_or_else(|| "PANNs embedding output missing".to_string())
        })
        .collect()
}

fn infer_embedding_with_model(
    model: &mut PannsModel,
    samples: &[f32],
    sample_rate: u32,
) -> Result<Vec<f32>, String> {
    let input_slice = model.input_scratch.as_mut_slice();
    prepare_panns_logmel(
        &mut model.resample_scratch,
        &mut model.wave_scratch,
        &mut model.preprocess_scratch,
        input_slice,
        samples,
        sample_rate,
    )?;
    let mut embeddings = run_panns_inference_for_model(model, model.input_scratch.as_slice(), 1)?;
    embeddings
        .pop()
        .ok_or_else(|| "PANNs embedding output missing".to_string())
}

fn infer_embeddings_with_model(
    model: &mut PannsModel,
    inputs: &[EmbeddingBatchInput<'_>],
) -> Result<Vec<Vec<f32>>, String> {
    let batch = inputs.len();
    let total_len = batch * PANNS_LOGMEL_LEN;
    model.input_batch_scratch.clear();
    model.input_batch_scratch.resize(total_len, 0.0);
    for (idx, input) in inputs.iter().enumerate() {
        let start = idx * PANNS_LOGMEL_LEN;
        let end = start + PANNS_LOGMEL_LEN;
        let out = &mut model.input_batch_scratch[start..end];
        prepare_panns_logmel(
            &mut model.resample_scratch,
            &mut model.wave_scratch,
            &mut model.preprocess_scratch,
            out,
            input.samples,
            input.sample_rate,
        )?;
    }
    run_panns_inference_for_model(model, model.input_batch_scratch.as_slice(), batch)
}

fn run_panns_inference<B: Backend>(
    model: &super::panns_burn::Model<B>,
    device: &B::Device,
    input: &[f32],
    batch: usize,
) -> Result<Vec<Vec<f32>>, String> {
    let data = TensorData::new(
        input.to_vec(),
        [batch, 1, PANNS_INPUT_FRAMES, PANNS_MEL_BANDS],
    );
    run_panns_inference_from_data(model, device, data, batch)
}

fn run_panns_inference_from_data<B: Backend>(
    model: &super::panns_burn::Model<B>,
    device: &B::Device,
    data: TensorData,
    batch: usize,
) -> Result<Vec<Vec<f32>>, String> {
    let output = run_panns_forward_from_data(model, device, data);
    extract_embeddings_from_data(output.into_data(), batch)
}

fn run_panns_forward_from_data<B: Backend>(
    model: &super::panns_burn::Model<B>,
    device: &B::Device,
    data: TensorData,
) -> Tensor<B, 2> {
    let input_tensor = Tensor::<B, 4>::from_data(data, device);
    model.forward(input_tensor)
}

pub(super) fn run_panns_inference_for_model(
    model: &PannsModel,
    input: &[f32],
    batch: usize,
) -> Result<Vec<Vec<f32>>, String> {
    match &model.inner {
        PannsModelInner::Wgpu { model, device } => run_panns_inference(model, device, input, batch),
        #[cfg(feature = "panns-cuda")]
        PannsModelInner::Cuda { model, device } => run_panns_inference(model, device, input, batch),
    }
}

fn run_panns_inference_from_data_for_model(
    model: &PannsModel,
    data: TensorData,
    batch: usize,
) -> Result<Vec<Vec<f32>>, String> {
    match &model.inner {
        PannsModelInner::Wgpu { model, device } => {
            run_panns_inference_from_data(model, device, data, batch)
        }
        #[cfg(feature = "panns-cuda")]
        PannsModelInner::Cuda { model, device } => {
            run_panns_inference_from_data(model, device, data, batch)
        }
    }
}

fn extract_embeddings_from_data(data: TensorData, batch: usize) -> Result<Vec<Vec<f32>>, String> {
    let shape = data.shape.clone();
    let flat = data
        .as_slice::<f32>()
        .map_err(|err| format!("Failed to read Burn output tensor: {err}"))?;
    if shape.is_empty() {
        return Err("PANNs output tensor has empty shape".to_string());
    }
    if shape.len() == 1 {
        if batch != 1 || flat.len() < EMBEDDING_DIM {
            return Err("PANNs output tensor has unexpected shape".to_string());
        }
        let mut pooled = flat[..EMBEDDING_DIM].to_vec();
        normalize_l2_in_place(&mut pooled);
        let norm = l2_norm(&pooled);
        if !norm.is_finite() || (norm - 1.0).abs() > 1e-3 {
            return Err(format!("PANNs embedding L2 norm out of range: {norm:.6}"));
        }
        return Ok(vec![pooled]);
    }
    let batch_dim = shape[0];
    if batch_dim != batch {
        return Err(format!(
            "PANNs output batch mismatch: expected {batch}, got {batch_dim}"
        ));
    }
    let embedding_dim = *shape.last().unwrap_or(&0);
    if embedding_dim != EMBEDDING_DIM {
        return Err(format!(
            "PANNs output embedding dim mismatch: expected {EMBEDDING_DIM}, got {embedding_dim}"
        ));
    }
    let mut frames_per = 1usize;
    if shape.len() > 2 {
        for dim in &shape[1..shape.len() - 1] {
            frames_per = frames_per.saturating_mul(*dim);
        }
    }
    let expected_len = batch
        .saturating_mul(frames_per)
        .saturating_mul(EMBEDDING_DIM);
    if flat.len() < expected_len {
        return Err("PANNs output tensor shorter than expected".to_string());
    }
    let mut outputs = Vec::with_capacity(batch);
    for batch_idx in 0..batch {
        let mut pooled = vec![0.0_f32; EMBEDDING_DIM];
        let frame_base = batch_idx * frames_per * EMBEDDING_DIM;
        for frame in 0..frames_per {
            let base = frame_base + frame * EMBEDDING_DIM;
            let chunk = &flat[base..base + EMBEDDING_DIM];
            for (idx, value) in chunk.iter().enumerate() {
                pooled[idx] += *value;
            }
        }
        let scale = 1.0 / frames_per.max(1) as f32;
        for value in &mut pooled {
            *value *= scale;
        }
        normalize_l2_in_place(&mut pooled);
        let norm = l2_norm(&pooled);
        if !norm.is_finite() || (norm - 1.0).abs() > 1e-3 {
            return Err(format!("PANNs embedding L2 norm out of range: {norm:.6}"));
        }
        outputs.push(pooled);
    }
    Ok(outputs)
}

fn normalize_l2_in_place(values: &mut [f32]) {
    let mut norm = 0.0_f32;
    for value in values.iter() {
        norm += value * value;
    }
    let norm = norm.sqrt();
    if norm > 0.0 {
        for value in values.iter_mut() {
            *value /= norm;
        }
    }
}

fn l2_norm(values: &[f32]) -> f32 {
    let mut sum = 0.0_f32;
    for value in values {
        sum += value * value;
    }
    sum.sqrt()
}
