//! Persistent settings state for the controller.

use crate::audio::{AudioInputConfig, AudioOutputConfig};
use std::path::PathBuf;

pub(in crate::egui_app::controller) struct AppSettingsState {
    pub(in crate::egui_app::controller) feature_flags: crate::sample_sources::config::FeatureFlags,
    pub(in crate::egui_app::controller) analysis: crate::sample_sources::config::AnalysisSettings,
    pub(in crate::egui_app::controller) updates: crate::sample_sources::config::UpdateSettings,
    pub(in crate::egui_app::controller) hints: crate::sample_sources::config::HintSettings,
    pub(in crate::egui_app::controller) app_data_dir: Option<PathBuf>,
    pub(in crate::egui_app::controller) audio_output: AudioOutputConfig,
    pub(in crate::egui_app::controller) audio_input: AudioInputConfig,
    pub(in crate::egui_app::controller) controls: crate::sample_sources::config::InteractionOptions,
    pub(in crate::egui_app::controller) trash_folder: Option<PathBuf>,
    pub(in crate::egui_app::controller) collection_export_root: Option<PathBuf>,
}

impl AppSettingsState {
    pub(in crate::egui_app::controller) fn new() -> Self {
        Self {
            feature_flags: crate::sample_sources::config::FeatureFlags::default(),
            analysis: crate::sample_sources::config::AnalysisSettings::default(),
            updates: crate::sample_sources::config::UpdateSettings::default(),
            hints: crate::sample_sources::config::HintSettings::default(),
            app_data_dir: None,
            audio_output: AudioOutputConfig::default(),
            audio_input: AudioInputConfig::default(),
            controls: crate::sample_sources::config::InteractionOptions::default(),
            trash_folder: None,
            collection_export_root: None,
        }
    }
}
