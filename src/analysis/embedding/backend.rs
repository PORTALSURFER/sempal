use std::env;
use std::path::PathBuf;
use std::sync::{LazyLock, OnceLock};

use burn::backend::wgpu::{self, graphics::Vulkan, WgpuDevice};
#[cfg(feature = "panns-cuda")]
use burn::backend::{cuda::CudaDevice, Cuda};
use tracing::warn;

use super::panns_paths;

pub(super) type PannsWgpuDevice = WgpuDevice;
#[cfg(feature = "panns-cuda")]
pub(super) type PannsCudaDevice = CudaDevice;

pub(super) type PannsWgpuBackend = wgpu::Wgpu;
#[cfg(feature = "panns-cuda")]
pub(super) type PannsCudaBackend = Cuda;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PannsBackendKind {
    Wgpu,
    #[cfg(feature = "panns-cuda")]
    Cuda,
}

static WGPU_INIT: OnceLock<()> = OnceLock::new();

pub(super) fn panns_backend_kind() -> PannsBackendKind {
    let requested = env::var("SEMPAL_PANNS_BACKEND")
        .ok()
        .map(|value| value.trim().to_ascii_lowercase());
    match requested.as_deref() {
        #[cfg(feature = "panns-cuda")]
        Some("cuda") => PannsBackendKind::Cuda,
        Some("wgpu") | Some("vulkan") | None => PannsBackendKind::Wgpu,
        Some(other) => {
            warn!("Unknown PANNs backend '{other}', defaulting to WGPU.");
            PannsBackendKind::Wgpu
        }
    }
}

/// Maximum micro-batch size for PANNs embedding inference.
pub(crate) fn embedding_batch_max() -> usize {
    let requested = env::var("SEMPAL_EMBEDDING_BATCH")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value >= 1);
    let default = if cfg!(target_os = "windows") { 4 } else { 16 };
    let max = requested.unwrap_or(default);
    if cfg!(target_os = "windows") {
        max.min(4)
    } else {
        max
    }
}

/// Maximum number of in-flight embedding batches for pipelined readback.
pub(crate) fn embedding_inflight_max() -> usize {
    env::var("SEMPAL_EMBEDDING_INFLIGHT")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value >= 1)
        .unwrap_or(2)
}

/// Whether to use the pipelined embedding path for overlapping readback.
pub(crate) fn embedding_pipeline_enabled() -> bool {
    env::var("SEMPAL_EMBEDDING_PIPELINE")
        .ok()
        .map(|value| value.trim().eq_ignore_ascii_case("1"))
        .unwrap_or(false)
}

pub(super) fn init_wgpu(device: &WgpuDevice) {
    WGPU_INIT.get_or_init(|| {
        wgpu::init_setup::<Vulkan>(device, Default::default());
    });
}

pub(super) fn init_cubecl_config() {
    static CUBECL_CONFIG: OnceLock<()> = OnceLock::new();
    CUBECL_CONFIG.get_or_init(|| {
        let mut config = cubecl_runtime::config::GlobalConfig::default();
        config.compilation.cache = Some(cubecl_runtime::config::cache::CacheConfig::Global);
        config.autotune.cache = cubecl_runtime::config::cache::CacheConfig::Global;
        let _ = std::panic::catch_unwind(|| cubecl_runtime::config::GlobalConfig::set(config));
    });
}

pub(super) fn panns_batch_enabled() -> bool {
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

/// Resolve the path to the PANNs burnpack, using env overrides if present.
pub(crate) fn panns_burnpack_path() -> Result<PathBuf, String> {
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

/// Default model path used by tooling that needs a stable location.
#[allow(dead_code)]
pub(crate) fn embedding_model_path() -> &'static PathBuf {
    static PATH: LazyLock<PathBuf> = LazyLock::new(|| {
        crate::app_dirs::app_root_dir()
            .map(|root| root.join("models").join("panns_cnn14_16k.bpk"))
            .unwrap_or_else(|_| PathBuf::from("panns_cnn14_16k.bpk"))
    });
    &PATH
}
