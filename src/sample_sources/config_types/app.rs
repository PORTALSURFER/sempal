use std::path::PathBuf;

use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    audio::{AudioInputConfig, AudioOutputConfig},
    sample_sources::library::LibraryState,
    sample_sources::{SampleSource, SourceId},
};

use super::super::config_defaults::{
    clamp_analysis_worker_count, clamp_job_message_queue_capacity, clamp_volume,
    default_audio_input, default_audio_output, default_job_message_queue_capacity, default_true,
    default_volume,
};
use super::{AnalysisSettings, InteractionOptions, UpdateSettings};

/// Aggregate application state loaded from disk.
///
/// Config keys (TOML): `feature_flags`, `analysis`, `updates`, `app_data_dir`,
/// `trash_folder`, `drop_targets`, `last_selected_source`,
/// `volume`, `audio_output`, `audio_input`, `controls`, `job_message_queue_capacity`.
///
/// `sources` are stored in the library database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Sample sources loaded from the library database.
    pub sources: Vec<SampleSource>,
    #[serde(default, flatten)]
    /// Core settings persisted in the config file.
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
            core: settings.core,
        }
    }
}

/// Shared config fields used across app config surfaces.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettingsCore {
    #[serde(default)]
    /// Feature flags for experimental or optional UI behavior.
    pub feature_flags: FeatureFlags,
    #[serde(default)]
    /// Analysis settings.
    pub analysis: AnalysisSettings,
    #[serde(default)]
    /// Update check settings.
    pub updates: UpdateSettings,
    #[serde(default = "default_job_message_queue_capacity")]
    /// Maximum number of pending controller job messages.
    pub job_message_queue_capacity: u32,
    /// Optional override for the `.sempal` data folder.
    #[serde(default)]
    pub app_data_dir: Option<PathBuf>,
    #[serde(default)]
    /// Optional trash folder path.
    pub trash_folder: Option<PathBuf>,
    /// User-defined drop target folders used by the sidebar, with optional colors.
    #[serde(default, deserialize_with = "deserialize_drop_targets")]
    /// Drop target configurations for the sidebar.
    pub drop_targets: Vec<DropTargetConfig>,
    #[serde(default)]
    /// Last selected source id.
    pub last_selected_source: Option<SourceId>,
    #[serde(default = "default_audio_output")]
    /// Output audio configuration.
    pub audio_output: AudioOutputConfig,
    #[serde(default = "default_audio_input")]
    /// Input audio configuration.
    pub audio_input: AudioInputConfig,
    #[serde(default = "default_volume")]
    /// Master volume (0.0-1.0).
    pub volume: f32,
    #[serde(default)]
    /// Interaction option defaults.
    pub controls: InteractionOptions,
}

impl AppSettingsCore {
    pub(super) fn normalized(mut self) -> Self {
        self.volume = clamp_volume(self.volume);
        self.analysis.analysis_worker_count =
            clamp_analysis_worker_count(self.analysis.analysis_worker_count);
        self.job_message_queue_capacity =
            clamp_job_message_queue_capacity(self.job_message_queue_capacity);
        self
    }
}

/// Persisted color choices for drop target rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DropTargetColor {
    /// Mint accent color.
    Mint,
    /// Ice accent color.
    Ice,
    /// Copper accent color.
    Copper,
    /// Fog accent color.
    Fog,
    /// Amber accent color.
    Amber,
    /// Rose accent color.
    Rose,
    /// Spruce accent color.
    Spruce,
    /// Clay accent color.
    Clay,
}

/// Config data for a single drop target.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropTargetConfig {
    /// Folder path that receives dropped samples.
    pub path: PathBuf,
    /// Optional display color selected for the target.
    pub color: Option<DropTargetColor>,
}

impl DropTargetConfig {
    /// Build a drop target entry for the given path, with no color assigned.
    pub fn new(path: PathBuf) -> Self {
        Self { path, color: None }
    }
}

/// Toggleable features that can be persisted and evolve without breaking old configs.
///
/// Config keys: `autoplay_selection`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFlags {
    #[serde(default = "default_true")]
    /// Auto-play when selection changes.
    pub autoplay_selection: bool,
}


impl Default for FeatureFlags {
    fn default() -> Self {
        Self {
            autoplay_selection: true,
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            sources: Vec::new(),
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
            job_message_queue_capacity: default_job_message_queue_capacity(),
            app_data_dir: None,
            trash_folder: None,
            drop_targets: Vec::new(),
            last_selected_source: None,
            audio_output: default_audio_output(),
            audio_input: default_audio_input(),
            volume: default_volume(),
            controls: InteractionOptions::default(),
        }
    }
}

fn deserialize_drop_targets<'de, D>(deserializer: D) -> Result<Vec<DropTargetConfig>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum DropTargetEntry {
        Path(PathBuf),
        Config(DropTargetConfig),
    }

    let items = Option::<Vec<DropTargetEntry>>::deserialize(deserializer)?.unwrap_or_default();
    Ok(items
        .into_iter()
        .map(|item| match item {
            DropTargetEntry::Path(path) => DropTargetConfig::new(path),
            DropTargetEntry::Config(config) => config,
        })
        .collect())
}
