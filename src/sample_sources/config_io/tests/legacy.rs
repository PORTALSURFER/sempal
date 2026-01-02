use super::TestConfigEnv;
use crate::audio::{AudioInputConfig, AudioOutputConfig};
use crate::sample_sources::config::AppConfig;
use crate::sample_sources::config_io::LEGACY_CONFIG_FILE_NAME;
use crate::sample_sources::config_io::load::load_or_default;
use crate::sample_sources::config_types::{
    AnalysisSettings, AppSettingsCore, FeatureFlags, HintSettings, InteractionOptions,
    UpdateSettings,
};
use crate::sample_sources::{Collection, SampleSource};

#[test]
fn migrates_from_legacy_json() {
    let env = TestConfigEnv::new();
    let legacy_path = env
        .ensure_app_dir()
        .join(LEGACY_CONFIG_FILE_NAME);
    let legacy = AppConfig {
        sources: vec![SampleSource::new(std::path::PathBuf::from("old_source"))],
        collections: vec![Collection::new("Old Collection")],
        core: AppSettingsCore {
            feature_flags: FeatureFlags::default(),
            analysis: AnalysisSettings::default(),
            updates: UpdateSettings::default(),
            hints: HintSettings::default(),
            app_data_dir: None,
            trash_folder: Some(std::path::PathBuf::from("trash_here")),
            collection_export_root: None,
            last_selected_source: None,
            audio_output: AudioOutputConfig::default(),
            audio_input: AudioInputConfig::default(),
            volume: 0.9,
            controls: InteractionOptions::default(),
        },
    };
    let data = serde_json::to_vec_pretty(&legacy).unwrap();
    std::fs::write(&legacy_path, data).unwrap();

    let loaded = load_or_default().unwrap();
    assert_eq!(loaded.sources.len(), 1);
    assert_eq!(loaded.collections.len(), 1);
    assert_eq!(loaded.core.trash_folder, Some(std::path::PathBuf::from("trash_here")));

    let backup = legacy_path.with_extension("json.bak");
    assert!(backup.exists(), "expected backup file {}", backup.display());
}
