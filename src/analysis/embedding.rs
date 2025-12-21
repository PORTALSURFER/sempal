use std::path::PathBuf;
use std::sync::LazyLock;

use ndarray::Array3;
use ort::session::Session;
use ort::session::builder::SessionBuilder;
use ort::session::output::SessionOutputs;
use ort::value::Tensor;

use crate::analysis::audio;

pub const EMBEDDING_MODEL_ID: &str =
    "clap_htsat_fused__sr48k__nfft1024__hop480__mel64__chunk10__repeatpad_v1";
pub const EMBEDDING_DIM: usize = 512;
pub const EMBEDDING_DTYPE_F32: &str = "f32";
const CLAP_SAMPLE_RATE: u32 = 48_000;
const CLAP_INPUT_SECONDS: f32 = 10.0;
const CLAP_INPUT_SAMPLES: usize = (CLAP_SAMPLE_RATE as f32 * CLAP_INPUT_SECONDS) as usize;

pub(crate) struct ClapModel {
    session: Session,
}

impl ClapModel {
    pub(crate) fn load() -> Result<Self, String> {
        let model_path = clap_model_path()?;
        if !model_path.exists() {
            return Err(format!(
                "CLAP ONNX model not found at {}",
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
        unsafe {
            std::env::set_var("ORT_DYLIB_PATH", &runtime_path);
        }
        ort::environment::init_from(runtime_path.to_string_lossy().to_string())
            .with_name("sempal_clap")
            .commit()
            .map_err(|err| format!("Failed to initialize ONNX environment: {err}"))?;
        let session = SessionBuilder::new()
            .map_err(|err| format!("Failed to create ONNX session builder: {err}"))?
            .with_intra_threads(
                std::thread::available_parallelism()
                    .map(|n| n.get().saturating_sub(1).max(1))
                    .unwrap_or(1),
            )
            .map_err(|err| format!("Failed to set ONNX threads: {err}"))?
            .commit_from_file(&model_path)
            .map_err(|err| format!("Failed to load ONNX model: {err}"))?;
        Ok(Self { session })
    }
}

pub(crate) fn infer_embedding(
    cache: &mut Option<ClapModel>,
    samples: &[f32],
    sample_rate: u32,
) -> Result<Vec<f32>, String> {
    if samples.is_empty() {
        return Err("CLAP inference requires non-empty samples".into());
    }
    if cache.is_none() {
        *cache = Some(ClapModel::load()?);
    }
    let model = cache.as_mut().expect("CLAP model loaded");

    let mut resampled = if sample_rate != CLAP_SAMPLE_RATE {
        audio::resample_linear(samples, sample_rate, CLAP_SAMPLE_RATE)
    } else {
        samples.to_vec()
    };
    audio::sanitize_samples_in_place(&mut resampled);
    let input = repeat_pad(&resampled, CLAP_INPUT_SAMPLES);
    let array = Array3::from_shape_vec((1, 1, CLAP_INPUT_SAMPLES), input)
        .map_err(|err| format!("Failed to build CLAP input: {err}"))?;
    let input_value = Tensor::from_array(array)
        .map_err(|err| format!("Failed to create ONNX input tensor: {err}"))?;
    let outputs = model
        .session
        .run(ort::inputs![input_value])
        .map_err(|err| format!("ONNX inference failed: {err}"))?;
    let mut embedding = extract_embedding(&outputs)?;
    if embedding.len() != EMBEDDING_DIM {
        return Err(format!(
            "CLAP embedding length mismatch: expected {}, got {}",
            EMBEDDING_DIM,
            embedding.len()
        ));
    }
    normalize_l2_in_place(&mut embedding);
    Ok(embedding)
}

fn repeat_pad(samples: &[f32], target_len: usize) -> Vec<f32> {
    if samples.is_empty() || target_len == 0 {
        return Vec::new();
    }
    if samples.len() >= target_len {
        return samples[..target_len].to_vec();
    }
    let mut out = Vec::with_capacity(target_len);
    while out.len() < target_len {
        let remaining = target_len - out.len();
        let take = remaining.min(samples.len());
        out.extend_from_slice(&samples[..take]);
    }
    out
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

fn clap_model_path() -> Result<PathBuf, String> {
    let root = crate::app_dirs::app_root_dir().map_err(|err| err.to_string())?;
    Ok(root.join("models").join("clap_audio.onnx"))
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
            .map(|root| root.join("models").join("clap_audio.onnx"))
            .unwrap_or_else(|_| PathBuf::from("clap_audio.onnx"))
    });
    &PATH
}

fn extract_embedding(outputs: &SessionOutputs) -> Result<Vec<f32>, String> {
    for value in outputs.values() {
        let array = value
            .try_extract_array::<f32>()
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
