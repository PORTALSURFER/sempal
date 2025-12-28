mod analysis;
mod app;
mod errors;
mod interaction;
mod updates;

pub use analysis::{AnalysisSettings, PannsBackendChoice, WgpuPowerPreference};
pub(crate) use app::AppSettings;
pub use app::{AppConfig, AppSettingsCore, FeatureFlags, HintSettings};
pub use errors::ConfigError;
pub use interaction::InteractionOptions;
pub use updates::{UpdateChannel, UpdateSettings};
