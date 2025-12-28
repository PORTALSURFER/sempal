use std::path::{Path, PathBuf};

use serde::de::Error as SerdeDeError;

use crate::app_dirs;

use super::config_types::{
    AnalysisSettings, AppConfig, AppSettings, AppSettingsCore, ConfigError, InteractionOptions,
    UpdateSettings,
};

/// Default filename used to store the app configuration.
pub const CONFIG_FILE_NAME: &str = "config.toml";
/// Legacy filename for migration support.
pub const LEGACY_CONFIG_FILE_NAME: &str = "config.json";

/// Resolve the configuration file path, ensuring the parent directory exists.
pub fn config_path() -> Result<PathBuf, ConfigError> {
    let dir = app_dirs::app_root_dir().map_err(map_app_dir_error)?;
    Ok(dir.join(CONFIG_FILE_NAME))
}

/// Load configuration from disk, returning defaults if missing.
///
/// This pulls settings from a TOML file and data from the SQLite library database.
/// If a legacy `config.json` exists, it will be migrated into the new layout.
pub fn load_or_default() -> Result<AppConfig, ConfigError> {
    let settings_path = config_path()?;
    let legacy_path = legacy_config_path()?;
    let mut settings = if settings_path.exists() {
        load_settings_from(&settings_path)?
    } else {
        migrate_legacy_config(&legacy_path, &settings_path)?
    };
    apply_app_data_dir(&settings_path, &mut settings)?;

    let library = crate::sample_sources::library::load()?;
    Ok(AppConfig {
        sources: library.sources,
        collections: library.collections,
        core: settings.core,
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
    save_settings_to_path(&AppSettings {
        core: config.core.clone(),
    }, path)?;
    crate::sample_sources::library::save(&crate::sample_sources::library::LibraryState {
        sources: config.sources.clone(),
        collections: config.collections.clone(),
    })?;
    Ok(())
}

/// Utility to convert absolute paths to strings for serialization durability.
pub fn normalize_path(path: &Path) -> PathBuf {
    PathBuf::from_iter(path.components())
}

fn legacy_config_path() -> Result<PathBuf, ConfigError> {
    let dir = app_dirs::app_root_dir().map_err(map_app_dir_error)?;
    Ok(dir.join(LEGACY_CONFIG_FILE_NAME))
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
    toml::from_str(&text)
        .map_err(|source| ConfigError::ParseToml {
            path: path.to_path_buf(),
            source,
        })
        .map(AppSettings::normalized)
}

fn apply_app_data_dir(
    settings_path: &Path,
    settings: &mut AppSettings,
) -> Result<(), ConfigError> {
    let Some(app_data_dir) = settings.core.app_data_dir.clone() else {
        return Ok(());
    };
    let override_path = app_data_dir.join(CONFIG_FILE_NAME);
    if override_path != settings_path && override_path.exists() {
        *settings = load_settings_from(&override_path)?;
    } else if override_path != settings_path && settings_path.exists() {
        save_settings_to_path(settings, &override_path)?;
    }
    settings.core.app_data_dir = Some(app_data_dir.clone());
    app_dirs::set_app_root_override(app_data_dir).map_err(map_app_dir_error)?;
    Ok(())
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
        core: AppSettingsCore {
            feature_flags: legacy.core.feature_flags,
            analysis: AnalysisSettings::default(),
            updates: UpdateSettings::default(),
            hints: super::config_types::HintSettings::default(),
            app_data_dir: None,
            trash_folder: legacy.core.trash_folder,
            collection_export_root: None,
            last_selected_source: legacy.core.last_selected_source,
            audio_output: legacy.core.audio_output,
            volume: legacy.core.volume,
            controls: InteractionOptions::default(),
        },
    }
    .normalized();
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
    use super::super::config_defaults::MAX_ANALYSIS_WORKER_COUNT;
    use super::super::config_types::{
        AnalysisSettings, AppSettingsCore, FeatureFlags, HintSettings, InteractionOptions,
        UpdateSettings,
    };
    use super::*;
    use crate::audio::AudioOutputConfig;
    use crate::sample_sources::{Collection, SampleSource};
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
                core: AppSettingsCore {
                    volume: 0.42,
                    trash_folder: Some(PathBuf::from("trash")),
                    ..AppSettingsCore::default()
                },
                ..AppConfig::default()
            };
            save_to_path(&cfg, &path).unwrap();
            let loaded = super::load_settings_from(&path).unwrap();
            assert!((loaded.core.volume - 0.42).abs() < f32::EPSILON);
            assert_eq!(
                loaded.core.trash_folder,
                Some(PathBuf::from("trash"))
            );
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
                core: AppSettingsCore {
                    feature_flags: FeatureFlags::default(),
                    analysis: AnalysisSettings::default(),
                    updates: UpdateSettings::default(),
                    hints: HintSettings::default(),
                    app_data_dir: None,
                    trash_folder: Some(PathBuf::from("trash_here")),
                    collection_export_root: None,
                    last_selected_source: None,
                    audio_output: AudioOutputConfig::default(),
                    volume: 0.9,
                    controls: InteractionOptions::default(),
                },
            };
            let data = serde_json::to_vec_pretty(&legacy).unwrap();
            std::fs::write(&legacy_path, data).unwrap();

            let loaded = load_or_default().unwrap();
            assert_eq!(loaded.sources.len(), 1);
            assert_eq!(loaded.collections.len(), 1);
            assert_eq!(loaded.core.trash_folder, Some(PathBuf::from("trash_here")));

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
            assert_eq!(cfg.core.volume, 1.0);
            cfg.core.volume = 0.42;
            save_to_path(&cfg, &path).unwrap();
            let loaded = super::load_settings_from(&path).unwrap();
            assert!((loaded.core.volume - 0.42).abs() < f32::EPSILON);
        });
    }

    #[test]
    fn audio_output_defaults_and_persists() {
        let dir = tempdir().unwrap();
        with_config_home(dir.path(), || {
            let path = dir.path().join("cfg.toml");
            let cfg = AppConfig {
                core: AppSettingsCore {
                    audio_output: AudioOutputConfig {
                        host: Some("asio".into()),
                        device: Some("Test Interface".into()),
                        sample_rate: Some(48_000),
                        buffer_size: Some(512),
                    },
                    ..AppSettingsCore::default()
                },
                ..AppConfig::default()
            };

            save_to_path(&cfg, &path).unwrap();
            let loaded = super::load_settings_from(&path).unwrap();
            assert_eq!(loaded.core.audio_output.host.as_deref(), Some("asio"));
            assert_eq!(
                loaded.core.audio_output.device.as_deref(),
                Some("Test Interface")
            );
            assert_eq!(loaded.core.audio_output.sample_rate, Some(48_000));
            assert_eq!(loaded.core.audio_output.buffer_size, Some(512));
        });
    }

    #[test]
    fn trash_folder_round_trips() {
        let dir = tempdir().unwrap();
        with_config_home(dir.path(), || {
            let path = dir.path().join("cfg.toml");
            let trash = PathBuf::from("trash_bin");
            let cfg = AppConfig {
                core: AppSettingsCore {
                    trash_folder: Some(trash.clone()),
                    ..AppSettingsCore::default()
                },
                ..AppConfig::default()
            };
            save_to_path(&cfg, &path).unwrap();
            let loaded = super::load_settings_from(&path).unwrap();
            assert_eq!(loaded.core.trash_folder, Some(trash));
        });
    }

    #[test]
    fn collection_export_root_round_trips() {
        let dir = tempdir().unwrap();
        with_config_home(dir.path(), || {
            let path = dir.path().join("cfg.toml");
            let root = PathBuf::from("exports");
            let cfg = AppConfig {
                core: AppSettingsCore {
                    collection_export_root: Some(root.clone()),
                    ..AppSettingsCore::default()
                },
                ..AppConfig::default()
            };
            save_to_path(&cfg, &path).unwrap();
            let loaded = super::load_settings_from(&path).unwrap();
            assert_eq!(loaded.core.collection_export_root, Some(root));
        });
    }

    #[test]
    fn clamps_volume_and_worker_count_on_load() {
        let dir = tempdir().unwrap();
        with_config_home(dir.path(), || {
            let path = dir.path().join("cfg.toml");
            let data = r#"
volume = 2.5

[analysis]
analysis_worker_count = 999
"#;
            std::fs::write(&path, data).unwrap();
            let loaded = super::load_settings_from(&path).unwrap();
            assert!((loaded.core.volume - 1.0).abs() < f32::EPSILON);
            assert_eq!(
                loaded.core.analysis.analysis_worker_count,
                MAX_ANALYSIS_WORKER_COUNT
            );
        });
    }
}
