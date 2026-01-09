use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{
    audio::{AudioInputConfig, AudioOutputConfig},
    sample_sources::library::LibraryState,
    sample_sources::{Collection, SampleSource, SourceId},
};

use super::super::config_defaults::{
    clamp_analysis_worker_count, clamp_volume, default_audio_input, default_audio_output,
    default_true, default_volume,
};
use super::{AnalysisSettings, InteractionOptions, UpdateSettings};

/// Aggregate application state loaded from disk.
///
/// Config keys (TOML): `feature_flags`, `analysis`, `updates`, `hints`, `app_data_dir`,
/// `trash_folder`, `collection_export_root`, `drop_targets`, `last_selected_source`,
/// `volume`, `audio_output`, `audio_input`, `controls`.
///
/// `sources` and `collections` are stored in the library database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub sources: Vec<SampleSource>,
    pub collections: Vec<Collection>,
    #[serde(default, flatten)]
    pub core: AppSettingsCore,
}

/// App settings that belong in the TOML config file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AppSettings {
    #[serde(default, flatten)]
    pub core: AppSettingsCore,
}

impl AppSettings {
    pub(crate) fn normalized(self) -> Self {
        Self {
            core: self.core.normalized(),
        }
    }
}

impl From<&AppConfig> for AppSettings {
    fn from(config: &AppConfig) -> Self {
        Self {
            core: config.core.clone(),
        }
    }
}

impl From<(AppSettings, LibraryState)> for AppConfig {
    fn from((settings, library): (AppSettings, LibraryState)) -> Self {
        Self {
            sources: library.sources,
            collections: library.collections,
            core: settings.core,
        }
    }
}

/// Shared config fields used across app config surfaces.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettingsCore {
    #[serde(default)]
    pub feature_flags: FeatureFlags,
    #[serde(default)]
    pub analysis: AnalysisSettings,
    #[serde(default)]
    pub updates: UpdateSettings,
    #[serde(default)]
    pub hints: HintSettings,
    /// Optional override for the `.sempal` data folder.
    #[serde(default)]
    pub app_data_dir: Option<PathBuf>,
    #[serde(default)]
    pub trash_folder: Option<PathBuf>,
    /// Optional default root used when creating collection export folders.
    #[serde(default)]
    pub collection_export_root: Option<PathBuf>,
    /// User-defined drop target folders used by the sidebar.
    #[serde(default)]
    pub drop_targets: Vec<PathBuf>,
    #[serde(default)]
    pub last_selected_source: Option<SourceId>,
    #[serde(default = "default_audio_output")]
    pub audio_output: AudioOutputConfig,
    #[serde(default = "default_audio_input")]
    pub audio_input: AudioInputConfig,
    #[serde(default = "default_volume")]
    pub volume: f32,
    #[serde(default)]
    pub controls: InteractionOptions,
}

impl AppSettingsCore {
    pub(super) fn normalized(mut self) -> Self {
        self.volume = clamp_volume(self.volume);
        self.analysis.analysis_worker_count =
            clamp_analysis_worker_count(self.analysis.analysis_worker_count);
        self
    }
}

/// Toggleable features that can be persisted and evolve without breaking old configs.
///
/// Config keys: `collections_enabled`, `autoplay_selection`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFlags {
    #[serde(default = "default_true")]
    pub collections_enabled: bool,
    #[serde(default = "default_true")]
    pub autoplay_selection: bool,
}

/// Persisted preferences for the hint-of-the-day popup.
///
/// Config keys: `show_on_startup`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HintSettings {
    #[serde(default = "default_true")]
    pub show_on_startup: bool,
}

impl Default for HintSettings {
    fn default() -> Self {
        Self {
            show_on_startup: true,
        }
    }
}

impl Default for FeatureFlags {
    fn default() -> Self {
        Self {
            collections_enabled: true,
            autoplay_selection: true,
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            sources: Vec::new(),
            collections: Vec::new(),
            core: AppSettingsCore::default(),
        }
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            core: AppSettingsCore::default(),
        }
    }
}

impl Default for AppSettingsCore {
    fn default() -> Self {
        Self {
            feature_flags: FeatureFlags::default(),
            analysis: AnalysisSettings::default(),
            updates: UpdateSettings::default(),
            hints: HintSettings::default(),
            app_data_dir: None,
            trash_folder: None,
            collection_export_root: None,
            drop_targets: Vec::new(),
            last_selected_source: None,
            audio_output: default_audio_output(),
            audio_input: default_audio_input(),
            volume: default_volume(),
            controls: InteractionOptions::default(),
        }
    }
}
