use std::cell::RefCell;
use std::env;
use std::path::PathBuf;
use std::sync::{LazyLock, OnceLock};

use burn::backend::wgpu::{self, graphics::Vulkan, WgpuDevice};
use burn::tensor::{Tensor, TensorData};

use crate::analysis::audio;

mod clap_burn {
    include!(concat!(env!("OUT_DIR"), "/burn_clap/clap_audio.rs"));
}

mod clap_paths {
    include!(concat!(env!("OUT_DIR"), "/burn_clap/clap_paths.rs"));
}

type ClapBackend = wgpu::Wgpu;

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
    model: clap_burn::Model<ClapBackend>,
    device: WgpuDevice,
    input_scratch: Vec<f32>,
    input_batch_scratch: Vec<f32>,
    resample_scratch: Vec<f32>,
}

static WGPU_INIT: OnceLock<()> = OnceLock::new();

thread_local! {
    static TLS_CLAP_MODEL: RefCell<Option<ClapModel>> = RefCell::new(None);
}

impl ClapModel {
    pub(crate) fn load() -> Result<Self, String> {
        let model_path = clap_burnpack_path()?;
        if !model_path.exists() {
            return Err(format!(
                "CLAP burnpack model not found at {}",
                model_path.to_string_lossy()
            ));
        }
        let device = WgpuDevice::default();
        init_wgpu(&device);
        let model = clap_burn::Model::<ClapBackend>::from_file(
            model_path
                .to_str()
                .ok_or_else(|| "CLAP burnpack path contains invalid UTF-8".to_string())?,
            &device,
        );
        Ok(Self {
            model,
            device,
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
        let mut outputs = Vec::with_capacity(inputs.len());
        for chunk in inputs.chunks(embedding_batch_max()) {
            let embeddings = infer_embeddings_with_model(model, chunk)?;
            outputs.extend(embeddings);
        }
        Ok(outputs)
    })
}

fn infer_embedding_with_model(
    model: &mut ClapModel,
    samples: &[f32],
    sample_rate: u32,
) -> Result<Vec<f32>, String> {
    let input_slice = model.input_scratch.as_mut_slice();
    prepare_clap_input(
        &mut model.resample_scratch,
        input_slice,
        samples,
        sample_rate,
    );
    let mut embeddings = run_clap_inference(
        &model.model,
        &model.device,
        model.input_scratch.as_slice(),
        1,
    )?;
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
        let start = idx * CLAP_INPUT_SAMPLES;
        let end = start + CLAP_INPUT_SAMPLES;
        let out = &mut model.input_batch_scratch[start..end];
        prepare_clap_input(
            &mut model.resample_scratch,
            out,
            input.samples,
            input.sample_rate,
        );
    }
    run_clap_inference(
        &model.model,
        &model.device,
        model.input_batch_scratch.as_slice(),
        batch,
    )
}

fn prepare_clap_input(
    resample_scratch: &mut Vec<f32>,
    out: &mut [f32],
    samples: &[f32],
    sample_rate: u32,
) {
    let resampled = if sample_rate != CLAP_SAMPLE_RATE {
        audio::resample_linear_into(
            resample_scratch,
            samples,
            sample_rate,
            CLAP_SAMPLE_RATE,
        );
        resample_scratch.as_mut_slice()
    } else {
        resample_scratch.clear();
        resample_scratch.extend_from_slice(samples);
        resample_scratch.as_mut_slice()
    };
    audio::sanitize_samples_in_place(resampled);
    repeat_pad_slice(out, resampled);
}

fn run_clap_inference(
    model: &clap_burn::Model<ClapBackend>,
    device: &WgpuDevice,
    input: &[f32],
    batch: usize,
) -> Result<Vec<Vec<f32>>, String> {
    let data = TensorData::new(input.to_vec(), [batch, 1, CLAP_INPUT_SAMPLES]);
    let input_tensor = Tensor::<ClapBackend, 3>::from_data(data, device);
    let output = model.forward(input_tensor);
    extract_embeddings_from_data(output.into_data(), batch)
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

fn init_wgpu(device: &WgpuDevice) {
    WGPU_INIT.get_or_init(|| {
        wgpu::init_setup::<Vulkan>(device, Default::default());
    });
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

fn clap_burnpack_path() -> Result<PathBuf, String> {
    if let Ok(path) = env::var("SEMPAL_CLAP_BURNPACK_PATH") {
        if !path.trim().is_empty() {
            return Ok(PathBuf::from(path));
        }
    }
    let generated = PathBuf::from(clap_paths::CLAP_BURNPACK_PATH);
    if generated.exists() {
        return Ok(generated);
    }
    let root = crate::app_dirs::app_root_dir().map_err(|err| err.to_string())?;
    Ok(root.join("models").join("clap_audio.bpk"))
}

#[allow(dead_code)]
pub(crate) fn embedding_model_path() -> &'static PathBuf {
    static PATH: LazyLock<PathBuf> = LazyLock::new(|| {
        crate::app_dirs::app_root_dir()
            .map(|root| root.join("models").join("clap_audio.bpk"))
            .unwrap_or_else(|_| PathBuf::from("clap_audio.bpk"))
    });
    &PATH
}

fn extract_embeddings_from_data(data: TensorData, batch: usize) -> Result<Vec<Vec<f32>>, String> {
    let shape = data.shape.clone();
    let flat = data
        .as_slice::<f32>()
        .map_err(|err| format!("Failed to read Burn output tensor: {err}"))?;
    if shape.is_empty() {
        return Err("CLAP output tensor has empty shape".to_string());
    }
    if shape.len() == 1 {
        if batch != 1 || flat.len() < EMBEDDING_DIM {
            return Err("CLAP output tensor has unexpected shape".to_string());
        }
        let mut pooled = flat[..EMBEDDING_DIM].to_vec();
        normalize_l2_in_place(&mut pooled);
        let norm = l2_norm(&pooled);
        if !norm.is_finite() || (norm - 1.0).abs() > 1e-3 {
            return Err(format!("CLAP embedding L2 norm out of range: {norm:.6}"));
        }
        return Ok(vec![pooled]);
    }
    let batch_dim = shape[0];
    if batch_dim != batch {
        return Err(format!(
            "CLAP output batch mismatch: expected {batch}, got {batch_dim}"
        ));
    }
    let embedding_dim = *shape.last().unwrap_or(&0);
    if embedding_dim != EMBEDDING_DIM {
        return Err(format!(
            "CLAP output embedding dim mismatch: expected {EMBEDDING_DIM}, got {embedding_dim}"
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
        return Err("CLAP output tensor shorter than expected".to_string());
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
        let path = std::env::var("SEMPAL_CLAP_EMBED_GOLDEN_PATH")
            .ok()
            .filter(|path| !path.trim().is_empty())
            .unwrap_or_else(|| "tests/golden_embedding.json".to_string());
        if !clap_burnpack_path().map(|p| p.exists()).unwrap_or(false) {
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
