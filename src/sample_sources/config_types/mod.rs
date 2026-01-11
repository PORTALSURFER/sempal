mod analysis;
mod app;
mod errors;
mod interaction;
mod updates;

pub use analysis::{AnalysisSettings, WgpuPowerPreference};
pub(crate) use app::AppSettings;
pub use app::{
    AppConfig, AppSettingsCore, DropTargetColor, DropTargetConfig, FeatureFlags,
};
pub use errors::ConfigError;
pub use interaction::InteractionOptions;
pub use updates::{UpdateChannel, UpdateSettings};
