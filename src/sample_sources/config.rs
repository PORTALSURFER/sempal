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
}
