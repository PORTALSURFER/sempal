use super::super::super::config_types::{
    AnalysisSettings, AppSettingsCore, DropTargetConfig, FeatureFlags,
    InteractionOptions, UpdateSettings,
};
use super::super::LEGACY_CONFIG_FILE_NAME;
use super::super::load::load_or_default;
use super::TestConfigEnv;
use crate::audio::{AudioInputConfig, AudioOutputConfig};
use crate::sample_sources::config::AppConfig;
use crate::sample_sources::SampleSource;

#[test]
fn migrates_from_legacy_json() {
    let env = TestConfigEnv::new();
    let legacy_path = env.ensure_app_dir().join(LEGACY_CONFIG_FILE_NAME);
    let legacy = AppConfig {
        sources: vec![SampleSource::new(std::path::PathBuf::from("old_source"))],
        core: AppSettingsCore {
            feature_flags: FeatureFlags::default(),
            analysis: AnalysisSettings::default(),
            updates: UpdateSettings::default(),
            job_message_queue_capacity: AppSettingsCore::default().job_message_queue_capacity,
            app_data_dir: None,
            trash_folder: Some(std::path::PathBuf::from("trash_here")),
            drop_targets: vec![DropTargetConfig::new(std::path::PathBuf::from(
                "legacy_drop",
            ))],
            last_selected_source: None,
            audio_output: AudioOutputConfig::default(),
            audio_input: AudioInputConfig::default(),
            volume: 0.9,
            controls: InteractionOptions::default(),
        },
    };
    let mut data = serde_json::to_value(&legacy).unwrap();
    if let Some(core) = data.get_mut("core") {
        if let Some(drop_targets) = core.get_mut("drop_targets") {
            *drop_targets = serde_json::json!(["legacy_drop"]);
        }
    }
    let data = serde_json::to_vec_pretty(&data).unwrap();
    std::fs::write(&legacy_path, data).unwrap();

    let loaded = load_or_default().unwrap();
    assert_eq!(loaded.sources.len(), 1);
    assert_eq!(
        loaded.core.trash_folder,
        Some(std::path::PathBuf::from("trash_here"))
    );

    let backup = legacy_path.with_extension("json.bak");
    assert!(backup.exists(), "expected backup file {}", backup.display());
}
