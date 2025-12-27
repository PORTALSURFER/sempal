use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{audio::AudioOutputConfig, waveform::WaveformChannelView};

use super::config_defaults::{
    clamp_analysis_worker_count, clamp_volume, default_analysis_worker_count,
    default_anti_clip_fade_ms, default_audio_output, default_keyboard_zoom_factor,
    default_fast_similarity_prep_sample_rate, default_false, default_max_analysis_duration_seconds,
    default_scroll_speed, default_true, default_volume, default_wheel_zoom_factor,
    default_bpm_value, default_transient_sensitivity,
};
use crate::sample_sources::{Collection, SampleSource, SourceId};

/// Aggregate application state loaded from disk.
///
/// Config keys (TOML): `feature_flags`, `analysis`, `updates`, `hints`, `app_data_dir`,
/// `trash_folder`, `collection_export_root`, `last_selected_source`, `volume`,
/// `audio_output`, `controls`.
///
/// `sources` and `collections` are stored in the library database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub sources: Vec<SampleSource>,
    pub collections: Vec<Collection>,
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
    pub trash_folder: Option<PathBuf>,
    /// Optional default root used when creating collection export folders.
    #[serde(default)]
    pub collection_export_root: Option<PathBuf>,
    pub last_selected_source: Option<SourceId>,
    #[serde(default = "default_audio_output")]
    pub audio_output: AudioOutputConfig,
    pub volume: f32,
    #[serde(default)]
    pub controls: InteractionOptions,
}

/// App settings that belong in the TOML config file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct AppSettings {
    #[serde(default)]
    pub feature_flags: FeatureFlags,
    #[serde(default)]
    pub analysis: AnalysisSettings,
    #[serde(default)]
    pub updates: UpdateSettings,
    #[serde(default)]
    pub hints: HintSettings,
    #[serde(default)]
    pub app_data_dir: Option<PathBuf>,
    #[serde(default)]
    pub trash_folder: Option<PathBuf>,
    #[serde(default)]
    pub collection_export_root: Option<PathBuf>,
    #[serde(default)]
    pub last_selected_source: Option<SourceId>,
    #[serde(default = "default_volume")]
    pub volume: f32,
    #[serde(default = "default_audio_output")]
    pub audio_output: AudioOutputConfig,
    #[serde(default)]
    pub controls: InteractionOptions,
}

impl AppSettings {
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

/// Global preferences for analysis and feature extraction.
///
/// Config keys: `max_analysis_duration_seconds`, `analysis_worker_count`,
/// `limit_similarity_prep_duration`, `fast_similarity_prep`, `fast_similarity_prep_sample_rate`,
/// `panns_backend`, `wgpu_power_preference`, `wgpu_adapter_name`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisSettings {
    /// Skip analysis for files longer than this many seconds.
    #[serde(default = "default_max_analysis_duration_seconds")]
    pub max_analysis_duration_seconds: f32,
    /// Apply the duration cap when preparing similarity search.
    #[serde(default = "default_true")]
    pub limit_similarity_prep_duration: bool,
    /// Analysis worker count override (0 = auto).
    #[serde(default = "default_analysis_worker_count")]
    pub analysis_worker_count: u32,
    /// Use a faster, lower-quality analysis pass during similarity prep.
    #[serde(default = "default_false")]
    pub fast_similarity_prep: bool,
    /// Sample rate used during fast similarity prep analysis.
    #[serde(default = "default_fast_similarity_prep_sample_rate")]
    pub fast_similarity_prep_sample_rate: u32,
    /// Backend used for PANNs inference.
    #[serde(default)]
    pub panns_backend: PannsBackendChoice,
    /// WGPU adapter power preference when using the WGPU backend.
    #[serde(default)]
    pub wgpu_power_preference: WgpuPowerPreference,
    /// Optional WGPU adapter name override (substring match).
    #[serde(default)]
    pub wgpu_adapter_name: Option<String>,
}

impl Default for AnalysisSettings {
    fn default() -> Self {
        Self {
            max_analysis_duration_seconds: default_max_analysis_duration_seconds(),
            limit_similarity_prep_duration: default_true(),
            analysis_worker_count: default_analysis_worker_count(),
            fast_similarity_prep: default_false(),
            fast_similarity_prep_sample_rate: default_fast_similarity_prep_sample_rate(),
            panns_backend: PannsBackendChoice::default(),
            wgpu_power_preference: WgpuPowerPreference::default(),
            wgpu_adapter_name: None,
        }
    }
}

/// Select which backend to use for PANNs inference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PannsBackendChoice {
    Wgpu,
    Cpu,
    Cuda,
}

impl Default for PannsBackendChoice {
    fn default() -> Self {
        Self::Wgpu
    }
}

impl PannsBackendChoice {
    pub fn as_env(&self) -> &'static str {
        match self {
            Self::Wgpu => "wgpu",
            Self::Cpu => "cpu",
            Self::Cuda => "cuda",
        }
    }

    pub fn from_env(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "wgpu" | "vulkan" | "metal" => Some(Self::Wgpu),
            "cpu" | "ndarray" => Some(Self::Cpu),
            "cuda" => Some(Self::Cuda),
            _ => None,
        }
    }
}

/// WGPU adapter power preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WgpuPowerPreference {
    Default,
    Low,
    High,
}

impl Default for WgpuPowerPreference {
    fn default() -> Self {
        Self::Default
    }
}

impl WgpuPowerPreference {
    pub fn as_env(&self) -> Option<&'static str> {
        match self {
            Self::Default => None,
            Self::Low => Some("low"),
            Self::High => Some("high"),
        }
    }

    pub fn from_env(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "low" | "low-power" | "lowpower" => Some(Self::Low),
            "high" | "high-performance" | "highperformance" => Some(Self::High),
            _ => None,
        }
    }
}

/// Persisted preferences for update checks.
///
/// Config keys: `channel`, `check_on_startup`, `last_seen_nightly_published_at`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSettings {
    #[serde(default)]
    pub channel: UpdateChannel,
    #[serde(default = "default_true")]
    pub check_on_startup: bool,
    #[serde(default)]
    pub last_seen_nightly_published_at: Option<String>,
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

impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            channel: UpdateChannel::Stable,
            check_on_startup: true,
            last_seen_nightly_published_at: None,
        }
    }
}

/// Update channel selection for GitHub releases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    Stable,
    Nightly,
}

impl Default for UpdateChannel {
    fn default() -> Self {
        Self::Stable
    }
}

/// Interaction tuning for waveform navigation.
///
/// Config keys: `invert_waveform_scroll`, `waveform_scroll_speed`,
/// `wheel_zoom_factor`, `keyboard_zoom_factor`, `anti_clip_fade_enabled`,
/// `anti_clip_fade_ms`, `destructive_yolo_mode`, `waveform_channel_view`,
/// `bpm_snap_enabled`, `bpm_value`, `transient_markers_enabled`, `transient_snap_enabled`,
/// `transient_sensitivity`, `transient_realtime_enabled`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionOptions {
    #[serde(default = "default_true")]
    pub invert_waveform_scroll: bool,
    #[serde(default = "default_scroll_speed")]
    pub waveform_scroll_speed: f32,
    #[serde(default = "default_wheel_zoom_factor")]
    pub wheel_zoom_factor: f32,
    #[serde(default = "default_keyboard_zoom_factor")]
    pub keyboard_zoom_factor: f32,
    #[serde(default = "default_true")]
    pub anti_clip_fade_enabled: bool,
    #[serde(default = "default_anti_clip_fade_ms")]
    pub anti_clip_fade_ms: f32,
    #[serde(default)]
    pub destructive_yolo_mode: bool,
    #[serde(default)]
    pub waveform_channel_view: WaveformChannelView,
    #[serde(default = "default_false")]
    pub bpm_snap_enabled: bool,
    #[serde(default = "default_bpm_value")]
    pub bpm_value: f32,
    #[serde(default = "default_false")]
    pub transient_snap_enabled: bool,
    #[serde(default = "default_true")]
    pub transient_markers_enabled: bool,
    #[serde(default = "default_transient_sensitivity")]
    pub transient_sensitivity: f32,
    #[serde(default = "default_false")]
    pub transient_realtime_enabled: bool,
}

impl Default for InteractionOptions {
    fn default() -> Self {
        Self {
            invert_waveform_scroll: true,
            waveform_scroll_speed: default_scroll_speed(),
            wheel_zoom_factor: default_wheel_zoom_factor(),
            keyboard_zoom_factor: default_keyboard_zoom_factor(),
            anti_clip_fade_enabled: true,
            anti_clip_fade_ms: default_anti_clip_fade_ms(),
            destructive_yolo_mode: false,
            waveform_channel_view: WaveformChannelView::Mono,
            bpm_snap_enabled: default_false(),
            bpm_value: default_bpm_value(),
            transient_snap_enabled: default_false(),
            transient_markers_enabled: default_true(),
            transient_sensitivity: default_transient_sensitivity(),
            transient_realtime_enabled: default_false(),
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
            feature_flags: FeatureFlags::default(),
            analysis: AnalysisSettings::default(),
            updates: UpdateSettings::default(),
            hints: HintSettings::default(),
            app_data_dir: None,
            trash_folder: None,
            collection_export_root: None,
            last_selected_source: None,
            audio_output: default_audio_output(),
            volume: default_volume(),
            controls: InteractionOptions::default(),
        }
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            feature_flags: FeatureFlags::default(),
            analysis: AnalysisSettings::default(),
            updates: UpdateSettings::default(),
            hints: HintSettings::default(),
            app_data_dir: None,
            trash_folder: None,
            collection_export_root: None,
            last_selected_source: None,
            audio_output: default_audio_output(),
            volume: default_volume(),
            controls: InteractionOptions::default(),
        }
    }
}

/// Errors that may occur while loading or saving app configuration.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Unable to create config directory {path}: {source}")]
    CreateDir {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Failed to read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Failed to write {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Invalid config at {path}: {source}")]
    ParseToml {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error("Invalid legacy config at {path}: {source}")]
    ParseJson {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("Failed to serialize config to TOML at {path}: {source}")]
    SerializeToml {
        path: PathBuf,
        source: toml::ser::Error,
    },
    #[error("Failed to migrate legacy config from {path}: {source}")]
    LegacyMigration {
        path: PathBuf,
        source: Box<ConfigError>,
    },
    #[error("Failed to back up legacy config {path} to {backup_path}: {source}")]
    BackupLegacy {
        path: PathBuf,
        backup_path: PathBuf,
        source: std::io::Error,
    },
    #[error("No suitable config directory found")]
    NoConfigDir,
    #[error("Library database error: {0}")]
    Library(#[from] crate::sample_sources::library::LibraryError),
}
