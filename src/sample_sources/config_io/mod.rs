use crate::app_dirs;

use super::config_types::ConfigError;

mod legacy;
mod load;
mod save;

#[cfg(test)]
mod tests;

/// Default filename used to store the app configuration.
pub const CONFIG_FILE_NAME: &str = "config.toml";
/// Legacy filename for migration support.
pub const LEGACY_CONFIG_FILE_NAME: &str = "config.json";

pub use load::{config_path, load_or_default, normalize_path};
pub use save::{save, save_to_path};

fn map_app_dir_error(error: app_dirs::AppDirError) -> ConfigError {
    match error {
        app_dirs::AppDirError::NoBaseDir => ConfigError::NoConfigDir,
        app_dirs::AppDirError::CreateDir { path, source } => {
            ConfigError::CreateDir { path, source }
        }
    }
}
