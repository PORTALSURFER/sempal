use std::env;
use std::path::PathBuf;
use std::sync::{LazyLock, OnceLock};

use burn::backend::ndarray::{NdArray, NdArrayDevice};
#[cfg(target_os = "macos")]
use burn::backend::wgpu::{self, WgpuDevice, graphics::Metal};
#[cfg(not(target_os = "macos"))]
use burn::backend::wgpu::{self, WgpuDevice, graphics::Vulkan};
#[cfg(feature = "panns-cuda")]
use burn::backend::{Cuda, cuda::CudaDevice};
use tracing::warn;

use super::panns_paths;

pub(super) type PannsWgpuDevice = WgpuDevice;
pub(super) type PannsCpuDevice = NdArrayDevice;
#[cfg(feature = "panns-cuda")]
pub(super) type PannsCudaDevice = CudaDevice;

pub(super) type PannsWgpuBackend = wgpu::Wgpu;
pub(super) type PannsCpuBackend = NdArray;
#[cfg(feature = "panns-cuda")]
pub(super) type PannsCudaBackend = Cuda;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PannsBackendKind {
    Wgpu,
    Cpu,
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
        Some("cpu") | Some("ndarray") => PannsBackendKind::Cpu,
        Some("wgpu") | Some("vulkan") | Some("metal") | None => PannsBackendKind::Wgpu,
        Some(other) => {
            warn!("Unknown PANNs backend '{other}', defaulting to WGPU.");
            PannsBackendKind::Wgpu
        }
    }
}

pub(super) fn init_wgpu(device: &WgpuDevice) {
    WGPU_INIT.get_or_init(|| {
        #[cfg(target_os = "macos")]
        wgpu::init_setup::<Metal>(device, Default::default());
        #[cfg(not(target_os = "macos"))]
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
