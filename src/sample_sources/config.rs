#[path = "config_defaults.rs"]
mod config_defaults;
#[path = "config_io.rs"]
mod config_io;
#[path = "config_types.rs"]
mod config_types;

pub use config_io::{
    config_path,
    normalize_path,
    save,
    save_to_path,
    CONFIG_FILE_NAME,
    LEGACY_CONFIG_FILE_NAME,
    load_or_default,
};
pub use config_types::{
    AnalysisSettings,
    AppConfig,
    ConfigError,
    FeatureFlags,
    InteractionOptions,
    UpdateChannel,
    UpdateSettings,
};
