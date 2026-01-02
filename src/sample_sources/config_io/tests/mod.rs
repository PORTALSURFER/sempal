use std::path::{Path, PathBuf};

use tempfile::TempDir;

pub(super) struct TestConfigEnv {
    dir: TempDir,
    _guard: crate::app_dirs::ConfigBaseGuard,
}

impl TestConfigEnv {
    pub(super) fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let guard = crate::app_dirs::ConfigBaseGuard::set(dir.path().to_path_buf());
        Self { dir, _guard: guard }
    }

    pub(super) fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    pub(super) fn ensure_app_dir(&self) -> PathBuf {
        let path = self.dir.path().join(crate::app_dirs::APP_DIR_NAME);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    pub(super) fn write(&self, path: &Path, data: &str) {
        std::fs::write(path, data).unwrap();
    }
}

mod legacy;
mod load;
mod save;
