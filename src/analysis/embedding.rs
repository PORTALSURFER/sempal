use std::path::PathBuf;
use std::sync::LazyLock;

use tract_tflite::prelude::*;

use crate::analysis::audio;

pub(crate) const EMBEDDING_MODEL_ID: &str = "yamnet_v1";
pub(crate) const EMBEDDING_DIM: usize = 1024;
pub(crate) const EMBEDDING_DTYPE_F32: i64 = 0;
const YAMNET_INPUT_SAMPLES: usize = 15_600;

pub(crate) struct YamnetModel {
    model: SimplePlan<TypedFact, Box<dyn TypedOp>, TypedModel>,
}

impl YamnetModel {
    pub(crate) fn load() -> Result<Self, String> {
        let path = yamnet_model_path()?;
        if !path.exists() {
            return Err(format!(
                "YAMNet model not found at {}",
                path.to_string_lossy()
            ));
        }
        let model = tract_tflite::tflite()
            .model_for_path(&path)
            .map_err(|err| format!("Failed to load YAMNet model: {err}"))?
            .with_input_fact(
                0,
                TypedFact::dt_shape(f32::datum_type(), tvec!(1, YAMNET_INPUT_SAMPLES)),
            )
            .map_err(|err| format!("Failed to set YAMNet input shape: {err}"))?
            .into_optimized()
            .map_err(|err| format!("Failed to optimize YAMNet model: {err}"))?
            .into_runnable()
            .map_err(|err| format!("Failed to make YAMNet runnable: {err}"))?;
        Ok(Self { model })
    }
}

pub(crate) fn infer_embedding(
    cache: &mut Option<YamnetModel>,
    samples: &[f32],
    sample_rate: u32,
) -> Result<Vec<f32>, String> {
    if sample_rate != audio::ANALYSIS_SAMPLE_RATE {
        return Err(format!(
            "YAMNet expects {} Hz input, got {} Hz",
            audio::ANALYSIS_SAMPLE_RATE, sample_rate
        ));
    }
    if samples.is_empty() {
        return Err("YAMNet inference requires non-empty samples".into());
    }
    if cache.is_none() {
        *cache = Some(YamnetModel::load()?);
    }
    let model = cache.as_mut().expect("YAMNet model loaded");

    let mut frames = Vec::new();
    let mut start = 0usize;
    while start < samples.len() {
        let end = start.saturating_add(YAMNET_INPUT_SAMPLES).min(samples.len());
        frames.push(&samples[start..end]);
        start = end;
    }
    if frames.is_empty() {
        return Err("YAMNet input produced no frames".into());
    }

    let mut pooled = vec![0.0_f32; EMBEDDING_DIM];
    let mut pooled_count = 0usize;
    for frame in frames {
        let mut input = vec![0.0_f32; YAMNET_INPUT_SAMPLES];
        let copy_len = frame.len().min(YAMNET_INPUT_SAMPLES);
        input[..copy_len].copy_from_slice(&frame[..copy_len]);
        let tensor = Tensor::from_shape(&[1, YAMNET_INPUT_SAMPLES], &input)
            .map_err(|err| format!("Failed to build YAMNet input tensor: {err}"))?;
        let outputs = model
            .model
            .run(tvec!(tensor.into()))
            .map_err(|err| format!("YAMNet inference failed: {err}"))?;
        let embedding = extract_embedding(&outputs)?;
        if embedding.len() != EMBEDDING_DIM {
            return Err(format!(
                "YAMNet embedding length mismatch: expected {}, got {}",
                EMBEDDING_DIM,
                embedding.len()
            ));
        }
        for (idx, value) in embedding.iter().enumerate() {
            pooled[idx] += *value;
        }
        pooled_count += 1;
    }

    if pooled_count == 0 {
        return Err("YAMNet pooling produced zero frames".into());
    }
    for value in &mut pooled {
        *value /= pooled_count as f32;
    }
    normalize_l2_in_place(&mut pooled);
    Ok(pooled)
}

fn extract_embedding(outputs: &TVec<TValue>) -> Result<Vec<f32>, String> {
    for output in outputs {
        let tensor = output
            .to_tensor()
            .map_err(|err| format!("Failed to read YAMNet output tensor: {err}"))?;
        let shape = tensor.shape();
        if !shape.iter().any(|dim| *dim == EMBEDDING_DIM) {
            continue;
        }
        let view = tensor
            .to_array_view::<f32>()
            .map_err(|err| format!("Failed to read YAMNet output tensor: {err}"))?;
        let slice = view
            .as_slice()
            .ok_or_else(|| "YAMNet output tensor is not contiguous".to_string())?;
        if slice.len() < EMBEDDING_DIM {
            continue;
        }
        let frames = slice.len() / EMBEDDING_DIM;
        if frames == 0 {
            continue;
        }
        let mut pooled = vec![0.0_f32; EMBEDDING_DIM];
        for frame in 0..frames {
            let base = frame * EMBEDDING_DIM;
            let chunk = &slice[base..base + EMBEDDING_DIM];
            for (idx, value) in chunk.iter().enumerate() {
                pooled[idx] += *value;
            }
        }
        for value in &mut pooled {
            *value /= frames as f32;
        }
        return Ok(pooled);
    }
    Err("YAMNet output did not include 1024-D embedding".into())
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

fn yamnet_model_path() -> Result<PathBuf, String> {
    let root = crate::app_dirs::app_root_dir().map_err(|err| err.to_string())?;
    Ok(root.join("models").join("yamnet.tflite"))
}

#[allow(dead_code)]
pub(crate) fn embedding_model_path() -> &'static PathBuf {
    static PATH: LazyLock<PathBuf> = LazyLock::new(|| {
        crate::app_dirs::app_root_dir()
            .map(|root| root.join("models").join("yamnet.tflite"))
            .unwrap_or_else(|_| PathBuf::from("yamnet.tflite"))
    });
    &PATH
}
