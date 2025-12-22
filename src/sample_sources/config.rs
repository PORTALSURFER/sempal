use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize, de::Error as SerdeDeError};
use thiserror::Error;

use crate::{app_dirs, audio::AudioOutputConfig, waveform::WaveformChannelView};

use super::{Collection, SampleSource};

/// Default filename used to store the app configuration.
pub const CONFIG_FILE_NAME: &str = "config.toml";
/// Legacy filename for migration support.
pub const LEGACY_CONFIG_FILE_NAME: &str = "config.json";

/// Aggregate application state loaded from disk (settings from TOML, data from SQLite).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub sources: Vec<SampleSource>,
    pub collections: Vec<Collection>,
    pub feature_flags: FeatureFlags,
    #[serde(default)]
    pub model: ModelSettings,
    #[serde(default)]
    pub training: TrainingSettings,
    #[serde(default)]
    pub analysis: AnalysisSettings,
    #[serde(default)]
    pub updates: UpdateSettings,
    pub trash_folder: Option<PathBuf>,
    /// Optional default root used when creating collection export folders.
    #[serde(default)]
    pub collection_export_root: Option<PathBuf>,
    pub last_selected_source: Option<super::SourceId>,
    #[serde(default = "default_audio_output")]
    pub audio_output: AudioOutputConfig,
    pub volume: f32,
    #[serde(default)]
    pub controls: InteractionOptions,
}

/// App settings that belong in the TOML config file.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppSettings {
    #[serde(default)]
    pub feature_flags: FeatureFlags,
    #[serde(default)]
    pub model: ModelSettings,
    #[serde(default)]
    pub training: TrainingSettings,
    #[serde(default)]
    pub analysis: AnalysisSettings,
    #[serde(default)]
    pub updates: UpdateSettings,
    #[serde(default)]
    pub trash_folder: Option<PathBuf>,
    #[serde(default)]
    pub collection_export_root: Option<PathBuf>,
    #[serde(default)]
    pub last_selected_source: Option<super::SourceId>,
    #[serde(default = "default_volume")]
    pub volume: f32,
    #[serde(default = "default_audio_output")]
    pub audio_output: AudioOutputConfig,
    #[serde(default)]
    pub controls: InteractionOptions,
}

/// Toggleable features that can be persisted and evolve without breaking old configs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFlags {
    #[serde(default = "default_true")]
    pub collections_enabled: bool,
    #[serde(default = "default_true")]
    pub autoplay_selection: bool,
}

/// Global model inference preferences.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSettings {
    /// Below this confidence, predictions are assigned to `UNKNOWN`.
    #[serde(default = "default_unknown_confidence_threshold")]
    pub unknown_confidence_threshold: f32,
    /// Preferred classifier model id to use for predictions.
    #[serde(default = "default_classifier_model_id")]
    pub classifier_model_id: String,
    /// Prefer user overrides when displaying categories.
    #[serde(default = "default_use_user_overrides")]
    pub use_user_overrides: bool,
}

impl Default for ModelSettings {
    fn default() -> Self {
        Self {
            unknown_confidence_threshold: default_unknown_confidence_threshold(),
            classifier_model_id: default_classifier_model_id(),
            use_user_overrides: default_use_user_overrides(),
        }
    }
}

/// Controls how in-app retraining selects weak labels from filenames/folders.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingSettings {
    /// Minimum `labels_weak.confidence` to include when exporting a training dataset.
    #[serde(default = "default_retrain_min_confidence")]
    pub retrain_min_confidence: f32,
    /// Folder depth used to derive `pack_id` and split datasets to prevent leakage.
    #[serde(default = "default_retrain_pack_depth")]
    pub retrain_pack_depth: usize,
    /// Include user override labels when exporting training data.
    #[serde(default = "default_use_user_labels")]
    pub use_user_labels: bool,
    /// Model type used for in-app retraining.
    #[serde(default = "default_training_model_kind")]
    pub model_kind: TrainingModelKind,
    /// Minimum samples required per class in curated datasets.
    #[serde(default = "default_training_min_class_samples")]
    pub min_class_samples: usize,
    /// Use hybrid embeddings + lightweight DSP features when training.
    #[serde(default)]
    pub use_hybrid_features: bool,
    /// Training-time audio augmentation controls.
    #[serde(default)]
    pub augmentation: TrainingAugmentation,
    /// Optional folder used for curated training data.
    #[serde(default)]
    pub training_dataset_root: Option<PathBuf>,
}

impl Default for TrainingSettings {
    fn default() -> Self {
        Self {
            retrain_min_confidence: default_retrain_min_confidence(),
            retrain_pack_depth: default_retrain_pack_depth(),
            use_user_labels: default_use_user_labels(),
            model_kind: default_training_model_kind(),
            min_class_samples: default_training_min_class_samples(),
            use_hybrid_features: false,
            augmentation: TrainingAugmentation::default(),
            training_dataset_root: None,
        }
    }
}

/// Training-time audio augmentation knobs for curated datasets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingAugmentation {
    /// Enable augmentation during training dataset ingestion.
    #[serde(default)]
    pub enabled: bool,
    /// Extra augmented copies per sample.
    #[serde(default = "default_augmentation_copies")]
    pub copies_per_sample: usize,
    /// Gain jitter range in dB (+/-).
    #[serde(default = "default_augmentation_gain_db")]
    pub gain_jitter_db: f32,
    /// Added noise standard deviation.
    #[serde(default = "default_augmentation_noise_std")]
    pub noise_std: f32,
    /// Pitch shift range in semitones (+/-).
    #[serde(default = "default_augmentation_pitch_semitones")]
    pub pitch_semitones: f32,
    /// Time-stretch range in percent (+/-).
    #[serde(default = "default_augmentation_time_stretch_pct")]
    pub time_stretch_pct: f32,
    /// Apply extra trim/normalize preprocessing before embedding.
    #[serde(default)]
    pub preprocess: bool,
}

impl Default for TrainingAugmentation {
    fn default() -> Self {
        Self {
            enabled: false,
            copies_per_sample: default_augmentation_copies(),
            gain_jitter_db: default_augmentation_gain_db(),
            noise_std: default_augmentation_noise_std(),
            pitch_semitones: default_augmentation_pitch_semitones(),
            time_stretch_pct: default_augmentation_time_stretch_pct(),
            preprocess: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrainingModelKind {
    GbdtStumpV1,
    MlpV1,
    LogRegV1,
}

/// Global preferences for analysis and feature extraction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisSettings {
    /// Skip feature extraction and predictions for files longer than this many seconds.
    #[serde(default = "default_max_analysis_duration_seconds")]
    pub max_analysis_duration_seconds: f32,
    /// Analysis worker count override (0 = auto).
    #[serde(default = "default_analysis_worker_count")]
    pub analysis_worker_count: u32,
    /// Aggregation strategy for training-free label scoring.
    #[serde(default)]
    pub tf_label_aggregation: TfLabelAggregationMode,
}

impl Default for AnalysisSettings {
    fn default() -> Self {
        Self {
            max_analysis_duration_seconds: default_max_analysis_duration_seconds(),
            analysis_worker_count: default_analysis_worker_count(),
            tf_label_aggregation: TfLabelAggregationMode::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TfLabelAggregationMode {
    MeanTopK,
    Max,
}

impl Default for TfLabelAggregationMode {
    fn default() -> Self {
        Self::MeanTopK
    }
}

/// Persisted preferences for update checks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSettings {
    #[serde(default)]
    pub channel: UpdateChannel,
    #[serde(default = "default_true")]
    pub check_on_startup: bool,
    #[serde(default)]
    pub last_seen_nightly_published_at: Option<String>,
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
    #[serde(default)]
    pub destructive_yolo_mode: bool,
    #[serde(default)]
    pub waveform_channel_view: WaveformChannelView,
}

impl Default for InteractionOptions {
    fn default() -> Self {
        Self {
            invert_waveform_scroll: true,
            waveform_scroll_speed: default_scroll_speed(),
            wheel_zoom_factor: default_wheel_zoom_factor(),
            keyboard_zoom_factor: default_keyboard_zoom_factor(),
            destructive_yolo_mode: false,
            waveform_channel_view: WaveformChannelView::Mono,
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

/// Resolve the configuration file path, ensuring the parent directory exists.
pub fn config_path() -> Result<PathBuf, ConfigError> {
    let dir = app_dirs::app_root_dir().map_err(map_app_dir_error)?;
    Ok(dir.join(CONFIG_FILE_NAME))
}

/// Resolve the legacy JSON configuration path used before migration.
fn legacy_config_path() -> Result<PathBuf, ConfigError> {
    let dir = app_dirs::app_root_dir().map_err(map_app_dir_error)?;
    Ok(dir.join(LEGACY_CONFIG_FILE_NAME))
}

/// Load configuration from disk, returning defaults if missing.
///
/// This pulls settings from a TOML file and data from the SQLite library database.
/// If a legacy `config.json` exists, it will be migrated into the new layout.
pub fn load_or_default() -> Result<AppConfig, ConfigError> {
    let settings_path = config_path()?;
    let legacy_path = legacy_config_path()?;
    let settings = if settings_path.exists() {
        load_settings_from(&settings_path)?
    } else {
        migrate_legacy_config(&legacy_path, &settings_path)?
    };

    let library = crate::sample_sources::library::load()?;
    Ok(AppConfig {
        sources: library.sources,
        collections: library.collections,
        feature_flags: settings.feature_flags,
        model: settings.model,
        training: settings.training,
        analysis: settings.analysis,
        updates: settings.updates,
        trash_folder: settings.trash_folder,
        collection_export_root: settings.collection_export_root,
        last_selected_source: settings.last_selected_source,
        audio_output: settings.audio_output,
        volume: settings.volume,
        controls: settings.controls,
    })
}

/// Persist configuration to disk, overwriting any previous contents.
///
/// Settings are written to TOML while sources/collections are stored in SQLite.
pub fn save(config: &AppConfig) -> Result<(), ConfigError> {
    let path = config_path()?;
    save_to_path(config, &path)
}

/// Save configuration to a specific path, creating parent directories as needed.
pub fn save_to_path(config: &AppConfig, path: &Path) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| ConfigError::CreateDir {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    save_settings_to_path(
        &AppSettings {
            feature_flags: config.feature_flags.clone(),
            model: config.model.clone(),
            training: config.training.clone(),
            analysis: config.analysis.clone(),
            updates: config.updates.clone(),
            trash_folder: config.trash_folder.clone(),
            collection_export_root: config.collection_export_root.clone(),
            last_selected_source: config.last_selected_source.clone(),
            volume: config.volume,
            audio_output: config.audio_output.clone(),
            controls: config.controls.clone(),
        },
        path,
    )?;
    crate::sample_sources::library::save(&crate::sample_sources::library::LibraryState {
        sources: config.sources.clone(),
        collections: config.collections.clone(),
    })?;
    Ok(())
}

fn load_settings_from(path: &Path) -> Result<AppSettings, ConfigError> {
    if !path.exists() {
        return Ok(AppSettings::default());
    }
    let bytes = std::fs::read(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let text = String::from_utf8(bytes).map_err(|source| ConfigError::ParseToml {
        path: path.to_path_buf(),
        source: SerdeDeError::custom(source),
    })?;
    toml::from_str(&text).map_err(|source| ConfigError::ParseToml {
        path: path.to_path_buf(),
        source,
    })
}

fn migrate_legacy_config(legacy_path: &Path, new_path: &Path) -> Result<AppSettings, ConfigError> {
    if !legacy_path.exists() {
        return Ok(AppSettings::default());
    }
    let legacy = load_legacy_from(legacy_path).map_err(|source| ConfigError::LegacyMigration {
        path: legacy_path.to_path_buf(),
        source: Box::new(source),
    })?;
    crate::sample_sources::library::save(&crate::sample_sources::library::LibraryState {
        sources: legacy.sources.clone(),
        collections: legacy.collections.clone(),
    })?;
    let settings = AppSettings {
        feature_flags: legacy.feature_flags,
        model: ModelSettings::default(),
        training: TrainingSettings::default(),
        analysis: AnalysisSettings::default(),
        updates: UpdateSettings::default(),
        trash_folder: legacy.trash_folder,
        collection_export_root: None,
        last_selected_source: legacy.last_selected_source,
        audio_output: legacy.audio_output,
        volume: legacy.volume,
        controls: InteractionOptions::default(),
    };
    save_settings_to_path(&settings, new_path)?;
    backup_legacy_file(legacy_path)?;
    Ok(settings)
}

fn backup_legacy_file(path: &Path) -> Result<(), ConfigError> {
    let backup_path = path.with_extension("json.bak");
    if backup_path.exists() {
        std::fs::remove_file(&backup_path).map_err(|source| ConfigError::BackupLegacy {
            path: path.to_path_buf(),
            backup_path: backup_path.clone(),
            source,
        })?;
    }
    std::fs::rename(path, &backup_path).map_err(|source| ConfigError::BackupLegacy {
        path: path.to_path_buf(),
        backup_path,
        source,
    })
}

fn save_settings_to_path(settings: &AppSettings, path: &Path) -> Result<(), ConfigError> {
    let data = toml::to_string_pretty(settings).map_err(|source| ConfigError::SerializeToml {
        path: path.to_path_buf(),
        source,
    })?;
    std::fs::write(path, data).map_err(|source| ConfigError::Write {
        path: path.to_path_buf(),
        source,
    })
}

fn load_legacy_from(path: &Path) -> Result<AppConfig, ConfigError> {
    let bytes = std::fs::read(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| ConfigError::ParseJson {
        path: path.to_path_buf(),
        source,
    })
}

/// Utility to convert absolute paths to strings for serialization durability.
pub fn normalize_path(path: &Path) -> PathBuf {
    PathBuf::from_iter(path.components())
}

fn default_true() -> bool {
    true
}

fn default_audio_output() -> AudioOutputConfig {
    AudioOutputConfig::default()
}

fn default_unknown_confidence_threshold() -> f32 {
    0.8
}

fn default_classifier_model_id() -> String {
    crate::ml::logreg::DEFAULT_CLASSIFIER_MODEL_ID.to_string()
}

fn default_use_user_overrides() -> bool {
    true
}

fn default_retrain_min_confidence() -> f32 {
    0.75
}

fn default_retrain_pack_depth() -> usize {
    1
}

fn default_use_user_labels() -> bool {
    true
}

fn default_training_model_kind() -> TrainingModelKind {
    TrainingModelKind::MlpV1
}

fn default_training_min_class_samples() -> usize {
    30
}

fn default_augmentation_copies() -> usize {
    1
}

fn default_augmentation_gain_db() -> f32 {
    2.0
}

fn default_augmentation_noise_std() -> f32 {
    0.002
}

fn default_augmentation_pitch_semitones() -> f32 {
    1.5
}

fn default_augmentation_time_stretch_pct() -> f32 {
    0.05
}

fn default_max_analysis_duration_seconds() -> f32 {
    30.0
}

fn default_analysis_worker_count() -> u32 {
    0
}

fn default_volume() -> f32 {
    1.0
}

fn default_scroll_speed() -> f32 {
    1.2
}

fn default_wheel_zoom_factor() -> f32 {
    0.96
}

fn default_keyboard_zoom_factor() -> f32 {
    0.9
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            sources: Vec::new(),
            collections: Vec::new(),
            feature_flags: FeatureFlags::default(),
            model: ModelSettings::default(),
            training: TrainingSettings::default(),
            analysis: AnalysisSettings::default(),
            updates: UpdateSettings::default(),
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
            model: ModelSettings::default(),
            training: TrainingSettings::default(),
            analysis: AnalysisSettings::default(),
            updates: UpdateSettings::default(),
            trash_folder: None,
            collection_export_root: None,
            last_selected_source: None,
            audio_output: default_audio_output(),
            volume: default_volume(),
            controls: InteractionOptions::default(),
        }
    }
}

fn map_app_dir_error(error: app_dirs::AppDirError) -> ConfigError {
    match error {
        app_dirs::AppDirError::NoBaseDir => ConfigError::NoConfigDir,
        app_dirs::AppDirError::CreateDir { path, source } => {
            ConfigError::CreateDir { path, source }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn with_config_home<T>(dir: &Path, f: impl FnOnce() -> T) -> T {
        let _guard = crate::app_dirs::ConfigBaseGuard::set(dir.to_path_buf());
        f()
    }

    #[test]
    fn saves_settings_to_toml() {
        let dir = tempdir().unwrap();
        with_config_home(dir.path(), || {
            let path = dir.path().join("cfg.toml");
            let cfg = AppConfig {
                volume: 0.42,
                trash_folder: Some(PathBuf::from("trash")),
                ..AppConfig::default()
            };
            save_to_path(&cfg, &path).unwrap();
            let loaded = super::load_settings_from(&path).unwrap();
            assert!((loaded.volume - 0.42).abs() < f32::EPSILON);
            assert_eq!(loaded.trash_folder, Some(PathBuf::from("trash")));
        });
    }

    #[test]
    fn migrates_from_legacy_json() {
        let dir = tempdir().unwrap();
        with_config_home(dir.path(), || {
            let legacy_path = dir
                .path()
                .join(app_dirs::APP_DIR_NAME)
                .join(LEGACY_CONFIG_FILE_NAME);
            std::fs::create_dir_all(legacy_path.parent().unwrap()).unwrap();
            let legacy = AppConfig {
                sources: vec![SampleSource::new(PathBuf::from("old_source"))],
                collections: vec![Collection::new("Old Collection")],
                feature_flags: FeatureFlags::default(),
                model: ModelSettings::default(),
                training: TrainingSettings::default(),
                analysis: AnalysisSettings::default(),
                updates: UpdateSettings::default(),
                trash_folder: Some(PathBuf::from("trash_here")),
                collection_export_root: None,
                last_selected_source: None,
                audio_output: default_audio_output(),
                volume: 0.9,
                controls: InteractionOptions::default(),
            };
            let data = serde_json::to_vec_pretty(&legacy).unwrap();
            std::fs::write(&legacy_path, data).unwrap();

            let loaded = load_or_default().unwrap();
            assert_eq!(loaded.sources.len(), 1);
            assert_eq!(loaded.collections.len(), 1);
            assert_eq!(loaded.trash_folder, Some(PathBuf::from("trash_here")));

            let backup = legacy_path.with_extension("json.bak");
            assert!(backup.exists(), "expected backup file {}", backup.display());
        });
    }

    #[test]
    fn volume_defaults_and_persists() {
        let dir = tempdir().unwrap();
        with_config_home(dir.path(), || {
            let path = dir.path().join("cfg.toml");
            let mut cfg = AppConfig::default();
            assert_eq!(cfg.volume, 1.0);
            cfg.volume = 0.42;
            save_to_path(&cfg, &path).unwrap();
            let loaded = super::load_settings_from(&path).unwrap();
            assert!((loaded.volume - 0.42).abs() < f32::EPSILON);
        });
    }

    #[test]
    fn audio_output_defaults_and_persists() {
        let dir = tempdir().unwrap();
        with_config_home(dir.path(), || {
            let path = dir.path().join("cfg.toml");
            let cfg = AppConfig {
                audio_output: AudioOutputConfig {
                    host: Some("asio".into()),
                    device: Some("Test Interface".into()),
                    sample_rate: Some(48_000),
                    buffer_size: Some(512),
                },
                ..AppConfig::default()
            };

            save_to_path(&cfg, &path).unwrap();
            let loaded = super::load_settings_from(&path).unwrap();
            assert_eq!(loaded.audio_output.host.as_deref(), Some("asio"));
            assert_eq!(
                loaded.audio_output.device.as_deref(),
                Some("Test Interface")
            );
            assert_eq!(loaded.audio_output.sample_rate, Some(48_000));
            assert_eq!(loaded.audio_output.buffer_size, Some(512));
        });
    }

    #[test]
    fn trash_folder_round_trips() {
        let dir = tempdir().unwrap();
        with_config_home(dir.path(), || {
            let path = dir.path().join("cfg.toml");
            let trash = PathBuf::from("trash_bin");
            let cfg = AppConfig {
                trash_folder: Some(trash.clone()),
                ..AppConfig::default()
            };
            save_to_path(&cfg, &path).unwrap();
            let loaded = super::load_settings_from(&path).unwrap();
            assert_eq!(loaded.trash_folder, Some(trash));
        });
    }

    #[test]
    fn collection_export_root_round_trips() {
        let dir = tempdir().unwrap();
        with_config_home(dir.path(), || {
            let path = dir.path().join("cfg.toml");
            let root = PathBuf::from("exports");
            let cfg = AppConfig {
                collection_export_root: Some(root.clone()),
                ..AppConfig::default()
            };
            save_to_path(&cfg, &path).unwrap();
            let loaded = super::load_settings_from(&path).unwrap();
            assert_eq!(loaded.collection_export_root, Some(root));
        });
    }

    #[test]
    fn model_settings_round_trip() {
        let dir = tempdir().unwrap();
        with_config_home(dir.path(), || {
            let path = dir.path().join("cfg.toml");
            let cfg = AppConfig {
                model: ModelSettings {
                    unknown_confidence_threshold: 0.91,
                    classifier_model_id: "test_model".to_string(),
                    use_user_overrides: false,
                },
                ..AppConfig::default()
            };
            save_to_path(&cfg, &path).unwrap();
            let loaded = super::load_settings_from(&path).unwrap();
            assert!((loaded.model.unknown_confidence_threshold - 0.91).abs() < f32::EPSILON);
            assert_eq!(loaded.model.classifier_model_id, "test_model");
            assert_eq!(loaded.model.use_user_overrides, false);
        });
    }

    #[test]
    fn training_settings_round_trip() {
        let dir = tempdir().unwrap();
        with_config_home(dir.path(), || {
            let path = dir.path().join("cfg.toml");
            let cfg = AppConfig {
                training: TrainingSettings {
                    retrain_min_confidence: 0.42,
                    retrain_pack_depth: 3,
                    use_user_labels: false,
                    model_kind: TrainingModelKind::GbdtStumpV1,
                    min_class_samples: default_training_min_class_samples(),
                    use_hybrid_features: false,
                    augmentation: TrainingAugmentation::default(),
                    training_dataset_root: Some(PathBuf::from("training")),
                },
                ..AppConfig::default()
            };
            save_to_path(&cfg, &path).unwrap();
            let loaded = super::load_settings_from(&path).unwrap();
            assert!((loaded.training.retrain_min_confidence - 0.42).abs() < f32::EPSILON);
            assert_eq!(loaded.training.retrain_pack_depth, 3);
            assert_eq!(loaded.training.use_user_labels, false);
            assert_eq!(loaded.training.model_kind, TrainingModelKind::GbdtStumpV1);
            assert_eq!(
                loaded.training.training_dataset_root,
                Some(PathBuf::from("training"))
            );
        });
    }
}
