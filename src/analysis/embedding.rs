use std::env;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex, OnceLock};

use burn::backend::wgpu::{self, graphics::Vulkan, WgpuDevice};
use burn::tensor::{Tensor, TensorData};

use crate::analysis::audio;
use crate::analysis::panns_preprocess::{
    PannsPreprocessScratch, PANNS_MEL_BANDS, PANNS_STFT_HOP, log_mel_frames_with_scratch,
};

mod panns_burn {
    include!(concat!(env!("OUT_DIR"), "/burn_panns/panns_cnn14_16k.rs"));
}

mod panns_paths {
    include!(concat!(env!("OUT_DIR"), "/burn_panns/panns_paths.rs"));
}

type PannsBackend = wgpu::Wgpu;
type PannsOutput = Tensor<PannsBackend, 2>;

pub const EMBEDDING_MODEL_ID: &str =
    "panns_cnn14_16k__sr16k__nfft512__hop160__mel64__log10__chunk10__repeatpad_v1";
pub const EMBEDDING_DIM: usize = 2048;
pub const EMBEDDING_DTYPE_F32: &str = "f32";
const PANNS_SAMPLE_RATE: u32 = 16_000;
const PANNS_INPUT_SECONDS: f32 = 10.0;
const PANNS_INPUT_SAMPLES: usize = (PANNS_SAMPLE_RATE as f32 * PANNS_INPUT_SECONDS) as usize;
const PANNS_INPUT_FRAMES: usize =
    (PANNS_SAMPLE_RATE as f32 * PANNS_INPUT_SECONDS / PANNS_STFT_HOP as f32) as usize;
pub(crate) const PANNS_LOGMEL_LEN: usize = PANNS_MEL_BANDS * PANNS_INPUT_FRAMES;
const QUERY_WINDOW_SECONDS: f32 = 2.0;
const QUERY_HOP_SECONDS: f32 = 1.0;
const QUERY_MAX_WINDOWS: usize = 24;

pub(crate) struct PannsModel {
    model: panns_burn::Model<PannsBackend>,
    device: WgpuDevice,
    input_scratch: Vec<f32>,
    input_batch_scratch: Vec<f32>,
    resample_scratch: Vec<f32>,
    wave_scratch: Vec<f32>,
    preprocess_scratch: PannsPreprocessScratch,
}

pub(crate) struct PannsLogMelScratch {
    resample_scratch: Vec<f32>,
    wave_scratch: Vec<f32>,
    preprocess_scratch: PannsPreprocessScratch,
}

impl Default for PannsLogMelScratch {
    fn default() -> Self {
        Self {
            resample_scratch: Vec::new(),
            wave_scratch: Vec::new(),
            preprocess_scratch: PannsPreprocessScratch::new(),
        }
    }
}

static WGPU_INIT: OnceLock<()> = OnceLock::new();
static PANNS_MODEL: OnceLock<Mutex<Option<PannsModel>>> = OnceLock::new();
static PANNS_WARMED: OnceLock<()> = OnceLock::new();

impl PannsModel {
    pub(crate) fn load() -> Result<Self, String> {
        let model_path = panns_burnpack_path()?;
        if !model_path.exists() {
            return Err(format!(
                "PANNs burnpack model not found at {}",
                model_path.to_string_lossy()
            ));
        }
        init_cubecl_config();
        let device = WgpuDevice::default();
        init_wgpu(&device);
        let model = panns_burn::Model::<PannsBackend>::from_file(
            model_path
                .to_str()
                .ok_or_else(|| "PANNs burnpack path contains invalid UTF-8".to_string())?,
            &device,
        );
        Ok(Self {
            model,
            device,
            input_scratch: vec![0.0_f32; PANNS_MEL_BANDS * PANNS_INPUT_FRAMES],
            input_batch_scratch: Vec::new(),
            resample_scratch: Vec::new(),
            wave_scratch: Vec::new(),
            preprocess_scratch: PannsPreprocessScratch::new(),
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

pub(crate) fn embedding_inflight_max() -> usize {
    env::var("SEMPAL_EMBEDDING_INFLIGHT")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value >= 1)
        .unwrap_or(2)
}

pub(crate) fn embedding_pipeline_enabled() -> bool {
    env::var("SEMPAL_EMBEDDING_PIPELINE")
        .ok()
        .map(|value| value.trim().eq_ignore_ascii_case("1"))
        .unwrap_or(false)
}

pub(crate) struct EmbeddingBatchInput<'a> {
    pub(crate) samples: &'a [f32],
    pub(crate) sample_rate: u32,
}

pub(crate) fn infer_embedding(samples: &[f32], sample_rate: u32) -> Result<Vec<f32>, String> {
    if samples.is_empty() {
        return Err("PANNs inference requires non-empty samples".into());
    }
    with_panns_model(|model| infer_embedding_with_model(model, samples, sample_rate))
}

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
                outputs.push(infer_embedding_with_model(
                    model,
                    input.samples,
                    input.sample_rate,
                )?);
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

pub(crate) fn build_panns_logmel_into(
    samples: &[f32],
    sample_rate: u32,
    out: &mut [f32],
    scratch: &mut PannsLogMelScratch,
) -> Result<(), String> {
    if out.len() != PANNS_LOGMEL_LEN {
        return Err(format!(
            "PANNs log-mel buffer has wrong length: expected {PANNS_LOGMEL_LEN}, got {}",
            out.len()
        ));
    }
    prepare_panns_logmel(
        &mut scratch.resample_scratch,
        &mut scratch.wave_scratch,
        &mut scratch.preprocess_scratch,
        out,
        samples,
        sample_rate,
    )
}

pub(crate) fn infer_embedding_from_logmel(logmel: &[f32]) -> Result<Vec<f32>, String> {
    if logmel.len() != PANNS_LOGMEL_LEN {
        return Err(format!(
            "PANNs log-mel buffer has wrong length: expected {PANNS_LOGMEL_LEN}, got {}",
            logmel.len()
        ));
    }
    with_panns_model(|model| {
        let mut embeddings = run_panns_inference(&model.model, &model.device, logmel, 1)?;
        embeddings
            .pop()
            .ok_or_else(|| "PANNs embedding output missing".to_string())
    })
}

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
        let data = TensorData::new(
            logmel,
            [batch, 1, PANNS_INPUT_FRAMES, PANNS_MEL_BANDS],
        );
        run_panns_inference_from_data(&model.model, &model.device, data, batch)
    })
}

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
        let err = format!(
            "PANNs log-mel buffer has wrong length: expected {expected}"
        );
        return logmels.iter().map(|_| Err(err.clone())).collect();
    }
    let micro_batch = micro_batch.max(1);
    let inflight = inflight.max(1);
    let results = std::sync::Arc::new(Mutex::new(vec![None; logmels.len()]));
    let errors = std::sync::Arc::new(Mutex::new(vec![None; logmels.len()]));
    let (tx, rx) = std::sync::mpsc::sync_channel::<(usize, usize, PannsOutput)>(inflight);
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
    let submit_result = with_panns_model(|model| {
        for (offset, chunk) in logmels.chunks(micro_batch).enumerate() {
            let start = offset * micro_batch;
            let mut batch_input =
                Vec::with_capacity(chunk.len() * PANNS_LOGMEL_LEN);
            for logmel in chunk {
                batch_input.extend_from_slice(logmel.as_slice());
            }
            let data = TensorData::new(
                batch_input,
                [chunk.len(), 1, PANNS_INPUT_FRAMES, PANNS_MEL_BANDS],
            );
            let output = run_panns_forward_from_data(&model.model, &model.device, data);
            if submit_tx.send((start, chunk.len(), output)).is_err() {
                return Err("PANNs readback channel closed".to_string());
            }
        }
        Ok(())
    });
    drop(tx);
    let _ = readback.join();
    if let Err(err) = submit_result {
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

pub(crate) fn warmup_panns() -> Result<(), String> {
    if PANNS_WARMED.get().is_some() {
        return Ok(());
    }
    let mut logmel = vec![0.0_f32; PANNS_LOGMEL_LEN];
    let result = with_panns_model(|model| {
        let _ = run_panns_inference(&model.model, &model.device, logmel.as_slice(), 1)?;
        Ok(())
    });
    if result.is_ok() {
        let _ = PANNS_WARMED.set(());
    }
    result
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
    let mut embeddings = run_panns_inference(
        &model.model,
        &model.device,
        model.input_scratch.as_slice(),
        1,
    )?;
    let embedding = embeddings
        .pop()
        .ok_or_else(|| "PANNs embedding output missing".to_string())?;
    Ok(embedding)
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
    run_panns_inference(
        &model.model,
        &model.device,
        model.input_batch_scratch.as_slice(),
        batch,
    )
}

fn prepare_panns_logmel(
    resample_scratch: &mut Vec<f32>,
    wave_scratch: &mut Vec<f32>,
    preprocess_scratch: &mut PannsPreprocessScratch,
    out: &mut [f32],
    samples: &[f32],
    sample_rate: u32,
) -> Result<(), String> {
    if sample_rate != PANNS_SAMPLE_RATE {
        audio::resample_linear_into(
            resample_scratch,
            samples,
            sample_rate,
            PANNS_SAMPLE_RATE,
        );
        audio::sanitize_samples_in_place(resample_scratch.as_mut_slice());
        repeat_pad_into(wave_scratch, resample_scratch.as_slice(), PANNS_INPUT_SAMPLES);
    } else {
        repeat_pad_into(wave_scratch, samples, PANNS_INPUT_SAMPLES);
        audio::sanitize_samples_in_place(wave_scratch.as_mut_slice());
    }
    let frames =
        log_mel_frames_with_scratch(wave_scratch, PANNS_SAMPLE_RATE, preprocess_scratch)?;
    out.fill(0.0);
    for (frame_idx, frame) in frames.iter().take(PANNS_INPUT_FRAMES).enumerate() {
        for (mel_idx, value) in frame.iter().enumerate().take(PANNS_MEL_BANDS) {
            let idx = frame_idx * PANNS_MEL_BANDS + mel_idx;
            out[idx] = *value;
        }
    }
    Ok(())
}

fn run_panns_inference(
    model: &panns_burn::Model<PannsBackend>,
    device: &WgpuDevice,
    input: &[f32],
    batch: usize,
) -> Result<Vec<Vec<f32>>, String> {
    let data = TensorData::new(
        input.to_vec(),
        [batch, 1, PANNS_INPUT_FRAMES, PANNS_MEL_BANDS],
    );
    run_panns_inference_from_data(model, device, data, batch)
}

fn run_panns_inference_from_data(
    model: &panns_burn::Model<PannsBackend>,
    device: &WgpuDevice,
    data: TensorData,
    batch: usize,
) -> Result<Vec<Vec<f32>>, String> {
    let output = run_panns_forward_from_data(model, device, data);
    extract_embeddings_from_data(output.into_data(), batch)
}

fn run_panns_forward_from_data(
    model: &panns_burn::Model<PannsBackend>,
    device: &WgpuDevice,
    data: TensorData,
) -> PannsOutput {
    let input_tensor = Tensor::<PannsBackend, 4>::from_data(data, device);
    model.forward(input_tensor)
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

fn init_wgpu(device: &WgpuDevice) {
    WGPU_INIT.get_or_init(|| {
        wgpu::init_setup::<Vulkan>(device, Default::default());
    });
}

fn init_cubecl_config() {
    static CUBECL_CONFIG: OnceLock<()> = OnceLock::new();
    CUBECL_CONFIG.get_or_init(|| {
        let mut config = cubecl_runtime::config::GlobalConfig::default();
        config.compilation.cache = Some(cubecl_runtime::config::cache::CacheConfig::Global);
        config.autotune.cache = cubecl_runtime::config::cache::CacheConfig::Global;
        let _ = std::panic::catch_unwind(|| cubecl_runtime::config::GlobalConfig::set(config));
    });
}

fn with_panns_model<T>(f: impl FnOnce(&mut PannsModel) -> Result<T, String>) -> Result<T, String> {
    let mutex = PANNS_MODEL.get_or_init(|| Mutex::new(None));
    let mut guard = mutex
        .lock()
        .map_err(|_| "PANNs model lock poisoned".to_string())?;
    if guard.is_none() {
        *guard = Some(PannsModel::load()?);
    }
    let model = guard.as_mut().expect("PANNs model loaded");
    f(model)
}

fn panns_batch_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        if cfg!(target_os = "windows") {
            if let Ok(value) = std::env::var("SEMPAL_PANNS_BATCH") {
                return value.trim() == "1";
            }
            return false;
        }
        match std::env::var("SEMPAL_PANNS_BATCH") {
            Ok(value) => value.trim() == "1",
            Err(_) => true,
        }
    })
}

fn panns_burnpack_path() -> Result<PathBuf, String> {
    if let Ok(path) = env::var("SEMPAL_PANNS_BURNPACK_PATH") {
        if !path.trim().is_empty() {
            return Ok(PathBuf::from(path));
        }
    }
    let generated = PathBuf::from(panns_paths::PANNS_BURNPACK_PATH);
    if generated.exists() {
        return Ok(generated);
    }
    let root = crate::app_dirs::app_root_dir().map_err(|err| err.to_string())?;
    Ok(root.join("models").join("panns_cnn14_16k.bpk"))
}

#[allow(dead_code)]
pub(crate) fn embedding_model_path() -> &'static PathBuf {
    static PATH: LazyLock<PathBuf> = LazyLock::new(|| {
        crate::app_dirs::app_root_dir()
            .map(|root| root.join("models").join("panns_cnn14_16k.bpk"))
            .unwrap_or_else(|_| PathBuf::from("panns_cnn14_16k.bpk"))
    });
    &PATH
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
        let path = std::env::var("SEMPAL_PANNS_EMBED_GOLDEN_PATH")
            .ok()
            .filter(|path| !path.trim().is_empty());
        let Some(path) = path else {
            return;
        };
        if !panns_burnpack_path().map(|p| p.exists()).unwrap_or(false) {
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
