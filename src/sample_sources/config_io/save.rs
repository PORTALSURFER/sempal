use std::io::Write;
use std::path::Path;

use super::super::config_types::{AppConfig, AppSettings, ConfigError};
use super::load::config_path;

/// Persist configuration to disk, overwriting any previous contents.
///
/// Settings are written to TOML while sources are stored in SQLite.
pub fn save(config: &AppConfig) -> Result<(), ConfigError> {
    let path = config_path()?;
    save_to_path(config, &path)
}

/// Save configuration to a specific path, creating parent directories as needed.
pub fn save_to_path(config: &AppConfig, path: &Path) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| ConfigError::CreateDir {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let settings = AppSettings::from(config);
    save_settings_to_path(&settings, path)?;
    crate::sample_sources::library::save(&crate::sample_sources::library::LibraryState {
        sources: config.sources.clone(),
    })?;
    Ok(())
}

/// Write the TOML settings file atomically to prevent partial writes on crash.
pub(super) fn save_settings_to_path(settings: &AppSettings, path: &Path) -> Result<(), ConfigError> {
    let data = toml::to_string_pretty(settings).map_err(|source| ConfigError::SerializeToml {
        path: path.to_path_buf(),
        source,
    })?;
    atomic_write(path, data.as_bytes())
}

fn atomic_write(path: &Path, data: &[u8]) -> Result<(), ConfigError> {
    use rand::TryRngCore;
    let dir = path.parent().ok_or_else(|| ConfigError::Write {
        path: path.to_path_buf(),
        source: std::io::Error::new(
            std::io::ErrorKind::Other,
            "config path has no parent directory",
        ),
    })?;
    let file_name = path.file_name().ok_or_else(|| ConfigError::Write {
        path: path.to_path_buf(),
        source: std::io::Error::new(
            std::io::ErrorKind::Other,
            "config path has no file name",
        ),
    })?;

    let mut last_err = None;
    for _ in 0..5 {
        let mut bytes = [0u8; 6];
        rand::rngs::OsRng.try_fill_bytes(&mut bytes).map_err(|source| ConfigError::Write {
            path: path.to_path_buf(),
            source: std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("failed to generate temporary file suffix: {source}"),
            ),
        })?;
        let suffix: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
        let tmp_path = dir.join(format!("{}.tmp-{}", file_name.to_string_lossy(), suffix));

        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path);

        let mut file = match file {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                last_err = Some(err);
                continue;
            }
            Err(err) => {
                return Err(ConfigError::Write {
                    path: tmp_path.clone(),
                    source: err,
                });
            }
        };

        if let Err(err) = file.write_all(data) {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(ConfigError::Write {
                path: tmp_path.clone(),
                source: err,
            });
        }
        if let Err(err) = file.sync_all() {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(ConfigError::Write {
                path: tmp_path.clone(),
                source: err,
            });
        }
        drop(file);
        if let Err(err) = replace_file(&tmp_path, path) {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(ConfigError::Write {
                path: path.to_path_buf(),
                source: err,
            });
        }
        sync_parent_dir(dir)?;
        return Ok(());
    }

    Err(ConfigError::Write {
        path: path.to_path_buf(),
        source: std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!(
                "failed to create temporary file for {}: {}",
                path.display(),
                last_err
                    .as_ref()
                    .map(|err| err.to_string())
                    .unwrap_or_else(|| "unknown error".into())
            ),
        ),
    })
}

fn replace_file(temp_path: &Path, path: &Path) -> Result<(), std::io::Error> {
    match std::fs::rename(temp_path, path) {
        Ok(()) => Ok(()),
        Err(err) => {
            #[cfg(target_os = "windows")]
            if err.kind() == std::io::ErrorKind::AlreadyExists
                || err.kind() == std::io::ErrorKind::PermissionDenied
            {
                if let Err(inner) = std::fs::remove_file(path) {
                    if inner.kind() != std::io::ErrorKind::NotFound {
                        return Err(inner);
                    }
                }
                std::fs::rename(temp_path, path)?;
                return Ok(());
            }
            Err(err)
        }
    }
}

fn sync_parent_dir(dir: &Path) -> Result<(), ConfigError> {
    #[cfg(unix)]
    {
        let dir_handle = std::fs::File::open(dir).map_err(|source| ConfigError::Write {
            path: dir.to_path_buf(),
            source,
        })?;
        dir_handle
            .sync_all()
            .map_err(|source| ConfigError::Write {
                path: dir.to_path_buf(),
                source,
            })?;
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
    }
    Ok(())
}
