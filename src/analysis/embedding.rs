use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

use ort::session::Session;
use ort::session::builder::SessionBuilder;
use ort::session::output::SessionOutputs;
use ort::value::TensorRef;

use crate::analysis::audio;

pub const EMBEDDING_MODEL_ID: &str =
    "clap_htsat_fused__sr48k__nfft1024__hop480__mel64__chunk10__repeatpad_v2";
pub const EMBEDDING_DIM: usize = 512;
pub const EMBEDDING_DTYPE_F32: &str = "f32";
const CLAP_SAMPLE_RATE: u32 = 48_000;
const CLAP_INPUT_SECONDS: f32 = 10.0;
const CLAP_INPUT_SAMPLES: usize = (CLAP_SAMPLE_RATE as f32 * CLAP_INPUT_SECONDS) as usize;

pub(crate) struct ClapModel {
    session: Session,
    input_scratch: Vec<f32>,
}

static GLOBAL_CLAP_MODEL: LazyLock<Mutex<Option<ClapModel>>> = LazyLock::new(|| Mutex::new(None));

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
        Ok(Self {
            session,
            input_scratch: vec![0.0_f32; CLAP_INPUT_SAMPLES],
        })
    }
}

pub(crate) fn infer_embedding(samples: &[f32], sample_rate: u32) -> Result<Vec<f32>, String> {
    if samples.is_empty() {
        return Err("CLAP inference requires non-empty samples".into());
    }
    let mut guard = GLOBAL_CLAP_MODEL
        .lock()
        .map_err(|_| "CLAP model lock poisoned".to_string())?;
    if guard.is_none() {
        *guard = Some(ClapModel::load()?);
    }
    let model = guard.as_mut().expect("CLAP model loaded");
    infer_embedding_with_model(model, samples, sample_rate)
}

fn infer_embedding_with_model(
    model: &mut ClapModel,
    samples: &[f32],
    sample_rate: u32,
) -> Result<Vec<f32>, String> {
    let mut resampled = if sample_rate != CLAP_SAMPLE_RATE {
        audio::resample_linear(samples, sample_rate, CLAP_SAMPLE_RATE)
    } else {
        samples.to_vec()
    };
    audio::sanitize_samples_in_place(&mut resampled);
    repeat_pad_into(&mut model.input_scratch, &resampled, CLAP_INPUT_SAMPLES);
    let input_value = TensorRef::from_array_view((
        [1usize, 1, CLAP_INPUT_SAMPLES],
        model.input_scratch.as_slice(),
    ))
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
    let norm = l2_norm(&embedding);
    if !norm.is_finite() || (norm - 1.0).abs() > 1e-3 {
        return Err(format!("CLAP embedding L2 norm out of range: {norm:.6}"));
    }
    Ok(embedding)
}

fn repeat_pad_into(out: &mut Vec<f32>, samples: &[f32], target_len: usize) {
    out.clear();
    out.resize(target_len, 0.0);
    if samples.is_empty() || target_len == 0 {
        return;
    }
    if samples.len() >= target_len {
        out[..target_len].copy_from_slice(&samples[..target_len]);
        return;
    }
    let mut offset = 0usize;
    while offset < target_len {
        let remaining = target_len - offset;
        let take = remaining.min(samples.len());
        out[offset..offset + take].copy_from_slice(&samples[..take]);
        offset += take;
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct GoldenEmbedding {
        sample_rate: u32,
        tone_hz: f32,
        tone_amp: f32,
        tone_seconds: f32,
        target_seconds: f32,
        embedding: Vec<f32>,
    }

    #[test]
    fn golden_embedding_matches_python() {
        let path = std::env::var("SEMPAL_CLAP_EMBED_GOLDEN_PATH")
            .ok()
            .filter(|path| !path.trim().is_empty())
            .unwrap_or_else(|| "tests/golden_embedding.json".to_string());
        if !clap_model_path().map(|p| p.exists()).unwrap_or(false) {
            return;
        }
        if !onnx_runtime_path().map(|p| p.exists()).unwrap_or(false) {
            return;
        }
        let payload = std::fs::read_to_string(path).expect("read golden json");
        let golden: GoldenEmbedding = serde_json::from_str(&payload).expect("parse golden json");
        assert_eq!(golden.embedding.len(), EMBEDDING_DIM);

        let tone_len = (golden.sample_rate as f32 * golden.tone_seconds).round() as usize;
        let mut tone = Vec::with_capacity(tone_len);
        for i in 0..tone_len {
            let t = i as f32 / golden.sample_rate.max(1) as f32;
            let sample =
                (2.0 * std::f32::consts::PI * golden.tone_hz * t).sin() * golden.tone_amp;
            tone.push(sample);
        }
        let target_len = (golden.sample_rate as f32 * golden.target_seconds).round() as usize;
        let mut padded = Vec::new();
        repeat_pad_into(&mut padded, &tone, target_len);

        let embedding =
            infer_embedding(&padded, golden.sample_rate).expect("rust embedding");
        assert_eq!(embedding.len(), golden.embedding.len());

        let mut max_diff = 0.0_f32;
        for (&a, &b) in embedding.iter().zip(golden.embedding.iter()) {
            max_diff = max_diff.max((a - b).abs());
        }
        const MAX_DIFF: f32 = 1e-3;
        assert!(
            max_diff <= MAX_DIFF,
            "max diff {max_diff} exceeds {MAX_DIFF}"
        );
    }
}
