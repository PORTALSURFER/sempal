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
// Prevent concurrent overrides from clobbering each other during tests.
#[cfg(test)]
static CONFIG_GUARD_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
#[cfg(test)]
static TEST_CONFIG_INIT: LazyLock<()> = LazyLock::new(|| {
    let dir = tempfile::tempdir().expect("create test config dir");
    let path = dir.path().to_path_buf();
    // Keep the directory alive for the test process.
    std::mem::forget(dir);
    let mut guard = CONFIG_BASE_OVERRIDE
        .lock()
        .expect("config base override mutex poisoned");
    if guard.is_none() {
        *guard = Some(path);
    }
});

/// Ensure tests do not touch real user config directories.
#[cfg(test)]
pub fn ensure_test_config_base() {
    LazyLock::force(&TEST_CONFIG_INIT);
}

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
    #[cfg(test)]
    ensure_test_config_base();
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

/// Guard that sets a temporary config base path for tests and restores the prior value.
#[cfg(test)]
pub struct ConfigBaseGuard {
    previous: Option<PathBuf>,
    _lock: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl ConfigBaseGuard {
    pub fn set(path: PathBuf) -> Self {
        let lock = CONFIG_GUARD_LOCK
            .lock()
            .expect("config guard lock poisoned");
        let mut guard = CONFIG_BASE_OVERRIDE
            .lock()
            .expect("config base override mutex poisoned");
        let previous = guard.clone();
        *guard = Some(path);
        Self {
            previous,
            _lock: lock,
        }
    }
}

#[cfg(test)]
impl Drop for ConfigBaseGuard {
    fn drop(&mut self) {
        if let Ok(mut guard) = CONFIG_BASE_OVERRIDE.lock() {
            *guard = self.previous.take();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn uses_override_for_root_dir() {
        let base = tempdir().unwrap();
        let _guard = ConfigBaseGuard::set(base.path().to_path_buf());
        let root = app_root_dir().unwrap();
        assert_eq!(root, base.path().join(APP_DIR_NAME));
        assert!(root.is_dir());
    }
}
