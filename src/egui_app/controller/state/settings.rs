//! Persistent settings state for the controller.

use crate::audio::{AudioInputConfig, AudioOutputConfig};
use crate::sample_sources::config::DropTargetConfig;
use std::path::PathBuf;

pub(crate) struct AppSettingsState {
    pub(crate) feature_flags: crate::sample_sources::config::FeatureFlags,
    pub(crate) analysis: crate::sample_sources::config::AnalysisSettings,
    pub(crate) updates: crate::sample_sources::config::UpdateSettings,
    pub(crate) app_data_dir: Option<PathBuf>,
    pub(crate) audio_output: AudioOutputConfig,
    pub(crate) audio_input: AudioInputConfig,
    pub(crate) controls: crate::sample_sources::config::InteractionOptions,
    pub(crate) trash_folder: Option<PathBuf>,
    pub(crate) collection_export_root: Option<PathBuf>,
    pub(crate) drop_targets: Vec<DropTargetConfig>,
}

impl AppSettingsState {
    pub(crate) fn new() -> Self {
        Self {
            feature_flags: crate::sample_sources::config::FeatureFlags::default(),
            analysis: crate::sample_sources::config::AnalysisSettings::default(),
            updates: crate::sample_sources::config::UpdateSettings::default(),
            app_data_dir: None,
            audio_output: AudioOutputConfig::default(),
            audio_input: AudioInputConfig::default(),
            controls: crate::sample_sources::config::InteractionOptions::default(),
            trash_folder: None,
            collection_export_root: None,
            drop_targets: Vec::new(),
        }
    }
}
