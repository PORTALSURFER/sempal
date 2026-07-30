use super::*;

#[test]
fn default_gui_restores_cached_sample_indicators_from_source_scan_cache() {
    let config_base = tempfile::tempdir().expect("config base");
    let _base_guard = wavecrate::app_dirs::ConfigBaseGuard::set(config_base.path().to_path_buf());
    let source_root = tempfile::tempdir().expect("source root");
    let sample_path = source_root.path().join("restored-cache.wav");
    write_test_wav_i16(&sample_path, &[0, 1024, -2048, 4096, -1024, 512]);
    let sample_id = sample_path.display().to_string();
    let source = wavecrate::sample_sources::SampleSource::new_with_id(
        wavecrate::sample_sources::SourceId::from_string("source_id::gui-cache-startup"),
        source_root.path().to_path_buf(),
    );
    wavecrate::sample_sources::config::save(&crate::native_app::test_support::config::AppConfig {
        sources: vec![source.clone()],
        core: crate::native_app::test_support::config::AppSettingsCore::default(),
    })
    .expect("seed config");
    crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[source])
        .save_source_scan_cache()
        .expect("persist source scan cache");

    let _waveform = crate::native_app::test_support::state::WaveformState::load_path(sample_path)
        .expect("persist waveform cache");

    let state = NativeAppState::load_default().expect("default state loads persisted cache");

    assert!(state.library.folder_browser.selected_source_loaded());
    assert!(
        !state.ui.startup.source_scan_pending,
        "cached source trees must not queue a full startup scan"
    );
    assert!(
        state.ui.startup.folder_verify_pending,
        "cached source trees should queue a bounded folder-tree refresh"
    );
    assert!(
        !state
            .waveform
            .cache
            .cached_sample_paths
            .contains(&sample_id),
        "startup must not probe waveform cache metadata on the UI thread"
    );
}

#[test]
fn cached_startup_queues_folder_tree_refresh_without_foreground_scan() {
    let config_base = tempfile::tempdir().expect("config base");
    let _base_guard = wavecrate::app_dirs::ConfigBaseGuard::set(config_base.path().to_path_buf());
    let source_root = tempfile::tempdir().expect("source root");
    fs::write(source_root.path().join("kick.wav"), [0_u8; 8]).expect("write sample");
    let source = wavecrate::sample_sources::SampleSource::new_with_id(
        wavecrate::sample_sources::SourceId::from_string("source_id::gui-cache-no-startup-scan"),
        source_root.path().to_path_buf(),
    );
    wavecrate::sample_sources::config::save(&crate::native_app::test_support::config::AppConfig {
        sources: vec![source.clone()],
        core: crate::native_app::test_support::config::AppSettingsCore::default(),
    })
    .expect("seed config");
    crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[source])
        .save_source_scan_cache()
        .expect("persist source scan cache");
    let mut state = NativeAppState::load_default().expect("default state loads persisted cache");
    let mut context = ui::UiUpdateContext::default();

    state.maybe_startup_source_scan(&mut context);

    assert!(
        state.library.folder_progress().is_none(),
        "cached startup must not queue a foreground source scan"
    );
    assert!(
        !state.ui.startup.source_scan_pending,
        "cached startup should not leave a full scan pending"
    );
    assert!(
        !state.ui.startup.folder_verify_pending,
        "folder-tree refresh should be consumed as a one-shot startup task"
    );
    assert!(
        state.background.folder_tree_refresh_task.active().is_some(),
        "cached startup should refresh only the folder tree in the background"
    );
    assert!(
        state.background.folder_verify_task.active().is_none(),
        "cached startup should not queue the old visible-folder verification task"
    );
}

#[test]
fn cached_startup_folder_refresh_hydrates_rating_and_persists_cache_for_restart() {
    let config_base = tempfile::tempdir().expect("config base");
    let _base_guard = wavecrate::app_dirs::ConfigBaseGuard::set(config_base.path().to_path_buf());
    let source_root = tempfile::tempdir().expect("source root");
    let kick = source_root.path().join("kick.wav");
    fs::write(&kick, [0_u8; 8]).expect("write sample");
    let source = wavecrate::sample_sources::SampleSource::new_with_id(
        wavecrate::sample_sources::SourceId::from_string("source_id::rating-cache-restart"),
        source_root.path().to_path_buf(),
    );
    wavecrate::sample_sources::config::save(&crate::native_app::test_support::config::AppConfig {
        sources: vec![source.clone()],
        core: crate::native_app::test_support::config::AppSettingsCore::default(),
    })
    .expect("seed config");
    crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[source])
        .save_source_scan_cache()
        .expect("persist neutral cache");

    let database =
        wavecrate::sample_sources::SourceDatabase::open_for_source_write(source_root.path())
            .expect("open source db");
    database
        .upsert_file(std::path::Path::new("kick.wav"), 8, 0)
        .expect("index sample");
    database
        .set_tag(
            std::path::Path::new("kick.wav"),
            wavecrate::sample_sources::Rating::KEEP_3,
        )
        .expect("persist rating");
    database
        .set_locked(std::path::Path::new("kick.wav"), true)
        .expect("persist lock");

    let mut state = NativeAppState::load_default().expect("load cached state");
    let mut context = ui::UiUpdateContext::default();
    assert!(state.queue_selected_source_folder_tree_refresh(
        &mut context,
        "test.folder_tree_refresh",
        "test-folder-tree-refresh",
    ));
    let ticket = state
        .background
        .folder_tree_refresh_task
        .active()
        .expect("refresh task ticket");
    let request = state
        .library
        .folder_browser
        .selected_source_folder_tree_refresh_request()
        .expect("refresh request");
    let result =
        crate::native_app::sample_library::folder_browser::scan::refresh_folder_tree_only(request);
    state.finish_folder_tree_refresh(
        ui::TaskCompletion {
            ticket,
            output: result,
        },
        &mut context,
    );
    assert_eq!(
        state
            .library
            .folder_browser
            .selected_audio_files()
            .into_iter()
            .find(|file| file.id == kick.display().to_string())
            .expect("hydrated kick")
            .rating,
        wavecrate::sample_sources::Rating::KEEP_3
    );

    let reloaded = NativeAppState::load_default().expect("reload persisted cache");
    let cached = reloaded
        .library
        .folder_browser
        .selected_audio_files()
        .into_iter()
        .find(|file| file.id == kick.display().to_string())
        .expect("cached kick after restart");
    assert_eq!(cached.rating, wavecrate::sample_sources::Rating::KEEP_3);
    assert!(cached.rating_locked);
}

#[test]
fn selecting_cached_second_source_hydrates_rating_and_persists_cache_for_restart() {
    let config_base = tempfile::tempdir().expect("config base");
    let _base_guard = wavecrate::app_dirs::ConfigBaseGuard::set(config_base.path().to_path_buf());
    let first_root = tempfile::tempdir().expect("first source root");
    let second_root = tempfile::tempdir().expect("second source root");
    let first_file = first_root.path().join("first.wav");
    let second_file = second_root.path().join("second.wav");
    fs::write(&first_file, [0_u8; 8]).expect("write first sample");
    fs::write(&second_file, [0_u8; 8]).expect("write second sample");
    let sources = vec![
        wavecrate::sample_sources::SampleSource::new_with_id(
            wavecrate::sample_sources::SourceId::from_string("source-a"),
            first_root.path().to_path_buf(),
        ),
        wavecrate::sample_sources::SampleSource::new_with_id(
            wavecrate::sample_sources::SourceId::from_string("source-b"),
            second_root.path().to_path_buf(),
        ),
    ];
    wavecrate::sample_sources::config::save(&crate::native_app::test_support::config::AppConfig {
        sources: sources.clone(),
        core: crate::native_app::test_support::config::AppSettingsCore::default(),
    })
    .expect("seed config");
    let mut seeded_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&sources);
    let second_scan = seeded_browser
        .begin_source_scan(String::from("source-b"), 1)
        .expect("seed second source cache");
    assert!(seeded_browser.apply_scan_finished(
        crate::native_app::sample_library::folder_browser::scan::scan_source_with_progress(
            second_scan,
            |_| {},
            |_| {},
        )
    ));
    seeded_browser
        .save_source_scan_cache()
        .expect("persist neutral source scan cache");

    let database =
        wavecrate::sample_sources::SourceDatabase::open_for_source_write(second_root.path())
            .expect("open second source db");
    database
        .upsert_file(std::path::Path::new("second.wav"), 8, 0)
        .expect("index second sample");
    database
        .set_tag(
            std::path::Path::new("second.wav"),
            wavecrate::sample_sources::Rating::KEEP_3,
        )
        .expect("persist second rating");
    database
        .set_locked(std::path::Path::new("second.wav"), true)
        .expect("persist second lock");

    let mut state = NativeAppState::load_default().expect("load cached state");
    assert_eq!(
        state.library.folder_browser.selected_source_id(),
        "source-a"
    );
    let mut context = ui::UiUpdateContext::default();
    state.select_source(String::from("source-b"), &mut context);

    assert_eq!(
        state.library.folder_browser.selected_source_id(),
        "source-b"
    );
    assert!(state.library.folder_browser.selected_source_loaded());
    assert!(state.library.folder_progress().is_none());
    let ticket = state
        .background
        .folder_tree_refresh_task
        .active()
        .expect("cached source selection should queue a tree refresh");
    let request = state
        .library
        .folder_browser
        .selected_source_folder_tree_refresh_request()
        .expect("selected source refresh request");
    let result =
        crate::native_app::sample_library::folder_browser::scan::refresh_folder_tree_only(request);
    state.finish_folder_tree_refresh(
        ui::TaskCompletion {
            ticket,
            output: result,
        },
        &mut context,
    );

    let hydrated = state
        .library
        .folder_browser
        .selected_audio_files()
        .into_iter()
        .find(|file| file.id == second_file.display().to_string())
        .expect("hydrated second sample");
    assert_eq!(hydrated.rating, wavecrate::sample_sources::Rating::KEEP_3);
    assert!(hydrated.rating_locked);

    let mut reloaded = NativeAppState::load_default().expect("reload persisted cache");
    reloaded.select_source(
        String::from("source-b"),
        &mut ui::UiUpdateContext::default(),
    );
    assert_eq!(
        reloaded.library.folder_browser.selected_source_id(),
        "source-b"
    );
    let cached = reloaded
        .library
        .folder_browser
        .selected_audio_files()
        .into_iter()
        .find(|file| file.id == second_file.display().to_string())
        .expect("cached second sample after restart");
    assert_eq!(cached.rating, wavecrate::sample_sources::Rating::KEEP_3);
    assert!(cached.rating_locked);
}

#[test]
fn stale_cached_source_refresh_cannot_apply_after_rapid_source_switch_back() {
    let config_base = tempfile::tempdir().expect("config base");
    let _base_guard = wavecrate::app_dirs::ConfigBaseGuard::set(config_base.path().to_path_buf());
    let first_root = tempfile::tempdir().expect("first source root");
    let second_root = tempfile::tempdir().expect("second source root");
    let first_file = first_root.path().join("first.wav");
    let second_file = second_root.path().join("second.wav");
    fs::write(&first_file, [0_u8; 8]).expect("write first sample");
    fs::write(&second_file, [0_u8; 8]).expect("write second sample");
    let sources = vec![
        wavecrate::sample_sources::SampleSource::new_with_id(
            wavecrate::sample_sources::SourceId::from_string("source-a-rapid"),
            first_root.path().to_path_buf(),
        ),
        wavecrate::sample_sources::SampleSource::new_with_id(
            wavecrate::sample_sources::SourceId::from_string("source-b-rapid"),
            second_root.path().to_path_buf(),
        ),
    ];
    wavecrate::sample_sources::config::save(&crate::native_app::test_support::config::AppConfig {
        sources: sources.clone(),
        core: crate::native_app::test_support::config::AppSettingsCore::default(),
    })
    .expect("seed config");
    let mut seeded_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&sources);
    let second_scan = seeded_browser
        .begin_source_scan(String::from("source-b-rapid"), 1)
        .expect("seed second source cache");
    assert!(seeded_browser.apply_scan_finished(
        crate::native_app::sample_library::folder_browser::scan::scan_source_with_progress(
            second_scan,
            |_| {},
            |_| {},
        )
    ));
    seeded_browser
        .save_source_scan_cache()
        .expect("persist neutral source scan cache");

    let mut state = NativeAppState::load_default().expect("load cached state");
    let mut context = ui::UiUpdateContext::default();
    state.select_source(String::from("source-b-rapid"), &mut context);
    let b_ticket = state
        .background
        .folder_tree_refresh_task
        .active()
        .expect("source B refresh ticket");
    let b_request = state
        .library
        .folder_browser
        .selected_source_folder_tree_refresh_request()
        .expect("source B refresh request");
    let b_result =
        crate::native_app::sample_library::folder_browser::scan::refresh_folder_tree_only(
            b_request,
        );

    state.select_source(String::from("source-a-rapid"), &mut context);
    let a_ticket = state
        .background
        .folder_tree_refresh_task
        .active()
        .expect("source A refresh ticket");
    assert_ne!(a_ticket, b_ticket, "latest refresh should replace source B");
    let a_request = state
        .library
        .folder_browser
        .selected_source_folder_tree_refresh_request()
        .expect("source A refresh request");
    let a_result =
        crate::native_app::sample_library::folder_browser::scan::refresh_folder_tree_only(
            a_request,
        );

    state.finish_folder_tree_refresh(
        ui::TaskCompletion {
            ticket: b_ticket,
            output: b_result,
        },
        &mut context,
    );
    assert_eq!(
        state.library.folder_browser.selected_source_id(),
        "source-a-rapid"
    );
    let first_visible = state
        .library
        .folder_browser
        .selected_audio_files()
        .into_iter()
        .find(|file| file.id == first_file.display().to_string())
        .expect("source A remains visible after stale source B completion");
    assert_eq!(
        first_visible.rating,
        wavecrate::sample_sources::Rating::NEUTRAL
    );
    assert!(!first_visible.rating_locked);

    state.finish_folder_tree_refresh(
        ui::TaskCompletion {
            ticket: a_ticket,
            output: a_result,
        },
        &mut context,
    );
    assert!(
        state
            .library
            .folder_browser
            .selected_audio_files()
            .into_iter()
            .any(|file| file.id == first_file.display().to_string())
    );
}

#[test]
fn stale_cached_refresh_does_not_prepare_cached_source_after_switch_to_uncached_source() {
    let config_base = tempfile::tempdir().expect("config base");
    let _base_guard = wavecrate::app_dirs::ConfigBaseGuard::set(config_base.path().to_path_buf());
    let first_root = tempfile::tempdir().expect("first source root");
    let cached_root = tempfile::tempdir().expect("cached source root");
    let uncached_root = tempfile::tempdir().expect("uncached source root");
    fs::write(first_root.path().join("first.wav"), [0_u8; 8]).expect("write first sample");
    fs::write(cached_root.path().join("cached.wav"), [0_u8; 8]).expect("write cached sample");
    fs::write(uncached_root.path().join("uncached.wav"), [0_u8; 8]).expect("write uncached sample");
    let sources = vec![
        wavecrate::sample_sources::SampleSource::new_with_id(
            wavecrate::sample_sources::SourceId::from_string("source-a-stale"),
            first_root.path().to_path_buf(),
        ),
        wavecrate::sample_sources::SampleSource::new_with_id(
            wavecrate::sample_sources::SourceId::from_string("source-b-stale"),
            cached_root.path().to_path_buf(),
        ),
        wavecrate::sample_sources::SampleSource::new_with_id(
            wavecrate::sample_sources::SourceId::from_string("source-c-stale"),
            uncached_root.path().to_path_buf(),
        ),
    ];
    wavecrate::sample_sources::config::save(&crate::native_app::test_support::config::AppConfig {
        sources: sources.clone(),
        core: crate::native_app::test_support::config::AppSettingsCore::default(),
    })
    .expect("seed config");
    let mut seeded_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&sources);
    let cached_scan = seeded_browser
        .begin_source_scan(String::from("source-b-stale"), 1)
        .expect("seed cached source");
    assert!(seeded_browser.apply_scan_finished(
        crate::native_app::sample_library::folder_browser::scan::scan_source_with_progress(
            cached_scan,
            |_| {},
            |_| {},
        )
    ));
    seeded_browser
        .save_source_scan_cache()
        .expect("persist cached source");

    let mut state = NativeAppState::load_default().expect("load cached sources");
    let mut context = ui::UiUpdateContext::default();
    state.select_source(String::from("source-b-stale"), &mut context);
    let b_ticket = state
        .background
        .folder_tree_refresh_task
        .active()
        .expect("cached source refresh ticket");
    let b_request = state
        .library
        .folder_browser
        .selected_source_folder_tree_refresh_request()
        .expect("cached source refresh request");
    let b_result =
        crate::native_app::sample_library::folder_browser::scan::refresh_folder_tree_only(
            b_request,
        );
    state
        .metadata
        .persisted_tag_sources_pending
        .remove("source-b-stale");

    state.apply_folder_browser_message(
        crate::native_app::sample_library::folder_browser::commands::FolderBrowserMessage::SelectSource(
            String::from("source-c-stale"),
        ),
        &mut context,
    );
    assert_eq!(
        state.library.folder_browser.selected_source_id(),
        "source-c-stale"
    );
    assert_eq!(
        state
            .background
            .source_processing
            .selected_source_priority_for_tests()
            .as_deref(),
        Some("source-c-stale")
    );
    assert!(
        state
            .library
            .folder_progress()
            .is_some_and(|progress| progress.source_id == "source-c-stale")
    );
    assert!(
        state
            .metadata
            .persisted_tag_sources_pending
            .contains("source-c-stale")
    );
    assert!(
        !state
            .metadata
            .persisted_tag_sources_pending
            .contains("source-b-stale")
    );

    state.finish_folder_tree_refresh(
        ui::TaskCompletion {
            ticket: b_ticket,
            output: b_result,
        },
        &mut context,
    );
    assert_eq!(
        state
            .background
            .source_processing
            .selected_source_priority_for_tests()
            .as_deref(),
        Some("source-c-stale")
    );
    assert!(
        !state
            .metadata
            .persisted_tag_sources_pending
            .contains("source-b-stale")
    );
    assert!(
        state
            .metadata
            .persisted_tag_sources_pending
            .contains("source-c-stale")
    );
    assert!(
        state
            .library
            .folder_progress()
            .is_some_and(|progress| progress.source_id == "source-c-stale")
    );
}

#[test]
fn moved_files_do_not_reappear_from_source_scan_cache_after_restart() {
    let config_base = tempfile::tempdir().expect("config base");
    let _base_guard = wavecrate::app_dirs::ConfigBaseGuard::set(config_base.path().to_path_buf());
    let source_root = tempfile::tempdir().expect("source root");
    let drums = source_root.path().join("drums");
    let loops = source_root.path().join("loops");
    fs::create_dir_all(&drums).expect("create drums folder");
    fs::create_dir_all(&loops).expect("create loops folder");
    let kick = drums.join("kick.wav");
    fs::write(&kick, [0_u8; 8]).expect("write sample");
    let source = wavecrate::sample_sources::SampleSource::new_with_id(
        wavecrate::sample_sources::SourceId::from_string("source_id::move-cache-restart"),
        source_root.path().to_path_buf(),
    );
    wavecrate::sample_sources::config::save(&crate::native_app::test_support::config::AppConfig {
        sources: vec![source.clone()],
        core: crate::native_app::test_support::config::AppSettingsCore::default(),
    })
    .expect("seed config");
    crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[source])
        .save_source_scan_cache()
        .expect("persist initial source scan cache");

    let mut state = NativeAppState::load_default().expect("default state loads persisted cache");
    state.library.folder_browser.apply_message(
        crate::native_app::test_support::state::FolderBrowserMessage::ActivateFolder(
            drums.display().to_string(),
            Default::default(),
        ),
    );
    state
        .library
        .folder_browser
        .select_file(kick.display().to_string());
    state
        .library
        .folder_browser
        .begin_file_drag(kick.display().to_string(), Point::new(4.0, 8.0));
    let request = match state
        .library
        .folder_browser
        .drop_drag_on_folder(&loops.display().to_string())
        .expect("drop should be accepted")
    {
        crate::native_app::sample_library::folder_browser::commands::FolderMoveDropInput::Request(
            request,
        ) => request,
        other => panic!("expected move request, got {other:?}"),
    };
    let completion =
        crate::native_app::sample_library::folder_browser::commands::execute_folder_move_request(
            request,
        );

    let mut context = radiant::prelude::UiUpdateContext::default();
    state.finish_folder_move(std::time::Instant::now(), completion, &mut context);
    super::super::run_command_for_tests(&mut state, context.into_command());

    assert!(!kick.exists(), "source file should be moved out of drums");
    let mut reloaded =
        NativeAppState::load_default().expect("default state reloads persisted cache");
    reloaded.library.folder_browser.apply_message(
        crate::native_app::test_support::state::FolderBrowserMessage::ActivateFolder(
            drums.display().to_string(),
            Default::default(),
        ),
    );
    assert!(
        reloaded
            .library
            .folder_browser
            .selected_audio_files()
            .is_empty(),
        "restart must not resurrect moved files from the old cached folder"
    );
    reloaded.library.folder_browser.apply_message(
        crate::native_app::test_support::state::FolderBrowserMessage::ActivateFolder(
            loops.display().to_string(),
            Default::default(),
        ),
    );
    assert_eq!(
        reloaded
            .library
            .folder_browser
            .selected_audio_files()
            .into_iter()
            .map(|file| file.name.clone())
            .collect::<Vec<_>>(),
        vec![String::from("kick.wav")]
    );
}

#[test]
fn clicked_missing_cached_file_stays_removed_after_restart() {
    let config_base = tempfile::tempdir().expect("config base");
    let _base_guard = wavecrate::app_dirs::ConfigBaseGuard::set(config_base.path().to_path_buf());
    let source_root = tempfile::tempdir().expect("source root");
    let drums = source_root.path().join("drums");
    fs::create_dir_all(&drums).expect("create drums folder");
    let kick = drums.join("kick.wav");
    fs::write(&kick, [0_u8; 8]).expect("write sample");
    let source = wavecrate::sample_sources::SampleSource::new_with_id(
        wavecrate::sample_sources::SourceId::from_string("source_id::missing-cache-prune"),
        source_root.path().to_path_buf(),
    );
    wavecrate::sample_sources::config::save(&crate::native_app::test_support::config::AppConfig {
        sources: vec![source.clone()],
        core: crate::native_app::test_support::config::AppSettingsCore::default(),
    })
    .expect("seed config");
    crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[source])
        .save_source_scan_cache()
        .expect("persist stale source scan cache");
    fs::remove_file(&kick).expect("remove sample after cache is written");
    let mut state = NativeAppState::load_default().expect("default state loads persisted cache");

    let mut context = radiant::prelude::UiUpdateContext::default();
    state.select_sample(kick.display().to_string(), &mut context);
    run_command_for_tests(&mut state, context.into_command());

    let mut reloaded = NativeAppState::load_default().expect("default state reloads pruned cache");
    reloaded.library.folder_browser.apply_message(
        crate::native_app::test_support::state::FolderBrowserMessage::ActivateFolder(
            drums.display().to_string(),
            Default::default(),
        ),
    );
    assert!(
        reloaded
            .library
            .folder_browser
            .selected_audio_files()
            .is_empty(),
        "click-pruned missing files should not return from source scan cache"
    );
}
