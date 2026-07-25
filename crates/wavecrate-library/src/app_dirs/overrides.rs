//! Scoped override guards for application-directory resolution.

use std::path::PathBuf;

use super::{
    AppDirError,
    state::{IGNORE_GLOBAL_APP_ROOT_OVERRIDE, SCOPED_APP_ROOT_OVERRIDE, TEST_CONFIG_OVERRIDE},
};

/// Guard that pins application-root resolution on the current thread.
///
/// Runtime owners use this to propagate an already-resolved persistence root
/// into child threads without changing process-wide environment variables.
pub struct AppRootGuard {
    previous: Option<PathBuf>,
}

impl AppRootGuard {
    /// Override the application root for the lifetime of the current-thread guard.
    pub fn set(path: PathBuf) -> Result<Self, AppDirError> {
        std::fs::create_dir_all(&path).map_err(|source| AppDirError::CreateDir {
            path: path.clone(),
            source,
        })?;
        let previous = SCOPED_APP_ROOT_OVERRIDE.with(|override_path| {
            let mut slot = override_path.borrow_mut();
            let previous = slot.clone();
            *slot = Some(path);
            previous
        });
        Ok(Self { previous })
    }
}

impl Drop for AppRootGuard {
    fn drop(&mut self) {
        let previous = self.previous.take();
        SCOPED_APP_ROOT_OVERRIDE.with(|override_path| {
            *override_path.borrow_mut() = previous;
        });
    }
}

/// Guard that sets a temporary config base path for tests and restores the prior value.
pub struct ConfigBaseGuard {
    previous: Option<PathBuf>,
    previous_scoped_root: Option<PathBuf>,
    previous_ignore_global_root: bool,
}

impl ConfigBaseGuard {
    /// Override the config base directory for the lifetime of the guard.
    pub fn set(path: PathBuf) -> Self {
        let previous = TEST_CONFIG_OVERRIDE.with(|override_path| {
            let mut slot = override_path.borrow_mut();
            let prev = slot.clone();
            *slot = Some(path);
            prev
        });
        let previous_scoped_root = SCOPED_APP_ROOT_OVERRIDE.with(|override_path| {
            let mut slot = override_path.borrow_mut();
            let prev = slot.clone();
            *slot = None;
            prev
        });
        let previous_ignore_global_root =
            IGNORE_GLOBAL_APP_ROOT_OVERRIDE.with(|ignore| ignore.replace(true));
        Self {
            previous,
            previous_scoped_root,
            previous_ignore_global_root,
        }
    }
}

impl Drop for ConfigBaseGuard {
    fn drop(&mut self) {
        let previous = self.previous.take();
        TEST_CONFIG_OVERRIDE.with(|override_path| {
            *override_path.borrow_mut() = previous;
        });
        let previous_scoped_root = self.previous_scoped_root.take();
        SCOPED_APP_ROOT_OVERRIDE.with(|override_path| {
            *override_path.borrow_mut() = previous_scoped_root;
        });
        IGNORE_GLOBAL_APP_ROOT_OVERRIDE.with(|ignore| ignore.set(self.previous_ignore_global_root));
    }
}
