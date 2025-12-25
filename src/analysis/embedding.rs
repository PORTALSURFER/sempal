use std::cell::RefCell;
use std::env;
use std::path::PathBuf;
use std::sync::{LazyLock, OnceLock};

use ort::execution_providers::{CPUExecutionProvider, ExecutionProvider, ExecutionProviderDispatch};
#[cfg(target_os = "windows")]
use ort::execution_providers::DirectMLExecutionProvider;
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
const QUERY_WINDOW_SECONDS: f32 = 2.0;
const QUERY_HOP_SECONDS: f32 = 1.0;
const QUERY_MAX_WINDOWS: usize = 24;

pub(crate) struct ClapModel {
    session: Session,
    input_scratch: Vec<f32>,
    input_batch_scratch: Vec<f32>,
    resample_scratch: Vec<f32>,
}

static ORT_ENV_INIT: OnceLock<Result<(), String>> = OnceLock::new();

thread_local! {
    static TLS_CLAP_MODEL: RefCell<Option<ClapModel>> = RefCell::new(None);
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
        ensure_onnx_env(&runtime_path)?;
        let mut session_builder = SessionBuilder::new()
            .map_err(|err| format!("Failed to create ONNX session builder: {err}"))?
            .with_intra_threads(onnx_intra_threads())
            .map_err(|err| format!("Failed to set ONNX threads: {err}"))?;
        let execution_providers = onnx_execution_providers()?;
        if !execution_providers.is_empty() {
            session_builder = session_builder
                .with_execution_providers(execution_providers)
                .map_err(|err| format!("Failed to configure ONNX execution providers: {err}"))?;
        }
        let session = session_builder
            .commit_from_file(&model_path)
            .map_err(|err| format!("Failed to load ONNX model: {err}"))?;
        Ok(Self {
            session,
            input_scratch: vec![0.0_f32; CLAP_INPUT_SAMPLES],
            input_batch_scratch: Vec::new(),
            resample_scratch: Vec::new(),
        })
    }
}

pub(crate) fn embedding_batch_max() -> usize {
    env::var("SEMPAL_EMBEDDING_BATCH")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value >= 1)
        .unwrap_or(16)
}

fn onnx_intra_threads() -> usize {
    env::var("SEMPAL_ONNX_INTRA_THREADS")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value >= 1)
        .unwrap_or(1)
}

fn onnx_execution_providers() -> Result<Vec<ExecutionProviderDispatch>, String> {
    let selection = env::var("SEMPAL_ONNX_EP")
        .ok()
        .map(|value| value.trim().to_lowercase());
    match selection.as_deref() {
        None | Some("auto") => onnx_execution_providers_auto(),
        Some("cpu") => Ok(vec![CPUExecutionProvider::default().build()]),
        Some("directml") => onnx_execution_providers_directml(),
        Some(other) => Err(format!(
            "Unsupported SEMPAL_ONNX_EP '{other}'. Use 'auto', 'cpu', or 'directml'."
        )),
    }
}

fn onnx_execution_providers_auto() -> Result<Vec<ExecutionProviderDispatch>, String> {
    let mut providers = Vec::new();
    #[cfg(target_os = "windows")]
    {
        if let Some(ep) = directml_execution_provider(false)? {
            providers.push(ep);
        }
    }
    providers.push(CPUExecutionProvider::default().build());
    Ok(providers)
}

fn onnx_execution_providers_directml() -> Result<Vec<ExecutionProviderDispatch>, String> {
    #[cfg(target_os = "windows")]
    {
        let mut providers = Vec::new();
        if let Some(ep) = directml_execution_provider(true)? {
            providers.push(ep);
        }
        providers.push(CPUExecutionProvider::default().build());
        return Ok(providers);
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("SEMPAL_ONNX_EP=directml is only supported on Windows.".to_string())
    }
}

#[cfg(target_os = "windows")]
fn directml_execution_provider(
    required: bool,
) -> Result<Option<ExecutionProviderDispatch>, String> {
    let provider = DirectMLExecutionProvider::default();
    match provider.is_available() {
        Ok(true) => {
            let dispatch = if required {
                provider.build().error_on_failure()
            } else {
                provider.build().fail_silently()
            };
            Ok(Some(dispatch))
        }
        Ok(false) => {
            if required {
                Err("DirectML execution provider not available. Ensure the ONNX Runtime build includes DirectML and DirectML is installed.".to_string())
            } else {
                Ok(None)
            }
        }
        Err(err) => {
            if required {
                Err(format!("Failed to query DirectML availability: {err}"))
            } else {
                Ok(None)
            }
        }
    }
}

pub(crate) struct EmbeddingBatchInput<'a> {
    pub(crate) samples: &'a [f32],
    pub(crate) sample_rate: u32,
}

pub(crate) fn infer_embedding(samples: &[f32], sample_rate: u32) -> Result<Vec<f32>, String> {
    if samples.is_empty() {
        return Err("CLAP inference requires non-empty samples".into());
    }
    with_clap_model(|model| infer_embedding_with_model(model, samples, sample_rate))
}

pub(crate) fn infer_embedding_query(samples: &[f32], sample_rate: u32) -> Result<Vec<f32>, String> {
    if samples.is_empty() {
        return Err("CLAP inference requires non-empty samples".into());
    }
    let ranges = query_window_ranges(samples.len(), sample_rate);
    if ranges.len() <= 1 {
        return infer_embedding(samples, sample_rate);
    }
    with_clap_model(|model| {
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

pub(crate) fn infer_embeddings_batch(
    inputs: &[EmbeddingBatchInput<'_>],
) -> Result<Vec<Vec<f32>>, String> {
    if inputs.is_empty() {
        return Ok(Vec::new());
    }
    if !clap_batch_enabled() {
        return with_clap_model(|model| {
            let mut outputs = Vec::with_capacity(inputs.len());
            for input in inputs {
                outputs.push(infer_embedding_with_model(
                    model,
                    input.samples,
                    input.sample_rate,
                )?);
            }
            Ok(outputs)
        });
    }
    with_clap_model(|model| {
        match infer_embeddings_with_model(model, inputs) {
            Ok(values) => Ok(values),
            Err(_err) if inputs.len() > 1 => {
                let mut outputs = Vec::with_capacity(inputs.len());
                for input in inputs {
                    outputs.push(infer_embedding_with_model(
                        model,
                        input.samples,
                        input.sample_rate,
                    )?);
                }
                Ok(outputs)
            }
            Err(err) => Err(err),
        }
    })
}

fn infer_embedding_with_model(
    model: &mut ClapModel,
    samples: &[f32],
    sample_rate: u32,
) -> Result<Vec<f32>, String> {
    let resampled = if sample_rate != CLAP_SAMPLE_RATE {
        audio::resample_linear_into(
            &mut model.resample_scratch,
            samples,
            sample_rate,
            CLAP_SAMPLE_RATE,
        );
        model.resample_scratch.as_mut_slice()
    } else {
        model.resample_scratch.clear();
        model.resample_scratch.extend_from_slice(samples);
        model.resample_scratch.as_mut_slice()
    };
    audio::sanitize_samples_in_place(resampled);
    repeat_pad_into(&mut model.input_scratch, resampled, CLAP_INPUT_SAMPLES);
    let input_value = TensorRef::from_array_view((
        [1usize, 1, CLAP_INPUT_SAMPLES],
        model.input_scratch.as_slice(),
    ))
    .map_err(|err| format!("Failed to create ONNX input tensor: {err}"))?;
    let outputs = model
        .session
        .run(ort::inputs![input_value])
        .map_err(|err| format!("ONNX inference failed: {err}"))?;
    let mut embeddings = extract_embeddings(&outputs, 1)?;
    let embedding = embeddings
        .pop()
        .ok_or_else(|| "CLAP embedding output missing".to_string())?;
    Ok(embedding)
}

fn infer_embeddings_with_model(
    model: &mut ClapModel,
    inputs: &[EmbeddingBatchInput<'_>],
) -> Result<Vec<Vec<f32>>, String> {
    let batch = inputs.len();
    let total_len = batch * CLAP_INPUT_SAMPLES;
    model.input_batch_scratch.clear();
    model.input_batch_scratch.resize(total_len, 0.0);
    for (idx, input) in inputs.iter().enumerate() {
        let resampled = if input.sample_rate != CLAP_SAMPLE_RATE {
            audio::resample_linear_into(
                &mut model.resample_scratch,
                input.samples,
                input.sample_rate,
                CLAP_SAMPLE_RATE,
            );
            model.resample_scratch.as_mut_slice()
        } else {
            model.resample_scratch.clear();
            model.resample_scratch.extend_from_slice(input.samples);
            model.resample_scratch.as_mut_slice()
        };
        audio::sanitize_samples_in_place(resampled);
        let start = idx * CLAP_INPUT_SAMPLES;
        let end = start + CLAP_INPUT_SAMPLES;
        repeat_pad_slice(&mut model.input_batch_scratch[start..end], resampled);
    }
    let input_value = TensorRef::from_array_view((
        [batch, 1usize, CLAP_INPUT_SAMPLES],
        model.input_batch_scratch.as_slice(),
    ))
    .map_err(|err| format!("Failed to create ONNX input tensor: {err}"))?;
    let outputs = model
        .session
        .run(ort::inputs![input_value])
        .map_err(|err| format!("ONNX inference failed: {err}"))?;
    extract_embeddings(&outputs, batch)
}

fn query_window_ranges(sample_len: usize, sample_rate: u32) -> Vec<(usize, usize)> {
    let window_len = (QUERY_WINDOW_SECONDS * sample_rate as f32).round() as usize;
    let hop_len = (QUERY_HOP_SECONDS * sample_rate as f32).round() as usize;
    if window_len == 0 || sample_len == 0 {
        return Vec::new();
    }
    if sample_len <= window_len {
        return vec![(0, sample_len)];
    }
    let hop_len = hop_len.max(1);
    let mut ranges = Vec::new();
    let max_start = sample_len.saturating_sub(window_len);
    let mut start = 0;
    while start <= max_start {
        ranges.push((start, start + window_len));
        start += hop_len;
    }
    if ranges.len() > QUERY_MAX_WINDOWS {
        let stride = (ranges.len() as f32 / QUERY_MAX_WINDOWS as f32).ceil() as usize;
        ranges = ranges
            .into_iter()
            .step_by(stride)
            .take(QUERY_MAX_WINDOWS)
            .collect();
    }
    ranges
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

fn repeat_pad_slice(out: &mut [f32], samples: &[f32]) {
    out.fill(0.0);
    if samples.is_empty() || out.is_empty() {
        return;
    }
    if samples.len() >= out.len() {
        out.copy_from_slice(&samples[..out.len()]);
        return;
    }
    let mut offset = 0usize;
    while offset < out.len() {
        let remaining = out.len() - offset;
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

fn ensure_onnx_env(runtime_path: &PathBuf) -> Result<(), String> {
    let runtime_path = runtime_path.to_string_lossy().to_string();
    let init = ORT_ENV_INIT.get_or_init(|| {
        unsafe {
            std::env::set_var("ORT_DYLIB_PATH", &runtime_path);
        }
        ort::environment::init_from(runtime_path.clone())
            .with_name("sempal_clap")
            .commit()
            .map(|_| ())
            .map_err(|err| format!("Failed to initialize ONNX environment: {err}"))
    });
    init.clone()
}

fn with_clap_model<T>(f: impl FnOnce(&mut ClapModel) -> Result<T, String>) -> Result<T, String> {
    TLS_CLAP_MODEL.with(|cell| {
        let mut guard = cell.borrow_mut();
        if guard.is_none() {
            *guard = Some(ClapModel::load()?);
        }
        let model = guard.as_mut().expect("CLAP model loaded");
        f(model)
    })
}

fn clap_batch_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        if cfg!(target_os = "windows") {
            if let Ok(value) = std::env::var("SEMPAL_CLAP_BATCH") {
                return value.trim() == "1";
            }
            return false;
        }
        match std::env::var("SEMPAL_CLAP_BATCH") {
            Ok(value) => value.trim() == "1",
            Err(_) => true,
        }
    })
}

fn clap_model_path() -> Result<PathBuf, String> {
    let root = crate::app_dirs::app_root_dir().map_err(|err| err.to_string())?;
    Ok(root.join("models").join("clap_audio.onnx"))
}

fn onnx_runtime_path() -> Result<PathBuf, String> {
    let root = crate::app_dirs::app_root_dir().map_err(|err| err.to_string())?;
    Ok(root
        .join("models")
        .join("onnxruntime")
        .join(onnx_runtime_filename()))
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

#[allow(dead_code)]
pub(crate) fn embedding_model_path() -> &'static PathBuf {
    static PATH: LazyLock<PathBuf> = LazyLock::new(|| {
        crate::app_dirs::app_root_dir()
            .map(|root| root.join("models").join("clap_audio.onnx"))
            .unwrap_or_else(|_| PathBuf::from("clap_audio.onnx"))
    });
    &PATH
}

fn extract_embeddings(outputs: &SessionOutputs, batch: usize) -> Result<Vec<Vec<f32>>, String> {
    for value in outputs.values() {
        let array = value
            .try_extract_array::<f32>()
            .map_err(|err| format!("Failed to read ONNX output tensor: {err}"))?;
        let shape = array.shape();
        if shape.is_empty() || *shape.last().unwrap_or(&0) != EMBEDDING_DIM {
            continue;
        }
        if shape.len() == 1 && batch == 1 {
            let flat = array
                .as_slice()
                .ok_or_else(|| "ONNX output tensor not contiguous".to_string())?;
            if flat.len() < EMBEDDING_DIM {
                continue;
            }
            let mut pooled = flat[..EMBEDDING_DIM].to_vec();
            normalize_l2_in_place(&mut pooled);
            let norm = l2_norm(&pooled);
            if !norm.is_finite() || (norm - 1.0).abs() > 1e-3 {
                return Err(format!("CLAP embedding L2 norm out of range: {norm:.6}"));
            }
            return Ok(vec![pooled]);
        }
        if shape.len() < 2 {
            continue;
        }
        let batch_dim = shape[0];
        if batch_dim != batch {
            continue;
        }
        let mut frames_per = 1usize;
        if shape.len() > 2 {
            for dim in &shape[1..shape.len() - 1] {
                frames_per = frames_per.saturating_mul(*dim);
            }
        }
        let flat = array
            .as_slice()
            .ok_or_else(|| "ONNX output tensor not contiguous".to_string())?;
        let expected_len = batch
            .saturating_mul(frames_per)
            .saturating_mul(EMBEDDING_DIM);
        if flat.len() < expected_len {
            continue;
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
                return Err(format!("CLAP embedding L2 norm out of range: {norm:.6}"));
            }
            outputs.push(pooled);
        }
        return Ok(outputs);
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
            let sample = (2.0 * std::f32::consts::PI * golden.tone_hz * t).sin() * golden.tone_amp;
            tone.push(sample);
        }
        let target_len = (golden.sample_rate as f32 * golden.target_seconds).round() as usize;
        let mut padded = Vec::new();
        repeat_pad_into(&mut padded, &tone, target_len);

        let embedding = infer_embedding(&padded, golden.sample_rate).expect("rust embedding");
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
