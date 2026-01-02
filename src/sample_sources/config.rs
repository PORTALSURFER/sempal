#[path = "config_defaults.rs"]
mod config_defaults;
mod config_io;
#[path = "config_types/mod.rs"]
mod config_types;

pub use config_io::{
    CONFIG_FILE_NAME, LEGACY_CONFIG_FILE_NAME, config_path, load_or_default, normalize_path, save,
    save_to_path,
};
pub use config_types::{
    AnalysisSettings, AppConfig, AppSettingsCore, ConfigError, FeatureFlags, HintSettings,
    InteractionOptions, PannsBackendChoice, UpdateChannel, UpdateSettings, WgpuPowerPreference,
};
