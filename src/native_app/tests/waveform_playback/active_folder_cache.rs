use super::*;

#[test]
fn folder_activation_schedules_cache_indicator_refresh_without_ui_thread_probe() {
    let config_base = tempfile::tempdir().expect("config base");
    let (_config_lock, _base_guard) =
        set_waveform_test_config_base(config_base.path().to_path_buf());
    let source_root = tempfile::tempdir().expect("source root");
    let folder = source_root.path().join("large-folder");
    fs::create_dir_all(&folder).expect("create folder");
    let sample_path = folder.join("cached.wav");
    write_test_wav_i16(&sample_path, &[0, 1024, -2048, 4096, -1024, 512]);
    let sample_path_string = sample_path.display().to_string();

    let waveform =
        crate::native_app::test_support::state::WaveformState::load_path(sample_path.clone())
            .expect("cache sample");
    let file = waveform.file();
    crate::native_app::waveform::store_cached_waveform_file_for_tests(&file);

    let mut state = gui_state_for_span_tests();
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
        ]);
    assert!(
        !state
            .waveform
            .cache
            .cached_sample_paths
            .contains(&sample_path_string)
    );

    let mut context = ui::UiUpdateContext::default();
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::FolderBrowser(
            crate::native_app::test_support::state::FolderBrowserMessage::ActivateFolder(
                folder.display().to_string(),
                Default::default(),
            ),
        ),
        &mut context,
    );
    assert!(
        state
            .waveform
            .cache
            .indicator_refresh_task
            .active()
            .is_some(),
        "folder activation should queue cache indicator probing off the UI thread"
    );
    assert!(
        !state
            .waveform
            .cache
            .cached_sample_paths
            .contains(&sample_path_string),
        "folder activation must not synchronously read persisted cache metadata"
    );
    assert!(
        state.waveform.cache.warm_pending.is_empty(),
        "summary cache warming should wait for the background indicator probe"
    );
}

#[test]
fn folder_activation_does_not_schedule_active_folder_cache_warm() {
    let config_base = tempfile::tempdir().expect("config base");
    let (_config_lock, _base_guard) =
        set_waveform_test_config_base(config_base.path().to_path_buf());
    let source_root = tempfile::tempdir().expect("source root");
    let folder = source_root.path().join("large-folder");
    fs::create_dir_all(&folder).expect("create folder");
    write_test_wav_i16(
        &folder.join("first.wav"),
        &[0, 1024, -2048, 4096, -1024, 512],
    );
    write_test_wav_i16(&folder.join("second.wav"), &[0, 512, -512, 1024, -1024, 0]);

    let mut state = gui_state_for_span_tests();
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
        ]);
    let mut context = ui::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::FolderBrowser(
            crate::native_app::test_support::state::FolderBrowserMessage::ActivateFolder(
                folder.display().to_string(),
                Default::default(),
            ),
        ),
        &mut context,
    );
    assert!(
        state
            .waveform
            .cache
            .active_folder_warm_delay_task
            .active()
            .is_none(),
        "folder activation should not start source-wide cache warming"
    );
    assert!(
        active_folder_cache_warm_ticket(&state).is_none(),
        "active folder cache warm must not start during folder activation"
    );
    assert!(active_folder_cache_warm_plan_ticket(&state).is_none());
    assert!(state.waveform.cache.active_folder_warm_pending.is_empty());
}

#[test]
fn active_folder_cache_plan_is_visible_before_decode_batches_start() {
    let source_root = tempfile::tempdir().expect("source root");
    let first = source_root.path().join("first.wav");
    let second = source_root.path().join("second.wav");
    write_test_wav_i16(&first, &[0, 1024, -2048, 4096]);
    write_test_wav_i16(&second, &[0, 512, -512, 1024]);

    let mut state = gui_state_for_span_tests();
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
        ]);
    let mut context = ui::UiUpdateContext::default();

    state.schedule_active_folder_cache_warm(&mut context);

    let status = crate::native_app::test_support::status_bar::status_bar_projection(&state);
    assert_eq!(
        status.job_details.expect("cache plan details")[2],
        "Progress: 0/2"
    );
    assert_eq!(
        status.worker_progress.expect("worker progress"),
        crate::native_app::test_support::status_bar::WorkerProgressProjection {
            completed: 0,
            total: 2,
            current_fraction: None,
            active_animation: true,
            compact_activity: false,
        }
    );

    let ticket = active_folder_cache_warm_plan_ticket(&state).expect("source warm plan");
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::ActiveFolderCacheWarmPlanProgress(
            ui::TaskCompletion {
                ticket,
                output: crate::native_app::test_support::state::ActiveFolderCacheWarmPlanProgress {
                    folder_id: source_root.path().display().to_string(),
                    path: first,
                    checked: 1,
                    total: 2,
                    playback_ready: false,
                },
            },
        ),
        &mut context,
    );

    let status = crate::native_app::test_support::status_bar::status_bar_projection(&state);
    let details = status.job_details.expect("cache plan details");
    assert_eq!(details[2], "Progress: 1/2");
    assert!(details[3].contains("checking 50%"));
    assert!(details[3].contains("first.wav"));
    assert_eq!(
        status.worker_progress.expect("worker progress"),
        crate::native_app::test_support::status_bar::WorkerProgressProjection {
            completed: 1,
            total: 2,
            current_fraction: Some(0.5),
            active_animation: true,
            compact_activity: false,
        }
    );
}

#[test]
fn active_folder_cache_plan_uses_blocking_io_lane() {
    let source_root = tempfile::tempdir().expect("source root");
    let first = source_root.path().join("first.wav");
    let second = source_root.path().join("second.wav");
    write_test_wav_i16(&first, &[0, 1024, -2048, 4096]);
    write_test_wav_i16(&second, &[0, 512, -512, 1024]);

    let mut state = gui_state_for_span_tests();
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
        ]);
    let mut context = ui::UiUpdateContext::default();

    state.schedule_active_folder_cache_warm(&mut context);

    assert_eq!(
        context
            .into_command()
            .business_task_priority("gui-active-folder-cache-warm-plan"),
        Some(ui::TaskPriority::BlockingIo),
        "cache probing should use the limited blocking-IO lane instead of ordinary background work"
    );
}

#[test]
fn manual_source_processing_queues_entire_source_for_background_cache_warm() {
    let config_base = tempfile::tempdir().expect("config base");
    let (_config_lock, _base_guard) =
        set_waveform_test_config_base(config_base.path().to_path_buf());
    let source_root = tempfile::tempdir().expect("source root");
    let folder = source_root.path().join("large-folder");
    let nested = source_root.path().join("nested");
    fs::create_dir_all(&folder).expect("create folder");
    fs::create_dir_all(&nested).expect("create nested folder");
    for index in 0..8 {
        fs::write(folder.join(format!("sample-{index:03}.wav")), []).expect("write sample");
    }
    fs::write(nested.join("nested.wav"), []).expect("write nested sample");
    let large_file = source_root.path().join("large-source.wav");
    write_sparse_test_wav_i16(&large_file, 1, 700);

    let mut state = gui_state_for_span_tests();
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
        ]);
    let mut context = ui::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::FolderBrowser(
            crate::native_app::test_support::state::FolderBrowserMessage::ActivateFolder(
                folder.display().to_string(),
                Default::default(),
            ),
        ),
        &mut context,
    );
    assert!(state.schedule_active_folder_cache_warm(&mut context));
    let (_folder_id, pending) = state
        .library
        .folder_browser
        .selected_source_cache_warm_request()
        .expect("source warm request");
    finish_active_folder_cache_warm_plan(
        &mut state,
        &mut context,
        source_root.path().display().to_string(),
        Vec::new(),
        pending,
    );

    assert_eq!(
        state.waveform.cache.active_folder_warm_pending.len(),
        10,
        "background cache warming should cover the whole selected source, not only the active folder"
    );
}

#[test]
fn active_folder_cache_warm_tracks_worker_progress() {
    let config_base = tempfile::tempdir().expect("config base");
    let (_config_lock, _base_guard) =
        set_waveform_test_config_base(config_base.path().to_path_buf());
    let source_root = tempfile::tempdir().expect("source root");
    let first = source_root.path().join("first.wav");
    let second = source_root.path().join("second.wav");
    write_test_wav_i16(&first, &[0, 1024, -2048, 4096]);
    write_test_wav_i16(&second, &[0, 512, -512, 1024]);

    let mut state = gui_state_for_span_tests();
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
        ]);
    let mut context = ui::UiUpdateContext::default();
    state.schedule_active_folder_cache_warm(&mut context);
    finish_active_folder_cache_warm_plan(
        &mut state,
        &mut context,
        source_root.path().display().to_string(),
        Vec::new(),
        vec![first.clone(), second.clone()],
    );

    assert_eq!(state.waveform.cache.active_folder_warm_completed, 0);
    assert_eq!(state.waveform.cache.active_folder_warm_total, 2);

    let warm_ticket = state
        .waveform
        .cache
        .active_folder_warm_delay_task
        .active()
        .expect("source warm delay");
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::ActiveFolderCacheWarmReady(warm_ticket),
        &mut context,
    );

    let running_ticket = active_folder_cache_warm_ticket(&state).expect("source warm task");
    assert_eq!(state.waveform.cache.active_folder_warm_completed, 0);
    assert!(state.waveform.cache.active_folder_warm_current.is_some());

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::ActiveFolderCacheWarmFinished(
            active_folder_cache_warm_completion_with_deferred(
                running_ticket,
                source_root.path().display().to_string(),
                Vec::new(),
                vec![second.clone()],
                1,
                true,
                false,
            ),
        ),
        &mut context,
    );

    assert_eq!(state.waveform.cache.active_folder_warm_completed, 1);
    assert_eq!(state.waveform.cache.active_folder_warm_total, 2);
    assert!(state.waveform.cache.active_folder_warm_current.is_none());
    assert_eq!(
        state.waveform.cache.active_folder_warm_current_progress,
        0.0
    );
    assert!(
        state
            .waveform
            .cache
            .active_folder_warm_current_stage
            .is_none()
    );
    assert!(
        active_folder_cache_warm_ticket(&state).is_none(),
        "cache warm should cool down between files instead of chaining immediately"
    );
    assert!(
        state
            .waveform
            .cache
            .active_folder_warm_delay_task
            .active()
            .is_some(),
        "cache warm should schedule the next file after a delay"
    );
}

#[test]
fn active_folder_cache_warm_uses_ordered_stream_for_cache_ready_progress() {
    let config_base = tempfile::tempdir().expect("config base");
    let (_config_lock, _base_guard) =
        set_waveform_test_config_base(config_base.path().to_path_buf());
    let source_root = tempfile::tempdir().expect("source root");
    let first = source_root.path().join("first.wav");
    let second = source_root.path().join("second.wav");
    write_test_wav_i16(&first, &[0, 1024, -2048, 4096]);
    write_test_wav_i16(&second, &[0, 512, -512, 1024]);

    let mut state = gui_state_for_span_tests();
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
        ]);
    let mut context = ui::UiUpdateContext::default();
    state.schedule_active_folder_cache_warm(&mut context);
    finish_active_folder_cache_warm_plan(
        &mut state,
        &mut context,
        source_root.path().display().to_string(),
        Vec::new(),
        vec![first.clone(), second],
    );

    let warm_ticket = state
        .waveform
        .cache
        .active_folder_warm_delay_task
        .active()
        .expect("source warm delay");
    let mut start_context = ui::UiUpdateContext::default();
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::ActiveFolderCacheWarmReady(warm_ticket),
        &mut start_context,
    );
    let command = start_context.into_command();

    let messages = crate::native_app::tests::run_command_and_capture_messages_for_tests(command);
    let progress = messages
        .iter()
        .filter_map(|message| match message {
            crate::native_app::test_support::state::GuiMessage::ActiveFolderCacheWarmProgress(
                completion,
            ) => Some((
                completion.output.path.clone(),
                completion.output.processed,
                completion.output.stage,
            )),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        progress.len() >= 4,
        "ordered cache warming should retain multiple progress events, got {}",
        progress.len()
    );
    assert_eq!(progress[0].0, first);
    assert_eq!(
        progress[0].2,
        crate::native_app::test_support::state::ActiveFolderCacheWarmStage::CheckingCache
    );
    assert_eq!(progress[1].0, first);
    assert_eq!(
        progress[1].2,
        crate::native_app::test_support::state::ActiveFolderCacheWarmStage::LoadingCache
    );
    assert!(
        progress.windows(2).all(|events| events[0].1 <= events[1].1),
        "ordered cache progress should preserve nondecreasing processed order"
    );
    assert!(
        matches!(
            messages.last(),
            Some(
                crate::native_app::test_support::state::GuiMessage::ActiveFolderCacheWarmFinished(
                    _
                )
            )
        ),
        "ordered cache progress must be followed by final completion"
    );
}

#[test]
fn active_folder_cache_warm_progress_updates_statusbar_realtime() {
    let config_base = tempfile::tempdir().expect("config base");
    let (_config_lock, _base_guard) =
        set_waveform_test_config_base(config_base.path().to_path_buf());
    let source_root = tempfile::tempdir().expect("source root");
    let first = source_root.path().join("first.wav");
    let second = source_root.path().join("second.wav");
    write_test_wav_i16(&first, &[0, 1024, -2048, 4096]);
    write_test_wav_i16(&second, &[0, 512, -512, 1024]);

    let mut state = gui_state_for_span_tests();
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
        ]);
    let mut context = ui::UiUpdateContext::default();
    state.schedule_active_folder_cache_warm(&mut context);
    finish_active_folder_cache_warm_plan(
        &mut state,
        &mut context,
        source_root.path().display().to_string(),
        Vec::new(),
        vec![first.clone(), second],
    );
    let warm_ticket = state
        .waveform
        .cache
        .active_folder_warm_delay_task
        .active()
        .expect("source warm delay");
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::ActiveFolderCacheWarmReady(warm_ticket),
        &mut context,
    );
    let running_ticket = active_folder_cache_warm_ticket(&state).expect("source warm task");
    let folder_id = source_root.path().display().to_string();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::ActiveFolderCacheWarmProgress(
            ui::KeyedTaskCompletion {
                key: crate::native_app::audio::sample_load_actions::active_folder_cache_warm_resource_key(
                    folder_id.as_str(),
                ),
                ticket: running_ticket,
                output: crate::native_app::test_support::state::ActiveFolderCacheWarmProgress {
                    folder_id,
                    path: first.clone(),
                    processed: 0,
                    current_progress: 0.42,
                    stage: crate::native_app::test_support::state::ActiveFolderCacheWarmStage::Decoding,
                    cached: false,
                },
            },
        ),
        &mut context,
    );

    let status = crate::native_app::test_support::status_bar::status_bar_projection(&state);
    let worker = status.worker_progress.expect("source warm progress");
    assert_eq!(worker.completed, 0);
    assert_eq!(worker.total, 2);
    assert_eq!(worker.current_fraction, Some(0.42));
    assert!(worker.active_animation);
    assert!(
        status.status_text.contains("decoding 42%"),
        "status should expose the current cache phase and file progress: {}",
        status.status_text
    );
    assert!(
        status.status_text.contains("first.wav"),
        "status should name the file currently being cached: {}",
        status.status_text
    );
}

#[test]
fn active_folder_cache_progress_promotes_completed_row_immediately() {
    let source_root = tempfile::tempdir().expect("source root");
    let sample_path = source_root.path().join("ready-row.wav");
    write_test_wav_i16(&sample_path, &[0, 1024, -2048, 4096]);
    let sample_path_string = sample_path.display().to_string();

    let mut state = gui_state_for_span_tests();
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
        ]);
    let theme = radiant::theme::ThemeTokens::default();
    crate::native_app::test_support::sample_browser::prepare_sample_browser_view(&mut state);
    let unloaded_frame = crate::native_app::test_support::sample_browser::sample_browser(&state)
        .view_frame_at_size(Vector2::new(720.0, 360.0), &theme);
    assert_eq!(
        unloaded_frame.paint_plan.first_text_color("ready-row"),
        Some(theme.text_primary),
        "uncached rows should remain readable"
    );

    let mut context = ui::UiUpdateContext::default();
    state.schedule_active_folder_cache_warm(&mut context);
    finish_active_folder_cache_warm_plan(
        &mut state,
        &mut context,
        source_root.path().display().to_string(),
        Vec::new(),
        vec![sample_path.clone()],
    );
    let warm_ticket = state
        .waveform
        .cache
        .active_folder_warm_delay_task
        .active()
        .expect("source warm delay");
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::ActiveFolderCacheWarmReady(warm_ticket),
        &mut context,
    );
    let running_ticket = active_folder_cache_warm_ticket(&state).expect("source warm task");
    let folder_id = source_root.path().display().to_string();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::ActiveFolderCacheWarmProgress(
            ui::KeyedTaskCompletion {
                key: crate::native_app::audio::sample_load_actions::active_folder_cache_warm_resource_key(
                    folder_id.as_str(),
                ),
                ticket: running_ticket,
                output: crate::native_app::test_support::state::ActiveFolderCacheWarmProgress {
                    folder_id,
                    path: sample_path.clone(),
                    processed: 1,
                    current_progress: 1.0,
                    stage: crate::native_app::test_support::state::ActiveFolderCacheWarmStage::Ready,
                    cached: true,
                },
            },
        ),
        &mut context,
    );

    assert!(
        state
            .waveform
            .cache
            .cached_sample_paths
            .contains(&sample_path_string),
        "ready progress should mark the row as cached before the worker finishes"
    );
    crate::native_app::test_support::sample_browser::prepare_sample_browser_view(&mut state);
    let loaded_frame = crate::native_app::test_support::sample_browser::sample_browser(&state)
        .view_frame_at_size(Vector2::new(720.0, 360.0), &theme);
    assert_eq!(
        loaded_frame.paint_plan.first_text_color("ready-row"),
        Some(theme.text_primary),
        "ready progress should repaint the row with loaded text"
    );
}

#[test]
fn active_folder_cache_warm_waits_while_sample_load_is_foreground() {
    let source_root = tempfile::tempdir().expect("source root");
    let folder = source_root.path().join("large-folder");
    fs::create_dir_all(&folder).expect("create folder");
    let first = folder.join("first.wav");
    let second = folder.join("second.wav");
    write_test_wav_i16(&first, &[0, 1024, -2048, 4096]);
    write_test_wav_i16(&second, &[0, 512, -512, 1024]);

    let mut state = gui_state_for_span_tests();
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
        ]);
    let mut context = ui::UiUpdateContext::default();
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::FolderBrowser(
            crate::native_app::test_support::state::FolderBrowserMessage::ActivateFolder(
                folder.display().to_string(),
                Default::default(),
            ),
        ),
        &mut context,
    );
    assert!(state.schedule_active_folder_cache_warm(&mut context));
    finish_active_folder_cache_warm_plan(
        &mut state,
        &mut context,
        source_root.path().display().to_string(),
        Vec::new(),
        vec![first.clone(), second.clone()],
    );
    let warm_ticket = state
        .waveform
        .cache
        .active_folder_warm_delay_task
        .active()
        .expect("folder warm delay");

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::SelectSampleWithModifiers {
            path: first.display().to_string(),
            modifiers: Default::default(),
        },
        &mut context,
    );
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::ActiveFolderCacheWarmReady(warm_ticket),
        &mut context,
    );

    assert!(
        active_folder_cache_warm_ticket(&state).is_none(),
        "background folder cache warm must not start while a foreground sample load is pending"
    );
    assert!(
        state
            .waveform
            .cache
            .active_folder_warm_delay_task
            .active()
            .is_some(),
        "folder cache warm should retry later instead of competing with selection"
    );
    assert_eq!(
        state.waveform.cache.active_folder_warm_pending.len(),
        2,
        "foreground selection must not drain warm candidates"
    );
}

#[test]
fn sample_selection_pauses_running_active_folder_cache_warm_without_hiding_progress() {
    let source_root = tempfile::tempdir().expect("source root");
    let folder = source_root.path().join("large-folder");
    fs::create_dir_all(&folder).expect("create folder");
    let first = folder.join("first.wav");
    let second = folder.join("second.wav");
    write_test_wav_i16(&first, &[0, 1024, -2048, 4096]);
    write_test_wav_i16(&second, &[0, 512, -512, 1024]);

    let mut state = gui_state_for_span_tests();
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
        ]);
    let mut context = ui::UiUpdateContext::default();
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::FolderBrowser(
            crate::native_app::test_support::state::FolderBrowserMessage::ActivateFolder(
                folder.display().to_string(),
                Default::default(),
            ),
        ),
        &mut context,
    );
    assert!(state.schedule_active_folder_cache_warm(&mut context));
    finish_active_folder_cache_warm_plan(
        &mut state,
        &mut context,
        source_root.path().display().to_string(),
        Vec::new(),
        vec![first.clone(), second.clone()],
    );
    let warm_ticket = state
        .waveform
        .cache
        .active_folder_warm_delay_task
        .active()
        .expect("folder warm delay");
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::ActiveFolderCacheWarmReady(warm_ticket),
        &mut context,
    );
    let running_ticket = active_folder_cache_warm_ticket(&state)
        .expect("test setup should start active-folder cache warming");

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::SelectSampleWithModifiers {
            path: first.display().to_string(),
            modifiers: Default::default(),
        },
        &mut context,
    );

    assert!(
        active_folder_cache_warm_ticket(&state).is_some(),
        "foreground selection should keep the draining cache worker tracked until it reports cancellation"
    );
    assert!(
        state.waveform.cache.active_folder_warm_cancel.is_none(),
        "foreground selection must request cooperative cancellation for background cache work"
    );
    assert!(
        active_sample_load_validation_ticket(&state).is_some(),
        "foreground sample validation should be queued after cancelling background warm work"
    );
    let status = crate::native_app::test_support::status_bar::status_bar_projection(&state);
    assert!(
        !status.status_text.starts_with("Caching source samples"),
        "background work belongs in the progress control, not status text: {}",
        status.status_text
    );
    assert!(
        status.worker_progress.is_some(),
        "status bar should keep the worker progress bar while the cache worker drains"
    );

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::ActiveFolderCacheWarmFinished(
            active_folder_cache_warm_completion_with_deferred(
                running_ticket,
                source_root.path().display().to_string(),
                Vec::new(),
                vec![first, second],
                0,
                false,
                true,
            ),
        ),
        &mut context,
    );

    assert!(
        active_folder_cache_warm_ticket(&state).is_none(),
        "cancelled cache worker should finish and release its tracked task"
    );
    assert_eq!(
        state.waveform.cache.active_folder_warm_pending.len(),
        2,
        "cancelled cache worker should return unprocessed files to the warm queue"
    );
    assert!(
        state
            .waveform
            .cache
            .active_folder_warm_delay_task
            .active()
            .is_some(),
        "active folder cache warm should retry after foreground work yields"
    );
}

#[test]
fn active_folder_cache_warm_yields_while_normalization_is_active() {
    let config_base = tempfile::tempdir().expect("config base");
    let (_config_lock, _base_guard) =
        set_waveform_test_config_base(config_base.path().to_path_buf());
    let source_root = tempfile::tempdir().expect("source root");
    let sample_path = source_root.path().join("normalize-yield.wav");
    write_test_wav_i16(&sample_path, &[0, 1024, -2048, 4096]);

    let mut state = gui_state_for_span_tests();
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
        ]);
    let mut context = ui::UiUpdateContext::default();
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::FolderBrowser(
            crate::native_app::test_support::state::FolderBrowserMessage::ActivateFolder(
                source_root.path().display().to_string(),
                Default::default(),
            ),
        ),
        &mut context,
    );
    assert!(state.schedule_active_folder_cache_warm(&mut context));
    finish_active_folder_cache_warm_plan(
        &mut state,
        &mut context,
        source_root.path().display().to_string(),
        Vec::new(),
        vec![sample_path.clone()],
    );
    state.background.normalization_progress = Some(
        crate::native_app::test_support::state::NormalizationProgress {
            task_id: 12,
            label: String::from("1 sample"),
            completed: 0,
            total: 1,
            work_completed: 100,
            work_total: 1_000,
            queued: 0,
            detail: String::from("normalize-yield.wav"),
        },
    );

    let warm_ticket = state
        .waveform
        .cache
        .active_folder_warm_delay_task
        .active()
        .expect("folder warm delay");
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::ActiveFolderCacheWarmReady(warm_ticket),
        &mut context,
    );

    assert!(
        active_folder_cache_warm_ticket(&state).is_none(),
        "normalization should keep active-folder cache warm from starting"
    );
    assert!(
        state
            .waveform
            .cache
            .active_folder_warm_delay_task
            .active()
            .is_some(),
        "cache warm should be delayed for later instead of competing with normalization"
    );
}

#[test]
fn changing_folder_in_same_source_keeps_active_source_cache_warm() {
    let config_base = tempfile::tempdir().expect("config base");
    let (_config_lock, _base_guard) =
        set_waveform_test_config_base(config_base.path().to_path_buf());
    let source_root = tempfile::tempdir().expect("source root");
    let first_folder = source_root.path().join("first-folder");
    let second_folder = source_root.path().join("second-folder");
    fs::create_dir_all(&first_folder).expect("create first folder");
    fs::create_dir_all(&second_folder).expect("create second folder");
    write_test_wav_i16(&first_folder.join("first.wav"), &[0, 1024, -2048, 4096]);
    write_test_wav_i16(&second_folder.join("second.wav"), &[0, 512, -512, 1024]);

    let mut state = gui_state_for_span_tests();
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
        ]);
    let mut context = ui::UiUpdateContext::default();
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::FolderBrowser(
            crate::native_app::test_support::state::FolderBrowserMessage::ActivateFolder(
                first_folder.display().to_string(),
                Default::default(),
            ),
        ),
        &mut context,
    );
    assert!(state.schedule_active_folder_cache_warm(&mut context));
    finish_active_folder_cache_warm_plan(
        &mut state,
        &mut context,
        source_root.path().display().to_string(),
        Vec::new(),
        vec![
            first_folder.join("first.wav"),
            second_folder.join("second.wav"),
        ],
    );
    let first_ticket = state
        .waveform
        .cache
        .active_folder_warm_delay_task
        .active()
        .expect("first folder warm delay");
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::ActiveFolderCacheWarmReady(
            first_ticket,
        ),
        &mut context,
    );
    let running_ticket =
        active_folder_cache_warm_ticket(&state).expect("source warm should be running");

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::FolderBrowser(
            crate::native_app::test_support::state::FolderBrowserMessage::ActivateFolder(
                second_folder.display().to_string(),
                Default::default(),
            ),
        ),
        &mut context,
    );

    assert_eq!(
        active_folder_cache_warm_ticket(&state),
        Some(running_ticket),
        "same-source folder navigation should not restart a source-wide warm worker"
    );
    assert!(
        active_folder_cache_warm_plan_ticket(&state).is_none(),
        "same-source folder navigation should not queue a replacement cache plan"
    );
    let source_warm_id = source_root.path().display().to_string();
    assert_eq!(
        state.waveform.cache.active_folder_warm_folder_id.as_deref(),
        Some(source_warm_id.as_str())
    );
    assert!(
        state.waveform.cache.active_folder_warm_current.is_some(),
        "the original warm worker should keep its current file"
    );
}

#[test]
fn active_folder_cache_warm_does_not_chain_batches_while_playing() {
    let source_root = tempfile::tempdir().expect("source root");
    let folder = source_root.path().join("large-folder");
    fs::create_dir_all(&folder).expect("create folder");
    write_test_wav_i16(&folder.join("first.wav"), &[0, 1024, -2048, 4096]);
    let second = folder.join("second.wav");
    write_test_wav_i16(&second, &[0, 512, -512, 1024]);

    let mut state = gui_state_for_span_tests();
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
        ]);
    let mut context = ui::UiUpdateContext::default();
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::FolderBrowser(
            crate::native_app::test_support::state::FolderBrowserMessage::ActivateFolder(
                folder.display().to_string(),
                Default::default(),
            ),
        ),
        &mut context,
    );
    assert!(state.schedule_active_folder_cache_warm(&mut context));
    finish_active_folder_cache_warm_plan(
        &mut state,
        &mut context,
        source_root.path().display().to_string(),
        Vec::new(),
        vec![folder.join("first.wav"), second.clone()],
    );
    let warm_ticket = state
        .waveform
        .cache
        .active_folder_warm_delay_task
        .active()
        .expect("folder warm delay");
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::ActiveFolderCacheWarmReady(warm_ticket),
        &mut context,
    );
    let running_ticket = active_folder_cache_warm_ticket(&state).expect("folder warm task");

    state.waveform.current.start_playback(0.0);
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::ActiveFolderCacheWarmFinished(
            active_folder_cache_warm_completion_with_deferred(
                running_ticket,
                source_root.path().display().to_string(),
                Vec::new(),
                vec![second.clone()],
                1,
                true,
                false,
            ),
        ),
        &mut context,
    );

    assert!(
        active_folder_cache_warm_ticket(&state).is_none(),
        "completed warm batches must not immediately start another batch during playback"
    );
    assert!(
        state
            .waveform
            .cache
            .active_folder_warm_delay_task
            .active()
            .is_none(),
        "active folder cache warm should not schedule retry pulses during playback"
    );
    assert_eq!(
        state.waveform.cache.active_folder_warm_pending.len(),
        1,
        "only the already-started single-file batch may be drained"
    );

    state.waveform.current.stop_playback();
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::Frame,
        &mut ui::UiUpdateContext::default(),
    );

    assert!(
        active_folder_cache_warm_ticket(&state).is_some(),
        "active folder cache warm should resume from the normal idle frame path"
    );
}

#[test]
fn running_active_folder_cache_warm_pause_for_playback_preserves_and_resumes_backlog() {
    let source_root = tempfile::tempdir().expect("source root");
    let folder = source_root.path().join("large-folder");
    fs::create_dir_all(&folder).expect("create folder");
    let first = folder.join("first.wav");
    let second = folder.join("second.wav");
    write_test_wav_i16(&first, &[0, 1024, -2048, 4096]);
    write_test_wav_i16(&second, &[0, 512, -512, 1024]);

    let mut state = gui_state_for_span_tests();
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
        ]);
    let mut context = ui::UiUpdateContext::default();
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::FolderBrowser(
            crate::native_app::test_support::state::FolderBrowserMessage::ActivateFolder(
                folder.display().to_string(),
                Default::default(),
            ),
        ),
        &mut context,
    );
    assert!(state.schedule_active_folder_cache_warm(&mut context));
    finish_active_folder_cache_warm_plan(
        &mut state,
        &mut context,
        source_root.path().display().to_string(),
        Vec::new(),
        vec![first, second.clone()],
    );
    let warm_ticket = state
        .waveform
        .cache
        .active_folder_warm_delay_task
        .active()
        .expect("folder warm delay");
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::ActiveFolderCacheWarmReady(warm_ticket),
        &mut context,
    );
    let running_ticket = active_folder_cache_warm_ticket(&state).expect("folder warm task");

    state.waveform.current.start_playback(0.0);
    state.pause_active_folder_cache_warm_for_playback();

    assert!(
        active_folder_cache_warm_ticket(&state).is_some(),
        "running warm task should stay tracked until its cancellation completion arrives"
    );
    assert!(
        state.waveform.cache.active_folder_warm_folder_id.is_some(),
        "playback pause should retain the durable source-cache job"
    );
    assert!(
        state.waveform.cache.active_folder_warm_pending.is_empty(),
        "the running batch returns its deferred paths through cancellation completion"
    );

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::ActiveFolderCacheWarmFinished(
            active_folder_cache_warm_completion_with_deferred(
                running_ticket,
                source_root.path().display().to_string(),
                Vec::new(),
                vec![second.clone()],
                1,
                true,
                true,
            ),
        ),
        &mut context,
    );
    assert!(
        active_folder_cache_warm_ticket(&state).is_none(),
        "cancelled warm completion should retire the tracked running task"
    );
    assert!(
        state
            .waveform
            .cache
            .active_folder_warm_pending
            .iter()
            .any(|path| path == &second),
        "cancelled warm completion should restore unfinished playback-time work"
    );

    state.waveform.current.stop_playback();
    state.maybe_start_active_folder_cache_warm(&mut context);
    assert!(
        active_folder_cache_warm_ticket(&state).is_some(),
        "the preserved backlog should resume once playback stops"
    );
}

#[test]
fn active_folder_cache_warm_delay_does_not_retry_while_playing() {
    let source_root = tempfile::tempdir().expect("source root");
    let folder = source_root.path().join("large-folder");
    fs::create_dir_all(&folder).expect("create folder");
    let first = folder.join("first.wav");
    let second = folder.join("second.wav");
    write_test_wav_i16(&first, &[0, 1024, -2048, 4096]);
    write_test_wav_i16(&second, &[0, 512, -512, 1024]);

    let mut state = gui_state_for_span_tests();
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
        ]);
    let mut context = ui::UiUpdateContext::default();
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::FolderBrowser(
            crate::native_app::test_support::state::FolderBrowserMessage::ActivateFolder(
                folder.display().to_string(),
                Default::default(),
            ),
        ),
        &mut context,
    );
    assert!(state.schedule_active_folder_cache_warm(&mut context));
    finish_active_folder_cache_warm_plan(
        &mut state,
        &mut context,
        source_root.path().display().to_string(),
        Vec::new(),
        vec![first, second],
    );
    let warm_ticket = state
        .waveform
        .cache
        .active_folder_warm_delay_task
        .active()
        .expect("folder warm delay");

    state.waveform.current.start_playback(0.0);
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::ActiveFolderCacheWarmReady(warm_ticket),
        &mut context,
    );

    assert!(active_folder_cache_warm_ticket(&state).is_none());
    assert!(
        state
            .waveform
            .cache
            .active_folder_warm_delay_task
            .active()
            .is_none(),
        "a warm delay that fires during playback should not schedule another playback-time wakeup"
    );
    assert_eq!(
        state.waveform.cache.active_folder_warm_pending.len(),
        2,
        "playback-time warm delay should not drain cache candidates"
    );

    state.waveform.current.stop_playback();
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::Frame,
        &mut ui::UiUpdateContext::default(),
    );

    assert!(
        active_folder_cache_warm_ticket(&state).is_some(),
        "idle frames should resume deferred active-folder cache warm work"
    );
}

#[test]
fn active_folder_cache_warm_generates_playback_ready_cache_for_uncached_file() {
    let config_base = tempfile::tempdir().expect("config base");
    let (_config_lock, _base_guard) =
        set_waveform_test_config_base(config_base.path().to_path_buf());
    let source_root = tempfile::tempdir().expect("source root");
    let sample_path = source_root.path().join("uncached-warm.wav");
    write_test_wav_i16(&sample_path, &[0, 1024, -2048, 4096, -1024, 512]);
    let sample_path = PathBuf::from(sample_path.display().to_string());

    assert!(!crate::native_app::waveform::cached_waveform_file_playback_ready_exists(&sample_path));

    let result = crate::native_app::audio::sample_load_actions::warm_active_folder_waveform_cache(
        crate::native_app::app::ActiveFolderCacheWarmRequest::new(
            String::from("source"),
            vec![sample_path.clone()],
        ),
        || false,
    );
    crate::native_app::waveform::flush_background_waveform_cache_stores_for_shutdown();

    assert_eq!(result.loaded.len(), 1);
    assert_eq!(result.processed, 1);
    assert!(result.decoded_source);
    assert!(result.deferred.is_empty());
    assert!(
        crate::native_app::waveform::cached_waveform_file_playback_ready_exists(&sample_path),
        "active folder warm should persist playback readiness for future selection"
    );
}

#[test]
fn active_folder_cache_warm_builds_summary_cache_for_large_uncached_source_files() {
    let config_base = tempfile::tempdir().expect("config base");
    let (_config_lock, _base_guard) =
        set_waveform_test_config_base(config_base.path().to_path_buf());
    let source_root = tempfile::tempdir().expect("source root");
    let sample_path = source_root.path().join("large-summary.wav");
    write_sparse_test_wav_i16(&sample_path, 1, 700);
    let sample_path = PathBuf::from(sample_path.display().to_string());

    let result = crate::native_app::audio::sample_load_actions::warm_active_folder_waveform_cache(
        crate::native_app::app::ActiveFolderCacheWarmRequest::new(
            String::from("source"),
            vec![sample_path.clone()],
        ),
        || false,
    );
    crate::native_app::waveform::flush_background_waveform_cache_stores_for_shutdown();

    assert_eq!(result.loaded.len(), 1);
    assert_eq!(result.processed, 1);
    assert!(result.decoded_source);
    assert!(result.deferred.is_empty());
    let (_path, file) = result.loaded.into_iter().next().expect("loaded summary");
    let waveform = crate::native_app::test_support::state::WaveformState::from_cached_file(file);
    assert_eq!(
        waveform.playback_source_file().as_deref(),
        Some(sample_path.as_path())
    );
    assert!(waveform.playback_samples().is_none());
    assert!(
        crate::native_app::waveform::cached_waveform_file_exists(&sample_path),
        "large source files should persist a reusable waveform summary"
    );
    assert!(
        !crate::native_app::waveform::cached_waveform_file_playback_ready_exists(&sample_path),
        "large source files should avoid full playback sidecar warming"
    );
}

#[test]
fn active_folder_cache_plan_queues_large_file_backed_summary_cache_for_memory_warm() {
    let config_base = tempfile::tempdir().expect("config base");
    let (_config_lock, _base_guard) =
        set_waveform_test_config_base(config_base.path().to_path_buf());
    let source_root = tempfile::tempdir().expect("source root");
    let sample_path = source_root.path().join("large-summary-restart.wav");
    write_sparse_test_wav_i16(&sample_path, 1, 700);
    let sample_path = PathBuf::from(sample_path.display().to_string());

    let warm_result =
        crate::native_app::audio::sample_load_actions::warm_active_folder_waveform_cache(
            crate::native_app::app::ActiveFolderCacheWarmRequest::new(
                String::from("source"),
                vec![sample_path.clone()],
            ),
            || false,
        );
    crate::native_app::waveform::flush_background_waveform_cache_stores_for_shutdown();
    assert_eq!(warm_result.loaded.len(), 1);
    assert!(crate::native_app::waveform::cached_waveform_file_exists(
        &sample_path
    ));
    assert!(
        !crate::native_app::waveform::cached_waveform_file_playback_ready_exists(&sample_path),
        "test setup should produce a summary-only cache"
    );

    let plan =
        crate::native_app::audio::sample_load_actions::plan_active_folder_waveform_cache_warm(
            crate::native_app::app::ActiveFolderCacheWarmRequest::new(
                source_root.path().display().to_string(),
                vec![sample_path.clone()],
            ),
            || false,
        );

    assert_eq!(
        plan.playback_ready,
        Vec::<PathBuf>::new(),
        "source prep should not treat persisted summaries as memory-ready"
    );
    assert_eq!(
        plan.pending,
        vec![sample_path],
        "source prep should hydrate large summary caches for instant node playback"
    );
}

#[test]
fn active_folder_cache_warm_hydrates_playback_ready_cache_hits() {
    let config_base = tempfile::tempdir().expect("config base");
    let (_config_lock, _base_guard) =
        set_waveform_test_config_base(config_base.path().to_path_buf());
    let source_root = tempfile::tempdir().expect("source root");
    let first = source_root.path().join("cached-first.wav");
    let second = source_root.path().join("cached-second.wav");
    write_test_wav_i16(&first, &[0, 1024, -2048, 4096]);
    write_test_wav_i16(&second, &[0, 512, -512, 1024]);

    for path in [&first, &second] {
        let waveform =
            crate::native_app::test_support::state::WaveformState::load_path_for_foreground_audition(
                path.clone(),
                |_| {},
                || false,
                |_| {},
            )
            .expect("cache sample");
        crate::native_app::waveform::flush_background_waveform_cache_stores_for_shutdown();
        crate::native_app::waveform::store_cached_waveform_file_for_tests(&waveform.file());
        wait_for_playback_ready_cache(path.display().to_string().as_str());
    }

    let result = crate::native_app::audio::sample_load_actions::warm_active_folder_waveform_cache(
        crate::native_app::app::ActiveFolderCacheWarmRequest::new(
            String::from("source"),
            vec![first.clone(), second.clone()],
        ),
        || false,
    );

    assert_eq!(
        result
            .loaded
            .iter()
            .map(|(path, _)| path.clone())
            .collect::<Vec<_>>(),
        vec![first, second],
        "source prep should hydrate multiple persisted cache hits in one worker batch"
    );
    assert!(result.playback_ready.is_empty());
    assert_eq!(result.processed, 2);
    assert!(!result.decoded_source);
    assert!(result.deferred.is_empty());
}

#[test]
fn active_folder_cache_warm_resumes_from_persisted_playback_ready_cache_after_restart() {
    let config_base = tempfile::tempdir().expect("config base");
    let (_config_lock, _base_guard) =
        set_waveform_test_config_base(config_base.path().to_path_buf());
    let source_root = tempfile::tempdir().expect("source root");
    let cached_first = source_root.path().join("cached-first.wav");
    let cached_second = source_root.path().join("cached-second.wav");
    let uncached = source_root.path().join("uncached-after-restart.wav");
    write_test_wav_i16(&cached_first, &[0, 1024, -2048, 4096]);
    write_test_wav_i16(&cached_second, &[0, 512, -512, 1024]);
    write_test_wav_i16(&uncached, &[0, 256, -256, 512]);

    for path in [&cached_first, &cached_second] {
        let waveform =
            crate::native_app::test_support::state::WaveformState::load_path_for_foreground_audition(
                path.clone(),
                |_| {},
                || false,
                |_| {},
            )
            .expect("cache sample before restart");
        crate::native_app::waveform::flush_background_waveform_cache_stores_for_shutdown();
        crate::native_app::waveform::store_cached_waveform_file_for_tests(&waveform.file());
        wait_for_playback_ready_cache(path.display().to_string().as_str());
    }

    let mut restarted_state = gui_state_for_span_tests();
    restarted_state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
        ]);
    let mut context = ui::UiUpdateContext::default();
    restarted_state.schedule_active_folder_cache_warm(&mut context);
    finish_active_folder_cache_warm_plan(
        &mut restarted_state,
        &mut context,
        source_root.path().display().to_string(),
        vec![cached_first.clone(), cached_second.clone()],
        vec![uncached.clone()],
    );
    assert_eq!(restarted_state.waveform.cache.active_folder_warm_total, 1);

    let warm_ticket = restarted_state
        .waveform
        .cache
        .active_folder_warm_delay_task
        .active()
        .expect("source warm delay");
    restarted_state.apply_message(
        crate::native_app::test_support::state::GuiMessage::ActiveFolderCacheWarmReady(warm_ticket),
        &mut context,
    );
    let running_ticket =
        active_folder_cache_warm_ticket(&restarted_state).expect("source warm task");
    assert!(
        restarted_state
            .waveform
            .cache
            .active_folder_warm_pending
            .is_empty(),
        "restart warm should scan cached files and the next uncached candidate in one worker batch"
    );

    let folder_id = source_root.path().display().to_string();
    let result = crate::native_app::audio::sample_load_actions::warm_active_folder_waveform_cache(
        crate::native_app::app::ActiveFolderCacheWarmRequest::new(
            folder_id.clone(),
            vec![uncached.clone()],
        ),
        || false,
    );
    assert!(result.playback_ready.is_empty());
    assert_eq!(result.loaded.len(), 1);
    assert_eq!(result.processed, 1);
    assert!(result.decoded_source);
    assert!(result.deferred.is_empty());

    restarted_state.apply_message(
        crate::native_app::test_support::state::GuiMessage::ActiveFolderCacheWarmFinished(
            ui::KeyedTaskCompletion {
                key: crate::native_app::audio::sample_load_actions::active_folder_cache_warm_resource_key(
                    folder_id.as_str(),
                ),
                ticket: running_ticket,
                output: result,
            },
        ),
        &mut context,
    );

    assert!(
        restarted_state
            .waveform
            .cache
            .active_folder_warm_folder_id
            .is_none(),
        "finished restart warm should not continue scheduling already cached files"
    );
    for path in [&cached_first, &cached_second, &uncached] {
        assert!(
            restarted_state
                .waveform
                .cache
                .cached_sample_paths
                .contains(&path.display().to_string()),
            "completed restart warm should mark {} as cached",
            path.display()
        );
    }
}

#[test]
fn active_folder_cache_plan_queues_processed_source_for_memory_warm() {
    let config_base = tempfile::tempdir().expect("config base");
    let (_config_lock, _base_guard) =
        set_waveform_test_config_base(config_base.path().to_path_buf());
    let source_root = tempfile::tempdir().expect("source root");
    let first = source_root.path().join("cached-first.wav");
    let second = source_root.path().join("cached-second.wav");
    write_test_wav_i16(&first, &[0, 1024, -2048, 4096]);
    write_test_wav_i16(&second, &[0, 512, -512, 1024]);

    for path in [&first, &second] {
        let waveform =
            crate::native_app::test_support::state::WaveformState::load_path_for_foreground_audition(
                path.clone(),
                |_| {},
                || false,
                |_| {},
            )
            .expect("cache sample before restart");
        crate::native_app::waveform::flush_background_waveform_cache_stores_for_shutdown();
        crate::native_app::waveform::store_cached_waveform_file_for_tests(&waveform.file());
        wait_for_playback_ready_cache(path.display().to_string().as_str());
    }

    let mut restarted_state = gui_state_for_span_tests();
    restarted_state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
        ]);
    let (folder_id, paths) = restarted_state
        .library
        .folder_browser
        .selected_source_cache_warm_request()
        .expect("source warm request");
    let plan =
        crate::native_app::audio::sample_load_actions::plan_active_folder_waveform_cache_warm(
            crate::native_app::app::ActiveFolderCacheWarmRequest::new(folder_id.clone(), paths),
            || false,
        );
    assert_eq!(plan.playback_ready, Vec::<PathBuf>::new());
    assert_eq!(plan.pending, vec![first.clone(), second.clone()]);

    let mut context = ui::UiUpdateContext::default();
    restarted_state.schedule_active_folder_cache_warm(&mut context);
    let ticket = active_folder_cache_warm_plan_ticket(&restarted_state).expect("source warm plan");
    restarted_state.apply_message(
        crate::native_app::test_support::state::GuiMessage::ActiveFolderCacheWarmPlanned(
            ui::TaskCompletion {
                ticket,
                output: plan,
            },
        ),
        &mut context,
    );

    assert!(
        restarted_state
            .waveform
            .cache
            .active_folder_warm_delay_task
            .active()
            .is_some(),
        "processed sources should still warm persisted cache into memory"
    );
    assert_eq!(
        restarted_state
            .waveform
            .cache
            .active_folder_warm_pending
            .iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec![first, second]
    );
}

#[test]
fn active_folder_cache_plan_only_reprocesses_changed_files_after_normalize() {
    let config_base = tempfile::tempdir().expect("config base");
    let (_config_lock, _base_guard) =
        set_waveform_test_config_base(config_base.path().to_path_buf());
    let source_root = tempfile::tempdir().expect("source root");
    let processed = source_root.path().join("processed.wav");
    let changed = source_root.path().join("changed-by-normalize.wav");
    write_test_wav_i16(&processed, &[0, 1024, -2048, 4096]);
    write_test_wav_i16(&changed, &[0, 512, -512, 1024]);

    for path in [&processed, &changed] {
        let waveform =
            crate::native_app::test_support::state::WaveformState::load_path_for_foreground_audition(
                path.clone(),
                |_| {},
                || false,
                |_| {},
            )
            .expect("cache sample before edit");
        crate::native_app::waveform::flush_background_waveform_cache_stores_for_shutdown();
        crate::native_app::waveform::store_cached_waveform_file_for_tests(&waveform.file());
        wait_for_playback_ready_cache(path.display().to_string().as_str());
    }

    write_test_wav_i16(&changed, &[0, 256, -256, 512]);
    filetime::set_file_mtime(
        &changed,
        filetime::FileTime::from_unix_time(4_000_000_000, 0),
    )
    .expect("force changed file identity");

    let plan =
        crate::native_app::audio::sample_load_actions::plan_active_folder_waveform_cache_warm(
            crate::native_app::app::ActiveFolderCacheWarmRequest::new(
                source_root.path().display().to_string(),
                vec![processed.clone(), changed.clone()],
            ),
            || false,
        );

    assert_eq!(
        plan.playback_ready,
        Vec::<PathBuf>::new(),
        "persisted-ready files still need memory warming for instant node playback"
    );
    assert_eq!(
        plan.pending,
        vec![processed, changed],
        "source prep should revisit both persisted-ready and changed files"
    );
    assert!(!plan.cancelled);
}

#[test]
fn normalize_finish_keeps_changed_file_in_active_folder_cache_warm_queue() {
    let config_base = tempfile::tempdir().expect("config base");
    let (_config_lock, _base_guard) =
        set_waveform_test_config_base(config_base.path().to_path_buf());
    let source_root = tempfile::tempdir().expect("source root");
    let sample_path = source_root.path().join("normalize-rewarm.wav");
    write_test_wav_i16(&sample_path, &[0, 1024, -2048, 4096]);
    let sample_id = sample_path.display().to_string();
    let source_id = source_root.path().display().to_string();

    let mut state = gui_state_for_span_tests();
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
        ]);
    let mut context = ui::UiUpdateContext::default();
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::FolderBrowser(
            crate::native_app::test_support::state::FolderBrowserMessage::ActivateFolder(
                source_root.path().display().to_string(),
                Default::default(),
            ),
        ),
        &mut context,
    );
    assert!(state.schedule_active_folder_cache_warm(&mut context));
    finish_active_folder_cache_warm_plan(
        &mut state,
        &mut context,
        source_id.clone(),
        Vec::new(),
        vec![sample_path.clone()],
    );
    assert!(
        state
            .waveform
            .cache
            .active_folder_warm_pending
            .contains(&sample_path)
    );

    crate::native_app::test_support::waveform::normalize_wav_file_in_place(&sample_path)
        .expect("normalize wav");
    state.background.normalization_progress = Some(
        crate::native_app::test_support::state::NormalizationProgress {
            task_id: 42,
            label: String::from("1 sample"),
            completed: 1,
            total: 1,
            work_completed: 1_000,
            work_total: 1_000,
            queued: 0,
            detail: sample_id.clone(),
        },
    );
    let normalization_source_id = state
        .library
        .folder_browser
        .selected_source_id()
        .to_string();
    state.finish_normalization(
        crate::native_app::app::NormalizationResult {
            task_id: 42,
            source_id: normalization_source_id,
            loaded_path: sample_path.clone(),
            normalizing_loaded: false,
            was_playing: false,
            restart_ratio: 0.0,
            restart_span: None,
            normalized: vec![sample_path.clone()],
            refreshed_files: Vec::new(),
            skipped: Vec::new(),
            failed: Vec::new(),
            harvest_derivations: Vec::new(),
        },
        &mut context,
    );

    assert!(
        state
            .waveform
            .cache
            .active_folder_warm_pending
            .contains(&sample_path),
        "normalization should leave the edited file queued for active-folder recache"
    );
    assert!(
        !state
            .waveform
            .cache
            .cached_sample_paths
            .contains(&sample_id),
        "normalization should still clear the stale cache-ready row marker"
    );

    let warm_ticket = state
        .waveform
        .cache
        .active_folder_warm_delay_task
        .active()
        .expect("folder warm delay");
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::ActiveFolderCacheWarmReady(warm_ticket),
        &mut context,
    );
    let running_ticket =
        active_folder_cache_warm_ticket(&state).expect("source warm should restart");
    let result = crate::native_app::audio::sample_load_actions::warm_active_folder_waveform_cache(
        crate::native_app::app::ActiveFolderCacheWarmRequest::new(
            source_id.clone(),
            vec![sample_path.clone()],
        ),
        || false,
    );
    crate::native_app::waveform::flush_background_waveform_cache_stores_for_shutdown();
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::ActiveFolderCacheWarmFinished(
            ui::KeyedTaskCompletion {
                key: crate::native_app::audio::sample_load_actions::active_folder_cache_warm_resource_key(
                    source_id.as_str(),
                ),
                ticket: running_ticket,
                output: result,
            },
        ),
        &mut context,
    );

    assert!(
        state.waveform.cache.active_folder_warm_pending.is_empty(),
        "edited file should leave the active-folder cache queue after recache"
    );
    assert_eq!(state.waveform.cache.active_folder_warm_folder_id, None);
    assert!(
        state
            .waveform
            .cache
            .cached_sample_paths
            .contains(&sample_id),
        "recache should restore the cache-ready row marker"
    );
}

#[test]
fn source_warm_marker_does_not_make_summary_only_cache_visually_ready() {
    let config_base = tempfile::tempdir().expect("config base");
    let (_config_lock, _base_guard) =
        set_waveform_test_config_base(config_base.path().to_path_buf());
    let source_root = tempfile::tempdir().expect("source root");
    let sample_path = source_root.path().join("summary-only.wav");
    write_test_wav_i16(&sample_path, &[0, 1024, -2048, 4096, -1024, 512]);
    let sample_path_string = sample_path.display().to_string();
    let sample_path = PathBuf::from(&sample_path_string);

    let file = crate::native_app::waveform::test_waveform_file_from_mono_samples(
        sample_path.clone(),
        fs::read(&sample_path).expect("read wav").into(),
        vec![0.0, 0.25, -0.25, 0.5, -0.5, 0.125],
    );
    crate::native_app::waveform::store_summary_only_cached_waveform_file_for_tests(&file);

    let mut state = gui_state_for_span_tests();
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
        ]);
    state.refresh_persisted_waveform_cache_indicators();

    assert!(
        !state
            .waveform
            .cache
            .cached_sample_paths
            .contains(&sample_path_string),
        "summary-only source-warm markers must not use the playback-ready row marker"
    );
    assert_eq!(
        state.waveform.cache.warm_pending.iter().collect::<Vec<_>>(),
        vec![&sample_path],
        "summary-only caches should be queued to build playback-ready cache data"
    );
}

#[test]
fn summary_only_persisted_cache_selection_uses_loading_pipeline_after_restart() {
    let config_base = tempfile::tempdir().expect("config base");
    let (_config_lock, _base_guard) =
        set_waveform_test_config_base(config_base.path().to_path_buf());
    let source_root = tempfile::tempdir().expect("source root");
    let sample_path = source_root.path().join("summary-only-click.wav");
    write_test_wav_i16(&sample_path, &[0, 1024, -2048, 4096, -1024, 512]);
    let sample_path_string = sample_path.display().to_string();
    let sample_path = PathBuf::from(&sample_path_string);

    let file = crate::native_app::waveform::test_waveform_file_from_mono_samples(
        sample_path.clone(),
        fs::read(&sample_path).expect("read wav").into(),
        vec![0.0, 0.25, -0.25, 0.5, -0.5, 0.125],
    );
    crate::native_app::waveform::store_summary_only_cached_waveform_file_for_tests(&file);

    let mut state = gui_state_for_span_tests();
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
        ]);
    state.refresh_persisted_waveform_cache_indicators();

    let mut context = ui::UiUpdateContext::default();
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::SelectSampleWithModifiers {
            path: sample_path_string.clone(),
            modifiers: Default::default(),
        },
        &mut context,
    );
    run_command_for_tests(&mut state, context.into_command());
    let mut context = ui::UiUpdateContext::default();

    assert!(
        state
            .background
            .deferred_sample_load_task
            .active()
            .is_none(),
        "summary-only cache selection should not debounce or probe cache metadata on the UI thread"
    );
    assert!(
        active_sample_load_ticket(&state).is_some(),
        "summary-only cache selection should queue foreground loading off the UI thread"
    );
    assert_eq!(
        state.waveform.current.path(),
        PathBuf::new(),
        "selection should wait for worker completion instead of hydrating a partial cache on the UI thread"
    );

    let ticket = active_sample_load_ticket(&state).expect("foreground load queued");
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::SampleLoadFinished(
            sample_load_completion(
                ticket,
                sample_path_string,
                crate::native_app::test_support::state::WaveformState::load_persisted_playback_cache(
                    sample_path.clone(),
                ),
                false,
            ),
        ),
        &mut context,
    );

    assert_eq!(state.waveform.current.path(), sample_path);
    assert!(
        state.waveform.current.playback_samples().is_some(),
        "summary-only persisted cache should be upgraded to playback-ready in the foreground worker"
    );
}

#[test]
fn background_warm_upgrades_summary_only_cache_to_playback_ready() {
    let config_base = tempfile::tempdir().expect("config base");
    let (_config_lock, _base_guard) =
        set_waveform_test_config_base(config_base.path().to_path_buf());
    let source_root = tempfile::tempdir().expect("source root");
    let sample_path = source_root.path().join("summary-only-warm.wav");
    write_test_wav_i16(&sample_path, &[0, 1024, -2048, 4096, -1024, 512]);
    let sample_path_string = sample_path.display().to_string();
    let sample_path = PathBuf::from(&sample_path_string);

    let file = crate::native_app::waveform::test_waveform_file_from_mono_samples(
        sample_path.clone(),
        fs::read(&sample_path).expect("read wav").into(),
        vec![0.0, 0.25, -0.25, 0.5, -0.5, 0.125],
    );
    crate::native_app::waveform::store_summary_only_cached_waveform_file_for_tests(&file);

    let result = crate::native_app::audio::sample_load_actions::warm_persisted_waveform_cache(
        vec![sample_path.clone()],
        || false,
    );
    assert_eq!(result.loaded.len(), 1);

    let mut restarted_state = gui_state_for_span_tests();
    restarted_state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
        ]);
    restarted_state.refresh_persisted_waveform_cache_indicators();

    assert!(
        restarted_state
            .waveform
            .cache
            .cached_sample_paths
            .contains(&sample_path_string),
        "background warm should persist playback readiness for future restarts"
    );
}

#[test]
fn normal_sample_load_persists_bright_cache_indicator_before_restart() {
    let config_base = tempfile::tempdir().expect("config base");
    let (_config_lock, _base_guard) =
        set_waveform_test_config_base(config_base.path().to_path_buf());
    let source_root = tempfile::tempdir().expect("source root");
    let sample_path = source_root.path().join("fresh-cache.wav");
    write_test_wav_i16(&sample_path, &[0, 1024, -2048, 4096, -1024, 512]);
    let sample_path = sample_path.display().to_string();

    let _waveform = crate::native_app::test_support::state::WaveformState::load_path(
        sample_path.clone().into(),
    )
    .expect("load sample");

    wait_for_playback_ready_cache(&sample_path);

    let mut restarted_state = gui_state_for_span_tests();
    restarted_state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
        ]);
    restarted_state.refresh_persisted_waveform_cache_indicators();

    assert!(
        restarted_state
            .waveform
            .cache
            .cached_sample_paths
            .contains(&sample_path),
        "freshly loaded cache indicator should survive immediate restart"
    );
}
