use std::path::PathBuf;
use std::sync::LazyLock;

use ndarray::Array2;
use ort::{Environment, Session, SessionBuilder, Value};

use crate::analysis::audio;

pub(crate) const EMBEDDING_MODEL_ID: &str = "yamnet_onnx_v1";
pub(crate) const EMBEDDING_DIM: usize = 1024;
pub(crate) const EMBEDDING_DTYPE_F32: i64 = 0;
const YAMNET_INPUT_SAMPLES: usize = 15_600;

pub(crate) struct YamnetModel {
    _env: Environment,
    session: Session,
}

impl YamnetModel {
    pub(crate) fn load() -> Result<Self, String> {
        let model_path = yamnet_model_path()?;
        if !model_path.exists() {
            return Err(format!(
                "YAMNet ONNX model not found at {}",
                model_path.to_string_lossy()
            ));
        }
        let runtime_path = onnx_runtime_path()?;
        if !runtime_path.exists() {
            return Err(format!(
                "ONNX Runtime DLL not found at {}",
                runtime_path.to_string_lossy()
            ));
        }
        std::env::set_var("ORT_DYLIB_PATH", &runtime_path);
        let env = Environment::builder()
            .with_name("sempal_yamnet")
            .build()
            .map_err(|err| format!("Failed to create ONNX environment: {err}"))?;
        let session = SessionBuilder::new(&env)
            .map_err(|err| format!("Failed to create ONNX session builder: {err}"))?
            .with_intra_threads(
                std::thread::available_parallelism()
                    .map(|n| n.get().saturating_sub(1).max(1))
                    .unwrap_or(1) as i32,
            )
            .map_err(|err| format!("Failed to set ONNX threads: {err}"))?
            .with_model_from_file(&model_path)
            .map_err(|err| format!("Failed to load ONNX model: {err}"))?;
        Ok(Self { _env: env, session })
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
        let array = Array2::from_shape_vec((1, YAMNET_INPUT_SAMPLES), input)
            .map_err(|err| format!("Failed to build ONNX input array: {err}"))?;
        let input_value = Value::from_array(array)
            .map_err(|err| format!("Failed to create ONNX input tensor: {err}"))?;
        let outputs = model
            .session
            .run(vec![input_value])
            .map_err(|err| format!("ONNX inference failed: {err}"))?;
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
    Ok(root.join("models").join("yamnet.onnx"))
}

fn onnx_runtime_path() -> Result<PathBuf, String> {
    let root = crate::app_dirs::app_root_dir().map_err(|err| err.to_string())?;
    Ok(root.join("models").join("onnxruntime").join(onnx_runtime_filename()))
}

fn onnx_runtime_filename() -> &'static str {
    if cfg!(target_os = "windows") {
        "onnxruntime.dll"
    } else if cfg!(target_os = "macos") {
        "libonnxruntime.dylib"
    } else {
        "libonnxruntime.so"
    }
}

pub(crate) fn embedding_model_path() -> &'static PathBuf {
    static PATH: LazyLock<PathBuf> = LazyLock::new(|| {
        crate::app_dirs::app_root_dir()
            .map(|root| root.join("models").join("yamnet.onnx"))
            .unwrap_or_else(|_| PathBuf::from("yamnet.onnx"))
    });
    &PATH
}

fn extract_embedding(outputs: &[Value]) -> Result<Vec<f32>, String> {
    for value in outputs {
        let array = value
            .try_extract::<ndarray::Array<f32, ndarray::IxDyn>>()
            .map_err(|err| format!("Failed to read ONNX output tensor: {err}"))?;
        let shape = array.shape();
        if shape.is_empty() || *shape.last().unwrap_or(&0) != EMBEDDING_DIM {
            continue;
        }
        let flat = array.as_slice().ok_or_else(|| {
            "ONNX output tensor not contiguous".to_string()
        })?;
        if flat.len() < EMBEDDING_DIM {
            continue;
        }
        let frames = flat.len() / EMBEDDING_DIM;
        if frames <= 1 {
            return Ok(flat.to_vec());
        }
        let mut pooled = vec![0.0_f32; EMBEDDING_DIM];
        for frame in 0..frames {
            let base = frame * EMBEDDING_DIM;
            let chunk = &flat[base..base + EMBEDDING_DIM];
            for (idx, value) in chunk.iter().enumerate() {
                pooled[idx] += *value;
            }
        }
        for value in &mut pooled {
            *value /= frames as f32;
        }
        return Ok(pooled);
    }
    Err("No embedding output found in ONNX outputs".to_string())
}
