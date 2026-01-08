use burn::tensor::backend::Backend;
use burn::tensor::{Tensor, TensorData};

use super::super::EMBEDDING_DIM;
use super::super::logmel::PANNS_INPUT_FRAMES;
use super::super::model::{PannsModel, PannsModelInner};
use super::super::panns_burn;
use crate::analysis::panns_preprocess::PANNS_MEL_BANDS;

pub(in crate::analysis::embedding) fn run_panns_inference_for_model(
    model: &PannsModel,
    input: &[f32],
    batch: usize,
) -> Result<Vec<Vec<f32>>, String> {
    match &model.inner {
        PannsModelInner::Wgpu { model, device } => run_panns_inference(model, device, input, batch),
        PannsModelInner::Cpu { model, device } => run_panns_inference(model, device, input, batch),
        #[cfg(feature = "panns-cuda")]
        PannsModelInner::Cuda { model, device } => run_panns_inference(model, device, input, batch),
    }
}

fn run_panns_inference<B: Backend>(
    model: &panns_burn::Model<B>,
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
    model: &panns_burn::Model<B>,
    device: &B::Device,
    data: TensorData,
    batch: usize,
) -> Result<Vec<Vec<f32>>, String> {
    let output = run_panns_forward_from_data(model, device, data);
    extract_embeddings_from_data(output.into_data(), batch)
}

fn run_panns_forward_from_data<B: Backend>(
    model: &panns_burn::Model<B>,
    device: &B::Device,
    data: TensorData,
) -> Tensor<B, 2> {
    let input_tensor = Tensor::<B, 4>::from_data(data, device);
    model.forward(input_tensor)
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

pub(super) fn normalize_l2_in_place(values: &mut [f32]) {
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
