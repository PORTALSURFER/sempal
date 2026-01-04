use std::sync::{Mutex, OnceLock};

use super::backend::{
    init_cubecl_config, init_wgpu, panns_backend_kind, PannsBackendKind, PannsCpuBackend,
    PannsCpuDevice, PannsWgpuBackend, PannsWgpuDevice,
};
#[cfg(feature = "panns-cuda")]
use super::backend::{PannsCudaBackend, PannsCudaDevice};
use super::logmel::{PANNS_LOGMEL_LEN, PANNS_SAMPLE_RATE};
use super::panns_burn;
use super::panns_burnpack_path;
use crate::analysis::panns_preprocess::{PannsPreprocessor, PANNS_STFT_HOP, PANNS_STFT_N_FFT};

pub(super) enum PannsModelInner {
    Wgpu {
        model: panns_burn::Model<PannsWgpuBackend>,
        device: PannsWgpuDevice,
    },
    Cpu {
        model: panns_burn::Model<PannsCpuBackend>,
        device: PannsCpuDevice,
    },
    #[cfg(feature = "panns-cuda")]
    Cuda {
        model: panns_burn::Model<PannsCudaBackend>,
        device: PannsCudaDevice,
    },
}

/// Loaded PANNs model plus reusable scratch buffers.
pub(crate) struct PannsModel {
    pub(super) inner: PannsModelInner,
    pub(super) input_scratch: Vec<f32>,
    #[allow(dead_code)]
    pub(super) input_batch_scratch: Vec<f32>,
    pub(super) resample_scratch: Vec<f32>,
    pub(super) wave_scratch: Vec<f32>,
    pub(super) preprocess: PannsPreprocessor,
}

static PANNS_MODEL: OnceLock<Mutex<Option<PannsModel>>> = OnceLock::new();
static PANNS_WARMED: OnceLock<()> = OnceLock::new();

impl PannsModel {
    pub(super) fn load() -> Result<Self, String> {
        let model_path = panns_burnpack_path()?;
        if !model_path.exists() {
            return Err(format!(
                "PANNs burnpack model not found at {}. Run sempal-model-setup to download it.",
                model_path.to_string_lossy()
            ));
        }
        init_cubecl_config();
        let inner = match panns_backend_kind() {
            PannsBackendKind::Wgpu => {
                let device = PannsWgpuDevice::default();
                init_wgpu(&device);
                let model = panns_burn::Model::<PannsWgpuBackend>::from_file(
                    model_path
                        .to_str()
                        .ok_or_else(|| "PANNs burnpack path contains invalid UTF-8".to_string())?,
                    &device,
                );
                PannsModelInner::Wgpu { model, device }
            }
            PannsBackendKind::Cpu => {
                let device = PannsCpuDevice::default();
                let model = panns_burn::Model::<PannsCpuBackend>::from_file(
                    model_path
                        .to_str()
                        .ok_or_else(|| "PANNs burnpack path contains invalid UTF-8".to_string())?,
                    &device,
                );
                PannsModelInner::Cpu { model, device }
            }
            #[cfg(feature = "panns-cuda")]
            PannsBackendKind::Cuda => {
                let device = PannsCudaDevice::default();
                let model = panns_burn::Model::<PannsCudaBackend>::from_file(
                    model_path
                        .to_str()
                        .ok_or_else(|| "PANNs burnpack path contains invalid UTF-8".to_string())?,
                    &device,
                );
                PannsModelInner::Cuda { model, device }
            }
        };
        Ok(Self {
            inner,
            input_scratch: vec![0.0_f32; PANNS_LOGMEL_LEN],
            input_batch_scratch: Vec::new(),
            resample_scratch: Vec::new(),
            wave_scratch: Vec::new(),
            preprocess: PannsPreprocessor::new(
                PANNS_SAMPLE_RATE,
                PANNS_STFT_N_FFT,
                PANNS_STFT_HOP,
            )?,
        })
    }
}

pub(super) fn with_panns_model<T>(f: impl FnOnce(&mut PannsModel) -> Result<T, String>) -> Result<T, String> {
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

pub(crate) fn reset_panns_model() {
    let mutex = PANNS_MODEL.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = mutex.lock() {
        *guard = None;
    }
}

pub(crate) fn try_reset_panns_model() -> bool {
    let mutex = PANNS_MODEL.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = mutex.try_lock() {
        *guard = None;
        return true;
    }
    false
}

/// Run a warm-up inference to compile kernels before measuring performance.
pub(crate) fn warmup_panns() -> Result<(), String> {
    if PANNS_WARMED.get().is_some() {
        return Ok(());
    }
    let logmel = vec![0.0_f32; PANNS_LOGMEL_LEN];
    let result = with_panns_model(|model| {
        let _ = super::infer::run_panns_inference_for_model(model, logmel.as_slice(), 1)?;
        Ok(())
    });
    if result.is_ok() {
        let _ = PANNS_WARMED.set(());
    }
    result
}
