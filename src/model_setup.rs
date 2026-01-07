//! PANNs model download and burnpack setup helpers.

use std::{
    collections::HashSet,
    env,
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use burn_import::onnx::ModelGen;
use sha2::{Digest, Sha256};
use url::Url;

use crate::{app_dirs, http_client};

const PANNS_ONNX_NAME: &str = "panns_cnn14_16k.onnx";
const PANNS_BURNPACK_NAME: &str = "panns_cnn14_16k.bpk";
const MAX_PANNS_ONNX_BYTES: usize = 512 * 1024 * 1024;
const MAX_PANNS_DATA_BYTES: usize = 512 * 1024 * 1024;
const PANNS_ONNX_SHA256_ENV: &str = "SEMPAL_PANNS_ONNX_SHA256";
const PANNS_ONNX_ALLOWED_HOSTS_ENV: &str = "SEMPAL_PANNS_ONNX_ALLOWED_HOSTS";
const PANNS_ALLOWED_HOSTS: &[&str] = &[
    "github.com",
    "objects.githubusercontent.com",
    "raw.githubusercontent.com",
];

#[derive(Debug, Clone)]
/// Options for preparing the PANNs model and burnpack artifacts.
pub struct PannsSetupOptions {
    /// Optional HTTPS URL for downloading the ONNX model.
    pub onnx_url: Option<String>,
    /// Optional SHA-256 (hex) to verify the downloaded ONNX file.
    pub onnx_sha256: Option<String>,
    /// Optional override for the models directory location.
    pub models_dir: Option<PathBuf>,
    /// Whether to overwrite existing model artifacts.
    pub force: bool,
}

impl Default for PannsSetupOptions {
    fn default() -> Self {
        Self {
            onnx_url: None,
            onnx_sha256: None,
            models_dir: None,
            force: false,
        }
    }
}

/// Ensure the PANNs burnpack exists, downloading and converting the ONNX model if needed.
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
        let allowed_hosts = resolve_allowed_hosts();
        validate_onnx_url(&url, &allowed_hosts)?;
        let expected_sha256 = resolve_onnx_sha256(options.onnx_sha256.as_deref())?;
        download_to_path(&url, &onnx_path, &expected_sha256)?;
        let data_url = format!("{url}.data");
        validate_onnx_url(&data_url, &allowed_hosts)?;
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

/// Copy a bundled burnpack into the models directory if it is missing.
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

fn resolve_onnx_sha256(explicit: Option<&str>) -> Result<String, String> {
    if let Some(value) = explicit {
        return normalize_sha256(value);
    }
    if let Ok(value) = env::var(PANNS_ONNX_SHA256_ENV) {
        if !value.trim().is_empty() {
            return normalize_sha256(&value);
        }
    }
    if let Some(value) = option_env!("SEMPAL_PANNS_ONNX_SHA256") {
        if !value.trim().is_empty() {
            return normalize_sha256(value);
        }
    }
    Err(format!(
        "Missing PANNs ONNX SHA-256; set {PANNS_ONNX_SHA256_ENV}."
    ))
}

fn resolve_allowed_hosts() -> HashSet<String> {
    let mut hosts: HashSet<String> = PANNS_ALLOWED_HOSTS
        .iter()
        .map(|host| host.to_string())
        .collect();
    if let Ok(value) = env::var(PANNS_ONNX_ALLOWED_HOSTS_ENV) {
        for host in value.split(',') {
            let trimmed = host.trim();
            if !trimmed.is_empty() {
                hosts.insert(trimmed.to_string());
            }
        }
    }
    hosts
}

fn validate_onnx_url(url: &str, allowed_hosts: &HashSet<String>) -> Result<(), String> {
    let parsed = Url::parse(url).map_err(|err| format!("Invalid PANNs ONNX URL {url}: {err}"))?;
    if parsed.scheme() != "https" {
        return Err(format!("PANNs ONNX URL must use https: {url}"));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| format!("PANNs ONNX URL is missing a host: {url}"))?;
    if !allowed_hosts.contains(host) {
        return Err(format!(
            "PANNs ONNX URL host '{host}' is not allowlisted; set {PANNS_ONNX_ALLOWED_HOSTS_ENV} to allow it."
        ));
    }
    Ok(())
}

fn normalize_sha256(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.len() != 64 || !trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!(
            "Invalid SHA-256 value; expected 64 hex characters, got '{trimmed}'."
        ));
    }
    Ok(trimmed.to_ascii_lowercase())
}

fn download_to_path(url: &str, dest: &Path, expected_sha256: &str) -> Result<(), String> {
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
    let actual_sha256 = sha256_file(&tmp)?;
    if actual_sha256 != expected_sha256 {
        let _ = fs::remove_file(&tmp);
        return Err(format!(
            "PANNs ONNX SHA-256 mismatch: expected {expected_sha256}, got {actual_sha256}."
        ));
    }
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

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path)
        .map_err(|err| format!("Failed to read {}: {err}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buf)
            .map_err(|err| format!("Failed to read {}: {err}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
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
    use super::{normalize_sha256, resolve_onnx_sha256, resolve_onnx_url, sha256_file, validate_onnx_url};
    use std::collections::HashSet;
    use std::io::Write;

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

    #[test]
    fn resolve_onnx_sha256_requires_hex() {
        let err = resolve_onnx_sha256(Some("nope")).unwrap_err();
        assert!(err.contains("Invalid SHA-256"));
    }

    #[test]
    fn validate_onnx_url_requires_https() {
        let hosts = allowed_hosts(&["example.com"]);
        let err = validate_onnx_url("http://example.com/panns.onnx", &hosts).unwrap_err();
        assert!(err.contains("https"));
    }

    #[test]
    fn validate_onnx_url_rejects_unknown_host() {
        let hosts = allowed_hosts(&["example.com"]);
        let err =
            validate_onnx_url("https://untrusted.test/panns.onnx", &hosts).unwrap_err();
        assert!(err.contains("allowlisted"));
    }

    #[test]
    fn sha256_file_reports_hash() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "panns").unwrap();
        let hash = sha256_file(file.path()).unwrap();
        let normalized = normalize_sha256(&hash).unwrap();
        assert_eq!(hash, normalized);
    }

    fn restore_env(previous: Option<String>) {
        if let Some(value) = previous {
            unsafe { std::env::set_var("SEMPAL_PANNS_ONNX_URL", value) };
        } else {
            unsafe { std::env::remove_var("SEMPAL_PANNS_ONNX_URL") };
        }
    }

    fn allowed_hosts(values: &[&str]) -> HashSet<String> {
        values.iter().map(|host| host.to_string()).collect()
    }
}
