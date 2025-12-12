use std::{
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub struct SempalEnvGuard {
    previous: Option<String>,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl SempalEnvGuard {
    pub fn set_config_home(path: PathBuf) -> Self {
        let lock = ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let previous = std::env::var("SEMPAL_CONFIG_HOME").ok();
        // SAFETY: tests run under a global lock to prevent concurrent env mutations.
        unsafe {
            std::env::set_var("SEMPAL_CONFIG_HOME", path);
        }
        Self {
            previous,
            _lock: lock,
        }
    }
}

impl Drop for SempalEnvGuard {
    fn drop(&mut self) {
        if let Some(value) = self.previous.take() {
            // SAFETY: tests run under a global lock to prevent concurrent env mutations.
            unsafe {
                std::env::set_var("SEMPAL_CONFIG_HOME", value);
            }
        } else {
            // SAFETY: tests run under a global lock to prevent concurrent env mutations.
            unsafe {
                std::env::remove_var("SEMPAL_CONFIG_HOME");
            }
        }
    }
}
