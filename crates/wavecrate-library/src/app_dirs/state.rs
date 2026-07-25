use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    sync::{LazyLock, Mutex},
};

use super::profile::ProfileSelection;

pub(super) static CONFIG_BASE_OVERRIDE: LazyLock<Mutex<Option<PathBuf>>> =
    LazyLock::new(|| Mutex::new(None));
pub(super) static APP_ROOT_OVERRIDE: LazyLock<Mutex<Option<PathBuf>>> =
    LazyLock::new(|| Mutex::new(None));
pub(super) static TEST_CONFIG_BASE: LazyLock<PathBuf> = LazyLock::new(|| {
    let path =
        std::env::temp_dir().join(format!("wavecrate-automated-tests-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&path);
    path
});

thread_local! {
    pub(super) static TEST_CONFIG_OVERRIDE: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}
thread_local! {
    pub(super) static SCOPED_APP_ROOT_OVERRIDE: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}
thread_local! {
    pub(super) static IGNORE_GLOBAL_APP_ROOT_OVERRIDE: Cell<bool> = const { Cell::new(false) };
}
thread_local! {
    pub(super) static SCOPED_PROFILE_OVERRIDE: RefCell<Option<ProfileSelection>> = const { RefCell::new(None) };
}
