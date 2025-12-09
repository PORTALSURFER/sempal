//! Application directory helpers anchored to a single `.sempal` folder.
//!
//! The helpers centralize where config and log files live across platforms,
//! defaulting to the OS config directory (e.g., `%APPDATA%` on Windows) and
//! allowing a `SEMPAL_CONFIG_HOME` override for tests or portable setups.

use std::{
    path::PathBuf,
    sync::{LazyLock, Mutex},
};

use directories::BaseDirs;
use thiserror::Error;

/// Name of the application directory that lives under the OS config root.
pub const APP_DIR_NAME: &str = ".sempal";

static CONFIG_BASE_OVERRIDE: LazyLock<Mutex<Option<PathBuf>>> = LazyLock::new(|| Mutex::new(None));

/// Errors that can occur while resolving or preparing application directories.
#[derive(Debug, Error)]
pub enum AppDirError {
    /// No suitable base config directory could be resolved.
    #[error("No suitable base config directory available for application files")]
    NoBaseDir,
    /// Failed to create the application directory.
    #[error("Failed to create application directory at {path}: {source}")]
    CreateDir {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// Return the root `.sempal` directory, creating it if needed.
pub fn app_root_dir() -> Result<PathBuf, AppDirError> {
    let base = config_base_dir().ok_or(AppDirError::NoBaseDir)?;
    let path = base.join(APP_DIR_NAME);
    std::fs::create_dir_all(&path).map_err(|source| AppDirError::CreateDir {
        path: path.clone(),
        source,
    })?;
    Ok(path)
}

/// Return the logs directory inside the `.sempal` root, creating it if needed.
pub fn logs_dir() -> Result<PathBuf, AppDirError> {
    let path = app_root_dir()?.join("logs");
    std::fs::create_dir_all(&path).map_err(|source| AppDirError::CreateDir {
        path: path.clone(),
        source,
    })?;
    Ok(path)
}

fn config_base_dir() -> Option<PathBuf> {
    if let Some(path) = CONFIG_BASE_OVERRIDE
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
    {
        return Some(path);
    }
    if let Ok(path) = std::env::var("SEMPAL_CONFIG_HOME") {
        return Some(PathBuf::from(path));
    }
    BaseDirs::new().map(|dirs| dirs.config_dir().to_path_buf())
}

#[cfg(test)]
fn set_config_base_override(path: PathBuf) {
    let mut guard = CONFIG_BASE_OVERRIDE
        .lock()
        .expect("config base override mutex poisoned");
    *guard = Some(path);
}

#[cfg(test)]
fn clear_config_base_override() {
    let mut guard = CONFIG_BASE_OVERRIDE
        .lock()
        .expect("config base override mutex poisoned");
    *guard = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    struct OverrideGuard;

    impl OverrideGuard {
        fn set(path: PathBuf) -> Self {
            set_config_base_override(path);
            Self
        }
    }

    impl Drop for OverrideGuard {
        fn drop(&mut self) {
            clear_config_base_override();
        }
    }

    #[test]
    fn uses_override_for_root_dir() {
        let base = tempdir().unwrap();
        let _guard = OverrideGuard::set(base.path().to_path_buf());
        let root = app_root_dir().unwrap();
        assert_eq!(root, base.path().join(APP_DIR_NAME));
        assert!(root.is_dir());
    }
}
