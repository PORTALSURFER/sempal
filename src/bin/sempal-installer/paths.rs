use std::{env, path::PathBuf};

use crate::APP_NAME;

pub(crate) fn default_install_dir() -> PathBuf {
    if let Ok(local_app_data) = env::var("LOCALAPPDATA") {
        return PathBuf::from(local_app_data)
            .join("Programs")
            .join(APP_NAME);
    }
    if let Ok(program_files) = env::var("ProgramFiles") {
        return PathBuf::from(program_files).join(APP_NAME);
    }
    PathBuf::from("C:\\Program Files").join(APP_NAME)
}

pub(crate) fn default_bundle_dir() -> PathBuf {
    env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("bundle")))
        .unwrap_or_else(|| PathBuf::from("bundle"))
}

pub(crate) fn start_menu_dir() -> Option<PathBuf> {
    env::var("APPDATA").ok().map(|root| {
        PathBuf::from(root)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join(APP_NAME)
    })
}
