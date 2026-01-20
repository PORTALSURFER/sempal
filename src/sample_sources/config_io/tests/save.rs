use super::super::config_types::{
    AnalysisSettings, AppSettingsCore, DropTargetColor, DropTargetConfig, FeatureFlags,
    InteractionOptions, TooltipMode, UpdateChannel, UpdateSettings,
    WgpuPowerPreference,
};
use super::super::load::load_settings_from;
use super::super::save::save_to_path;
use super::TestConfigEnv;
#[cfg(unix)]
use super::super::save::save_settings_to_path;
use crate::audio::{AudioInputConfig, AudioOutputConfig};
use crate::sample_sources::config::AppConfig;
#[cfg(unix)]
use super::super::config_types::AppSettings;
use crate::sample_sources::library::LibraryState;
use crate::sample_sources::{Collection, SampleSource, SourceId};
use crate::waveform::WaveformChannelView;

#[test]
fn saves_settings_to_toml() {
    let env = TestConfigEnv::new();
    let path = env.path("cfg.toml");
    let cfg = AppConfig {
        core: AppSettingsCore {
            volume: 0.42,
            trash_folder: Some(std::path::PathBuf::from("trash")),
            ..AppSettingsCore::default()
        },
        ..AppConfig::default()
    };
    save_to_path(&cfg, &path).unwrap();
    let loaded = load_settings_from(&path).unwrap();
    assert!((loaded.core.volume - 0.42).abs() < f32::EPSILON);
    assert_eq!(
        loaded.core.trash_folder,
        Some(std::path::PathBuf::from("trash"))
    );
}

#[test]
fn volume_defaults_and_persists() {
    let env = TestConfigEnv::new();
    let path = env.path("cfg.toml");
    let mut cfg = AppConfig::default();
    assert_eq!(cfg.core.volume, 1.0);
    cfg.core.volume = 0.42;
    save_to_path(&cfg, &path).unwrap();
    let loaded = load_settings_from(&path).unwrap();
    assert!((loaded.core.volume - 0.42).abs() < f32::EPSILON);
}

#[test]
fn audio_output_defaults_and_persists() {
    let env = TestConfigEnv::new();
    let path = env.path("cfg.toml");
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
    let loaded = load_settings_from(&path).unwrap();
    assert_eq!(loaded.core.audio_output.host.as_deref(), Some("asio"));
    assert_eq!(
        loaded.core.audio_output.device.as_deref(),
        Some("Test Interface")
    );
    assert_eq!(loaded.core.audio_output.sample_rate, Some(48_000));
    assert_eq!(loaded.core.audio_output.buffer_size, Some(512));
}

#[test]
fn audio_input_defaults_and_persists() {
    let env = TestConfigEnv::new();
    let path = env.path("cfg.toml");
    let cfg = AppConfig {
        core: AppSettingsCore {
            audio_input: AudioInputConfig {
                host: Some("asio".into()),
                device: Some("Test Mic".into()),
                sample_rate: Some(44_100),
                buffer_size: Some(256),
                channels: vec![1, 2],
            },
            ..AppSettingsCore::default()
        },
        ..AppConfig::default()
    };

    save_to_path(&cfg, &path).unwrap();
    let loaded = load_settings_from(&path).unwrap();
    assert_eq!(loaded.core.audio_input.host.as_deref(), Some("asio"));
    assert_eq!(loaded.core.audio_input.device.as_deref(), Some("Test Mic"));
    assert_eq!(loaded.core.audio_input.sample_rate, Some(44_100));
    assert_eq!(loaded.core.audio_input.buffer_size, Some(256));
    assert_eq!(loaded.core.audio_input.channels, vec![1, 2]);
}

#[test]
fn trash_folder_round_trips() {
    let env = TestConfigEnv::new();
    let path = env.path("cfg.toml");
    let trash = std::path::PathBuf::from("trash_bin");
    let cfg = AppConfig {
        core: AppSettingsCore {
            trash_folder: Some(trash.clone()),
            ..AppSettingsCore::default()
        },
        ..AppConfig::default()
    };
    save_to_path(&cfg, &path).unwrap();
    let loaded = load_settings_from(&path).unwrap();
    assert_eq!(loaded.core.trash_folder, Some(trash));
}

#[test]
fn collection_export_root_round_trips() {
    let env = TestConfigEnv::new();
    let path = env.path("cfg.toml");
    let root = std::path::PathBuf::from("exports");
    let cfg = AppConfig {
        core: AppSettingsCore {
            collection_export_root: Some(root.clone()),
            ..AppSettingsCore::default()
        },
        ..AppConfig::default()
    };
    save_to_path(&cfg, &path).unwrap();
    let loaded = load_settings_from(&path).unwrap();
    assert_eq!(loaded.core.collection_export_root, Some(root));
}

#[test]
fn settings_round_trip_preserves_fields() {
    let env = TestConfigEnv::new();
    let path = env.path("cfg.toml");
    let source_id = SourceId::from_string("source_id::test");
    let cfg = AppConfig {
        sources: vec![SampleSource::new_with_id(
            source_id.clone(),
            std::path::PathBuf::from("samples"),
        )],
        collections: vec![Collection::new("Test Collection")],
        core: AppSettingsCore {
            feature_flags: FeatureFlags {
                collections_enabled: false,
                autoplay_selection: false,
            },
            analysis: AnalysisSettings {
                max_analysis_duration_seconds: 12.5,
                limit_similarity_prep_duration: false,
                analysis_worker_count: 2,
                fast_similarity_prep: true,
                fast_similarity_prep_sample_rate: 8_000,
                wgpu_power_preference: WgpuPowerPreference::High,
                wgpu_adapter_name: Some("adapter".into()),
            },
            updates: UpdateSettings {
                channel: UpdateChannel::Nightly,
                check_on_startup: false,
                last_seen_nightly_published_at: Some("2024-01-01".into()),
            },
            app_data_dir: Some(std::path::PathBuf::from("data_root")),
            trash_folder: Some(std::path::PathBuf::from("trash_bin")),
            collection_export_root: Some(std::path::PathBuf::from("exports_root")),
            drop_targets: vec![
                DropTargetConfig {
                    path: std::path::PathBuf::from("drops/a"),
                    color: Some(DropTargetColor::Mint),
                },
                DropTargetConfig::new(std::path::PathBuf::from("drops/b")),
            ],
            last_selected_source: Some(source_id.clone()),
            audio_output: AudioOutputConfig {
                host: Some("coreaudio".into()),
                device: Some("Test Interface".into()),
                sample_rate: Some(96_000),
                buffer_size: Some(256),
            },
            audio_input: AudioInputConfig {
                host: Some("asio".into()),
                device: Some("Test Mic".into()),
                sample_rate: Some(44_100),
                buffer_size: Some(256),
                channels: vec![1],
            },
            volume: 0.75,
            controls: InteractionOptions {
                invert_waveform_scroll: false,
                waveform_scroll_speed: 2.5,
                wheel_zoom_factor: 1.5,
                keyboard_zoom_factor: 1.2,
                anti_clip_fade_enabled: false,
                anti_clip_fade_ms: 12.0,
                destructive_yolo_mode: true,
                waveform_channel_view: WaveformChannelView::SplitStereo,
                bpm_snap_enabled: true,
                bpm_lock_enabled: true,
                bpm_stretch_enabled: true,
                bpm_value: 123.0,
                transient_snap_enabled: true,
                transient_markers_enabled: false,
                input_monitoring_enabled: false,
                normalized_audition_enabled: true,
                advance_after_rating: true,
                tooltip_mode: TooltipMode::Regular,
            },
        },
    };

    save_to_path(&cfg, &path).unwrap();
    let loaded_settings = load_settings_from(&path).unwrap();
    let library_state = LibraryState {
        sources: cfg.sources.clone(),
        collections: cfg.collections.clone(),
    };
    let round_trip = AppConfig::from((loaded_settings, library_state));

    assert_eq!(
        round_trip.core.feature_flags.collections_enabled,
        cfg.core.feature_flags.collections_enabled
    );
    assert_eq!(
        round_trip.core.feature_flags.autoplay_selection,
        cfg.core.feature_flags.autoplay_selection
    );
    assert_eq!(
        round_trip.core.analysis.max_analysis_duration_seconds,
        cfg.core.analysis.max_analysis_duration_seconds
    );
    assert_eq!(
        round_trip.core.analysis.limit_similarity_prep_duration,
        cfg.core.analysis.limit_similarity_prep_duration
    );
    assert_eq!(
        round_trip.core.analysis.analysis_worker_count,
        cfg.core.analysis.analysis_worker_count
    );
    assert_eq!(
        round_trip.core.analysis.fast_similarity_prep,
        cfg.core.analysis.fast_similarity_prep
    );
    assert_eq!(
        round_trip.core.analysis.fast_similarity_prep_sample_rate,
        cfg.core.analysis.fast_similarity_prep_sample_rate
    );
    assert_eq!(
        round_trip.core.analysis.wgpu_power_preference,
        cfg.core.analysis.wgpu_power_preference
    );
    assert_eq!(
        round_trip.core.analysis.wgpu_adapter_name,
        cfg.core.analysis.wgpu_adapter_name
    );
    assert_eq!(round_trip.core.updates.channel, cfg.core.updates.channel);
    assert_eq!(
        round_trip.core.updates.check_on_startup,
        cfg.core.updates.check_on_startup
    );
    assert_eq!(
        round_trip.core.updates.last_seen_nightly_published_at,
        cfg.core.updates.last_seen_nightly_published_at
    );
    assert_eq!(round_trip.core.app_data_dir, cfg.core.app_data_dir);
    assert_eq!(round_trip.core.trash_folder, cfg.core.trash_folder);
    assert_eq!(
        round_trip.core.collection_export_root,
        cfg.core.collection_export_root
    );
    assert_eq!(
        round_trip.core.last_selected_source,
        cfg.core.last_selected_source
    );
    assert_eq!(round_trip.core.audio_output, cfg.core.audio_output);
    assert!((round_trip.core.volume - cfg.core.volume).abs() < f32::EPSILON);
    assert_eq!(
        round_trip.core.controls.invert_waveform_scroll,
        cfg.core.controls.invert_waveform_scroll
    );
    assert_eq!(
        round_trip.core.controls.waveform_scroll_speed,
        cfg.core.controls.waveform_scroll_speed
    );
    assert_eq!(
        round_trip.core.controls.wheel_zoom_factor,
        cfg.core.controls.wheel_zoom_factor
    );
    assert_eq!(
        round_trip.core.controls.keyboard_zoom_factor,
        cfg.core.controls.keyboard_zoom_factor
    );
    assert_eq!(
        round_trip.core.controls.anti_clip_fade_enabled,
        cfg.core.controls.anti_clip_fade_enabled
    );
    assert_eq!(
        round_trip.core.controls.anti_clip_fade_ms,
        cfg.core.controls.anti_clip_fade_ms
    );
    assert_eq!(
        round_trip.core.controls.destructive_yolo_mode,
        cfg.core.controls.destructive_yolo_mode
    );
    assert_eq!(
        round_trip.core.controls.waveform_channel_view,
        cfg.core.controls.waveform_channel_view
    );
    assert_eq!(
        round_trip.core.controls.bpm_snap_enabled,
        cfg.core.controls.bpm_snap_enabled
    );
    assert_eq!(
        round_trip.core.controls.bpm_lock_enabled,
        cfg.core.controls.bpm_lock_enabled
    );
    assert_eq!(
        round_trip.core.controls.bpm_stretch_enabled,
        cfg.core.controls.bpm_stretch_enabled
    );
    assert_eq!(
        round_trip.core.controls.bpm_value,
        cfg.core.controls.bpm_value
    );
    assert_eq!(
        round_trip.core.controls.transient_snap_enabled,
        cfg.core.controls.transient_snap_enabled
    );
    assert_eq!(
        round_trip.core.controls.transient_markers_enabled,
        cfg.core.controls.transient_markers_enabled
    );
    assert_eq!(
        round_trip.core.controls.input_monitoring_enabled,
        cfg.core.controls.input_monitoring_enabled
    );
    assert_eq!(
        round_trip.core.controls.normalized_audition_enabled,
        cfg.core.controls.normalized_audition_enabled
    );
    assert_eq!(
        round_trip.core.controls.advance_after_rating,
        cfg.core.controls.advance_after_rating
    );
    assert_eq!(
        round_trip.core.controls.tooltip_mode,
        cfg.core.controls.tooltip_mode
    );
}

#[test]
#[cfg(unix)]
fn settings_atomic_write_preserves_existing_on_failure() {
    use std::os::unix::fs::PermissionsExt;

    let env = TestConfigEnv::new();
    let path = env.path("cfg.toml");
    env.write(&path, "sentinel = true\n");

    let dir = path.parent().unwrap();
    let mut permissions = std::fs::metadata(dir).unwrap().permissions();
    permissions.set_mode(0o500);
    std::fs::set_permissions(dir, permissions).unwrap();

    let mut settings = AppSettings::default();
    settings.core.volume = 0.33;
    let result = save_settings_to_path(&settings, &path);
    assert!(result.is_err());

    let mut restore = std::fs::metadata(dir).unwrap().permissions();
    restore.set_mode(0o700);
    std::fs::set_permissions(dir, restore).unwrap();

    let contents = std::fs::read_to_string(&path).unwrap();
    assert_eq!(contents, "sentinel = true\n");
}
