//! Application directory helpers anchored to a single `.sempal` folder.
//!
//! The helpers centralize where config and log files live across platforms,
//! defaulting to the OS config directory (e.g., `%APPDATA%` on Windows) and
//! allowing a `SEMPAL_CONFIG_HOME` override for tests or portable setups.

use std::{
    path::PathBuf,
    sync::{LazyLock, Mutex},
};

#[cfg(test)]
use std::cell::RefCell;

use directories::BaseDirs;
use thiserror::Error;

/// Name of the application directory that lives under the OS config root.
pub const APP_DIR_NAME: &str = ".sempal";

static CONFIG_BASE_OVERRIDE: LazyLock<Mutex<Option<PathBuf>>> = LazyLock::new(|| Mutex::new(None));
static APP_ROOT_OVERRIDE: LazyLock<Mutex<Option<PathBuf>>> = LazyLock::new(|| Mutex::new(None));
#[cfg(test)]
static TEST_CONFIG_BASE: LazyLock<PathBuf> = LazyLock::new(|| {
    let dir = tempfile::tempdir().expect("create test config dir");
    let path = dir.path().to_path_buf();
    // Keep the directory alive for the test process.
    std::mem::forget(dir);
    path
});

#[cfg(test)]
thread_local! {
    static TEST_CONFIG_OVERRIDE: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// Ensure tests do not touch real user config directories.
#[cfg(test)]
pub fn ensure_test_config_base() {
    let test_base = LazyLock::force(&TEST_CONFIG_BASE).clone();
    let mut guard = CONFIG_BASE_OVERRIDE
        .lock()
        .expect("config base override mutex poisoned");
    if guard.is_none() {
        *guard = Some(test_base);
    }
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
        /// Path that told the directory to be created.
        path: PathBuf,
        /// Underlying IO error.
        source: std::io::Error,
    },
}

/// Return the root `.sempal` directory, creating it if needed.
pub fn app_root_dir() -> Result<PathBuf, AppDirError> {
    #[cfg(test)]
    ensure_test_config_base();
    if let Some(path) = APP_ROOT_OVERRIDE
        .lock()
        .expect("app root override mutex poisoned")
        .clone()
    {
        std::fs::create_dir_all(&path).map_err(|source| AppDirError::CreateDir {
            path: path.clone(),
            source,
        })?;
        return Ok(path);
    }
    let base = config_base_dir().ok_or(AppDirError::NoBaseDir)?;
    let path = base.join(APP_DIR_NAME);
    std::fs::create_dir_all(&path).map_err(|source| AppDirError::CreateDir {
        path: path.clone(),
        source,
    })?;
    Ok(path)
}

/// Override the resolved application root directory (the `.sempal` folder).
pub fn set_app_root_override(path: PathBuf) -> Result<(), AppDirError> {
    std::fs::create_dir_all(&path).map_err(|source| AppDirError::CreateDir {
        path: path.clone(),
        source,
    })?;
    let mut guard = APP_ROOT_OVERRIDE
        .lock()
        .expect("app root override mutex poisoned");
    *guard = Some(path);
    Ok(())
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

/// Return the base directory used for `.sempal` when no override is set.
pub fn config_base_dir_path() -> Option<PathBuf> {
    config_base_dir()
}

fn config_base_dir() -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(path) = TEST_CONFIG_OVERRIDE.with(|override_path| override_path.borrow().clone()) {
        return Some(path);
    }
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
}

#[cfg(test)]
impl ConfigBaseGuard {
    pub fn set(path: PathBuf) -> Self {
        let previous = TEST_CONFIG_OVERRIDE.with(|override_path| {
            let mut slot = override_path.borrow_mut();
            let prev = slot.clone();
            *slot = Some(path);
            prev
        });
        Self { previous }
    }
}

#[cfg(test)]
impl Drop for ConfigBaseGuard {
    fn drop(&mut self) {
        let previous = self.previous.take();
        TEST_CONFIG_OVERRIDE.with(|override_path| {
            *override_path.borrow_mut() = previous;
        });
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

    #[test]
    fn reapplies_test_override_when_cleared() {
        {
            let mut guard = CONFIG_BASE_OVERRIDE
                .lock()
                .expect("config base override mutex poisoned");
            *guard = None;
        }
        let root = app_root_dir().unwrap();
        assert!(root.ends_with(APP_DIR_NAME));

        {
            let mut guard = CONFIG_BASE_OVERRIDE
                .lock()
                .expect("config base override mutex poisoned");
            *guard = None;
        }
        let root2 = app_root_dir().unwrap();
        assert!(root2.ends_with(APP_DIR_NAME));
    }
}
