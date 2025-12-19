use std::path::PathBuf;
use std::sync::LazyLock;

use crate::analysis::audio;
use crate::analysis::tflite_runtime::TfliteRuntime;

pub(crate) const EMBEDDING_MODEL_ID: &str = "yamnet_v1";
pub(crate) const EMBEDDING_DIM: usize = 1024;
pub(crate) const EMBEDDING_DTYPE_F32: i64 = 0;
const YAMNET_INPUT_SAMPLES: usize = 15_600;

pub(crate) struct YamnetModel {
    runtime: TfliteRuntime,
}

impl YamnetModel {
    pub(crate) fn load() -> Result<Self, String> {
        let model_path = yamnet_model_path()?;
        if !model_path.exists() {
            return Err(format!(
                "YAMNet model not found at {}",
                model_path.to_string_lossy()
            ));
        }
        let runtime_path = tflite_runtime_path()?;
        if !runtime_path.exists() {
            return Err(format!(
                "TFLite runtime not found at {}",
                runtime_path.to_string_lossy()
            ));
        }
        let threads = std::thread::available_parallelism()
            .map(|n| n.get().saturating_sub(1).max(1))
            .unwrap_or(1) as i32;
        let runtime = TfliteRuntime::load(&model_path, &runtime_path, threads)?;
        Ok(Self { runtime })
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
        let embedding = model.runtime.run(&input)?;
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
    Ok(root.join("models").join("yamnet.tflite"))
}

fn tflite_runtime_path() -> Result<PathBuf, String> {
    let root = crate::app_dirs::app_root_dir().map_err(|err| err.to_string())?;
    Ok(root.join("models").join("tflite").join(tflite_runtime_filename()))
}

fn tflite_runtime_filename() -> &'static str {
    if cfg!(target_os = "windows") {
        "tensorflowlite_c.dll"
    } else if cfg!(target_os = "macos") {
        "libtensorflowlite_c.dylib"
    } else {
        "libtensorflowlite_c.so"
    }
}

pub(crate) fn embedding_model_path() -> &'static PathBuf {
    static PATH: LazyLock<PathBuf> = LazyLock::new(|| {
        crate::app_dirs::app_root_dir()
            .map(|root| root.join("models").join("yamnet.tflite"))
            .unwrap_or_else(|_| PathBuf::from("yamnet.tflite"))
    });
    &PATH
}
