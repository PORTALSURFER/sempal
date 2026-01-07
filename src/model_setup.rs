use std::{
    env,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};

use burn_import::onnx::ModelGen;

use crate::{app_dirs, http_client};

const PANNS_ONNX_NAME: &str = "panns_cnn14_16k.onnx";
const PANNS_BURNPACK_NAME: &str = "panns_cnn14_16k.bpk";
const MAX_PANNS_ONNX_BYTES: usize = 512 * 1024 * 1024;
const MAX_PANNS_DATA_BYTES: usize = 512 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct PannsSetupOptions {
    pub onnx_url: Option<String>,
    pub models_dir: Option<PathBuf>,
    pub force: bool,
}

impl Default for PannsSetupOptions {
    fn default() -> Self {
        Self {
            onnx_url: None,
            models_dir: None,
            force: false,
        }
    }
}

pub fn ensure_panns_burnpack(options: PannsSetupOptions) -> Result<PathBuf, String> {
    let models_dir = resolve_models_dir(options.models_dir)?;
    let onnx_path = models_dir.join(PANNS_ONNX_NAME);
    let burnpack_path = models_dir.join(PANNS_BURNPACK_NAME);

    if burnpack_path.exists() && !options.force {
        return Ok(burnpack_path);
    }

    if !onnx_path.exists() || options.force {
        let url = resolve_onnx_url(options.onnx_url.as_deref()).ok_or_else(|| {
            "Missing PANNs ONNX URL; set SEMPAL_PANNS_ONNX_URL.".to_string()
        })?;
        download_to_path(&url, &onnx_path)?;
        let data_url = format!("{url}.data");
        let data_path = PathBuf::from(format!("{}.data", onnx_path.display()));
        let _ = download_optional(&data_url, &data_path);
    }

    generate_burnpack(&onnx_path, &models_dir)?;
    if !burnpack_path.exists() {
        return Err(format!(
            "Burnpack not generated at {}",
            burnpack_path.display()
        ));
    }
    Ok(burnpack_path)
}

pub fn sync_bundled_burnpack() -> Result<bool, String> {
    let models_dir = resolve_models_dir(None)?;
    let target = models_dir.join(PANNS_BURNPACK_NAME);
    if target.exists() {
        return Ok(false);
    }
    let Some(source) = bundled_burnpack_path() else {
        return Ok(false);
    };
    fs::copy(&source, &target).map_err(|err| {
        format!(
            "Failed to copy bundled burnpack from {}: {err}",
            source.display()
        )
    })?;
    Ok(true)
}

fn bundled_burnpack_path() -> Option<PathBuf> {
    let exe = env::current_exe().ok()?;
    let exe_dir = exe.parent()?;
    let bundled = exe_dir.join("models").join(PANNS_BURNPACK_NAME);
    if bundled.exists() {
        Some(bundled)
    } else {
        None
    }
}

fn resolve_models_dir(override_dir: Option<PathBuf>) -> Result<PathBuf, String> {
    let root = match override_dir {
        Some(path) => path,
        None => app_dirs::app_root_dir().map_err(|err| err.to_string())?,
    };
    let models_dir = root.join("models");
    fs::create_dir_all(&models_dir)
        .map_err(|err| format!("Failed to create models dir {}: {err}", models_dir.display()))?;
    Ok(models_dir)
}

fn resolve_onnx_url(explicit: Option<&str>) -> Option<String> {
    if let Some(value) = explicit {
        let value = value.trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    if let Ok(value) = env::var("SEMPAL_PANNS_ONNX_URL") {
        let value = value.trim().to_string();
        if !value.is_empty() {
            return Some(value);
        }
    }
    option_env!("SEMPAL_PANNS_ONNX_URL")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn download_to_path(url: &str, dest: &Path) -> Result<(), String> {
    let response = http_client::agent()
        .get(url)
        .call()
        .map_err(|err| format!("Failed to download {url}: {err}"))?;
    if response.status() >= 400 {
        return Err(format!(
            "Failed to download {url}: HTTP {}",
            response.status()
        ));
    }
    let tmp = dest.with_extension("tmp");
    let mut file = File::create(&tmp)
        .map_err(|err| format!("Failed to write {}: {err}", tmp.display()))?;
    http_client::copy_response_to_writer(response, &mut file, MAX_PANNS_ONNX_BYTES)
        .map_err(|err| format!("Failed to write {}: {err}", tmp.display()))?;
    file.flush()
        .map_err(|err| format!("Failed to flush {}: {err}", tmp.display()))?;
    fs::rename(&tmp, dest)
        .map_err(|err| format!("Failed to move {}: {err}", dest.display()))?;
    Ok(())
}

fn download_optional(url: &str, dest: &Path) -> Result<(), String> {
    match http_client::agent().get(url).call() {
        Ok(response) => {
            if response.status() >= 400 {
                return Err(format!("Failed to download {url}: HTTP {}", response.status()));
            }
            let tmp = dest.with_extension("tmp");
            let mut file = File::create(&tmp)
                .map_err(|err| format!("Failed to write {}: {err}", tmp.display()))?;
            http_client::copy_response_to_writer(response, &mut file, MAX_PANNS_DATA_BYTES)
                .map_err(|err| format!("Failed to write {}: {err}", tmp.display()))?;
            file.flush()
                .map_err(|err| format!("Failed to flush {}: {err}", tmp.display()))?;
            fs::rename(&tmp, dest)
                .map_err(|err| format!("Failed to move {}: {err}", dest.display()))?;
            Ok(())
        }
        Err(ureq::Error::Status(_, response)) => {
            if response.status() == 404 {
                return Ok(());
            }
            Err(format!("Failed to download {url}: HTTP {}", response.status()))
        }
        Err(err) => Err(format!("Failed to download {url}: {err}")),
    }
}

fn generate_burnpack(onnx_path: &Path, models_dir: &Path) -> Result<(), String> {
    let onnx_path = onnx_path
        .to_str()
        .ok_or_else(|| "PANNs ONNX path contains invalid UTF-8".to_string())?;
    let out_dir = models_dir
        .to_str()
        .ok_or_else(|| "Models dir contains invalid UTF-8".to_string())?;
    let result = std::panic::catch_unwind(|| {
        ModelGen::new()
            .input(onnx_path)
            .out_dir(out_dir)
            .run_from_cli();
    });
    if result.is_err() {
        return Err("Failed to convert ONNX to BurnPack".to_string());
    }
    let model_rs = models_dir.join("panns_cnn14_16k.rs");
    if model_rs.exists() {
        let _ = fs::remove_file(model_rs);
    }
    let onnx_txt = models_dir.join("panns_cnn14_16k.onnx.txt");
    if onnx_txt.exists() {
        let _ = fs::remove_file(onnx_txt);
    }
    let graph_txt = models_dir.join("panns_cnn14_16k.graph.txt");
    if graph_txt.exists() {
        let _ = fs::remove_file(graph_txt);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::resolve_onnx_url;

    #[test]
    fn resolve_onnx_url_prefers_explicit() {
        let prev = std::env::var("SEMPAL_PANNS_ONNX_URL").ok();
        unsafe { std::env::set_var("SEMPAL_PANNS_ONNX_URL", "env") };
        let url = resolve_onnx_url(Some("explicit"));
        assert_eq!(url.as_deref(), Some("explicit"));
        restore_env(prev);
    }

    #[test]
    fn resolve_onnx_url_uses_env() {
        let prev = std::env::var("SEMPAL_PANNS_ONNX_URL").ok();
        unsafe { std::env::set_var("SEMPAL_PANNS_ONNX_URL", "env") };
        let url = resolve_onnx_url(None);
        assert_eq!(url.as_deref(), Some("env"));
        restore_env(prev);
    }


    fn restore_env(previous: Option<String>) {
        if let Some(value) = previous {
            unsafe { std::env::set_var("SEMPAL_PANNS_ONNX_URL", value) };
        } else {
            unsafe { std::env::remove_var("SEMPAL_PANNS_ONNX_URL") };
        }
    }

}
