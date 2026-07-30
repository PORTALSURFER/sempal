use super::*;
use crate::native_app::app::NormalizationResult;
use std::path::Path;
use wavecrate::sample_sources::capture_source_file_evidence;

#[test]
fn normalize_wav_file_in_place_scales_loaded_sample_peak() {
    let root = std::env::temp_dir().join(format!(
        "wavecrate-default-gui-normalize-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    fs::create_dir_all(&root).expect("create temp root");
    let path = root.join("quiet.wav");
    write_test_wav_i16(&path, &[0, 1024, -2048, 4096]);

    let outcome = crate::native_app::test_support::waveform::normalize_wav_file_in_place(&path)
        .expect("normalize wav");
    assert_eq!(
        outcome,
        crate::native_app::test_support::waveform::WavNormalizationOutcome::Normalized
    );

    let spec = hound::WavReader::open(&path)
        .expect("open normalized wav")
        .spec();
    assert_eq!(spec.bits_per_sample, 16);
    assert_eq!(spec.sample_format, hound::SampleFormat::Int);
    let samples = read_test_wav_f32(&path);
    let peak = samples
        .iter()
        .copied()
        .map(f32::abs)
        .fold(0.0_f32, f32::max);
    assert!((peak - 1.0).abs() < 0.000_1, "peak was {peak}");
    assert!(samples.iter().all(|sample| sample.is_finite()));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn normalize_wav_file_in_place_cleans_work_files_after_success() {
    let root = std::env::temp_dir().join(format!(
        "wavecrate-default-gui-normalize-cleanup-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    fs::create_dir_all(&root).expect("create temp root");
    let path = root.join("sample.wav");
    write_test_wav_i16(&path, &[0, 2048, -4096, 8192]);

    let outcome = crate::native_app::test_support::waveform::normalize_wav_file_in_place(&path)
        .expect("normalize wav");
    assert_eq!(
        outcome,
        crate::native_app::test_support::waveform::WavNormalizationOutcome::Normalized
    );

    let work_files: Vec<_> = fs::read_dir(&root)
        .expect("read temp root")
        .map(|entry| entry.expect("read temp entry").file_name())
        .filter(|file_name| file_name.to_string_lossy().contains(".wavecrate-normalize"))
        .collect();
    assert!(
        work_files.is_empty(),
        "normalization should clean temporary and backup files, found {work_files:?}"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn normalize_wav_file_in_place_reports_realtime_progress_phases() {
    let root = std::env::temp_dir().join(format!(
        "wavecrate-default-gui-normalize-progress-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    fs::create_dir_all(&root).expect("create temp root");
    let path = root.join("progress.wav");
    let samples: Vec<i16> = (0..40_000)
        .map(|index| if index % 2 == 0 { 2_048 } else { -4_096 })
        .collect();
    write_test_wav_i16(&path, &samples);

    let mut snapshots = Vec::new();
    let outcome =
        crate::native_app::test_support::waveform::normalize_wav_file_in_place_with_progress(
            &path,
            |fraction, phase| snapshots.push((fraction, phase.to_string())),
        )
        .expect("normalize wav");

    assert_eq!(
        outcome,
        crate::native_app::test_support::waveform::WavNormalizationOutcome::Normalized
    );
    assert!(
        snapshots.iter().any(|(_, phase)| phase == "Analyzing"),
        "expected analyze progress updates"
    );
    assert!(
        snapshots.iter().any(|(_, phase)| phase == "Writing"),
        "expected write progress updates"
    );
    assert!(
        snapshots
            .iter()
            .any(|(fraction, _)| *fraction > 0.0 && *fraction < 1.0),
        "expected intermediate progress fractions"
    );
    assert_eq!(
        snapshots.last().expect("final progress snapshot"),
        &(1.0, String::from("Done"))
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn normalize_wav_file_in_place_reports_invalid_wav_without_rewrite() {
    let root = std::env::temp_dir().join(format!(
        "wavecrate-default-gui-normalize-invalid-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    fs::create_dir_all(&root).expect("create temp root");
    let path = root.join("truncated.wav");
    fs::write(&path, b"RIFF").expect("write truncated wav");

    let error = crate::native_app::test_support::waveform::normalize_wav_file_in_place(&path)
        .expect_err("truncated wav should fail");

    assert!(
        error.starts_with("Invalid WAV:"),
        "expected invalid WAV error, got {error}"
    );
    assert_eq!(
        fs::read(&path).expect("read truncated wav after failure"),
        b"RIFF",
        "failed normalization must not rewrite invalid source files"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn normalize_selected_samples_queues_worker_without_rewriting_on_ui_thread() {
    let (mut state, _source_root, selected_file) = native_app_state_with_temp_sample("quiet.wav");
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1024, -2048, 4096]);
    let before = fs::read(&path).expect("read wav before normalization");

    let mut context = ui::UiUpdateContext::default();
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::NormalizeSelectedSamples,
        &mut context,
    );

    assert_eq!(
        fs::read(&path).expect("read wav after queue"),
        before,
        "normalization must not rewrite the selected sample on the UI thread"
    );
    let progress = state
        .background
        .normalization_progress
        .as_ref()
        .expect("normalization progress should be visible after queueing");
    assert_eq!(progress.completed, 0);
    assert_eq!(progress.total, 1);
    assert_eq!(progress.work_completed, 0);
    assert_eq!(progress.work_total, 1_000);
    assert_eq!(progress.queued, 0);
    assert_eq!(progress.detail, "Queued");
    assert!(state.ui.status.sample.contains("Normalizing 1 sample"));
}

#[test]
fn protected_normalize_copies_to_primary_and_records_harvest_derivation() {
    let config_base = tempfile::tempdir().expect("config base");
    let _base_guard = wavecrate::app_dirs::ConfigBaseGuard::set(config_base.path().to_path_buf());
    let (mut state, source_root, selected_file) =
        native_app_state_with_temp_sample("protected-normalize.wav");
    let primary_root = tempfile::tempdir().expect("primary source root");
    let protected_source =
        wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()).protected();
    let primary_source =
        wavecrate::sample_sources::SampleSource::new(primary_root.path().to_path_buf()).primary();
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            protected_source.clone(),
            primary_source.clone(),
        ]);
    state
        .library
        .folder_browser
        .select_file(selected_file.clone());
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1024, -2048, 4096]);
    let before = fs::read(&path).expect("read protected source before normalize");
    let harvest_source_folder = source_root
        .path()
        .file_name()
        .expect("source root folder name");
    let normalized_copy = primary_root
        .path()
        .join("_Harvests")
        .join(harvest_source_folder)
        .join("protected-normalize_normalized.wav");
    let mut context = ui::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::NormalizeSelectedSamples,
        &mut context,
    );
    run_command_for_tests(&mut state, context.into_command());

    assert_eq!(
        fs::read(&path).expect("read protected source after normalize"),
        before,
        "protected source must not be normalized in place"
    );
    assert!(normalized_copy.is_file());
    let peak = read_test_wav_f32(&normalized_copy)
        .into_iter()
        .map(f32::abs)
        .fold(0.0_f32, f32::max);
    assert!(peak > 0.9, "normalized copy peak should be near full scale");
    let parent_key = wavecrate::sample_sources::HarvestFileKey::new(
        protected_source.id.clone(),
        PathBuf::from("protected-normalize.wav"),
    );
    let parent = wavecrate::sample_sources::library::harvest_file(&parent_key)
        .expect("load harvest parent")
        .expect("harvest parent");
    assert_eq!(
        parent.state,
        wavecrate::sample_sources::HarvestState::Touched
    );
    let edges = wavecrate::sample_sources::library::harvest_derivations_for_parent(&parent_key)
        .expect("load harvest derivations");
    assert_eq!(edges.len(), 1);
    assert_eq!(
        edges[0].operation,
        wavecrate::sample_sources::HarvestDerivationOperation::NormalizeCopy
    );
    assert_eq!(edges[0].child.key.source_id, primary_source.id);
    assert_eq!(
        edges[0].child.key.relative_path,
        PathBuf::from("_Harvests")
            .join(harvest_source_folder)
            .join("protected-normalize_normalized.wav")
    );
}

#[test]
fn normalize_selected_samples_does_not_enqueue_duplicate_active_file() {
    let (mut state, _source_root, selected_file) = native_app_state_with_temp_sample("queued.wav");
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1024, -2048, 4096]);

    let mut context = ui::UiUpdateContext::default();
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::NormalizeSelectedSamples,
        &mut context,
    );
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::NormalizeSelectedSamples,
        &mut context,
    );

    assert!(state.background.normalization_queue.is_empty());
    let progress = state
        .background
        .normalization_progress
        .as_ref()
        .expect("normalization progress should remain active");
    assert_eq!(progress.queued, 0);
    assert!(
        state
            .ui
            .status
            .sample
            .contains("already queued for selection")
    );
}

#[test]
fn normalize_selected_samples_uses_interactive_worker_priority() {
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("priority.wav");
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1024, -2048, 4096]);

    let mut context = ui::UiUpdateContext::default();
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::NormalizeSelectedSamples,
        &mut context,
    );

    assert_eq!(
        context
            .into_command()
            .business_task_priority("gui-normalize-selected-samples"),
        Some(ui::TaskPriority::Interactive),
        "normalization must not wait behind low-priority cache warming"
    );
}

#[test]
fn normalize_many_selected_samples_uses_background_worker_priority() {
    let source_root = tempfile::tempdir().expect("source root");
    for index in 0..33 {
        let path = source_root.path().join(format!("bulk-{index:02}.wav"));
        write_test_wav_i16(&path, &[0, 1024, -2048, 4096]);
    }

    let mut state = gui_state_for_span_tests();
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
        ]);
    assert_eq!(state.library.folder_browser.select_all_audio_files(), 33);

    let mut context = ui::UiUpdateContext::default();
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::NormalizeSelectedSamples,
        &mut context,
    );

    let command = context.into_command();
    assert_eq!(
        command.business_task_priority("gui-normalize-selected-samples"),
        Some(ui::TaskPriority::Background),
        "large normalization batches should run on the background lane"
    );
    assert!(
        state.ui.status.sample.contains("in background"),
        "bulk normalization should make the background behavior visible in status"
    );
    assert_eq!(
        state
            .background
            .normalization_progress
            .as_ref()
            .expect("bulk normalization should report progress")
            .total,
        33
    );
}

#[test]
fn uncached_sample_load_waits_for_active_normalization() {
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("wait-load.wav");
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1024, -2048, 4096]);
    state.background.normalization_progress = Some(
        crate::native_app::test_support::state::NormalizationProgress {
            task_id: 77,
            label: String::from("1 sample"),
            completed: 0,
            total: 1,
            work_completed: 250,
            work_total: 1_000,
            queued: 0,
            detail: String::from("normalizing.wav"),
        },
    );
    state
        .background
        .normalization_active_paths
        .insert(path.clone());

    let mut context = ui::UiUpdateContext::default();
    state.load_sample(selected_file.clone(), &mut context);
    run_command_for_tests(&mut state, context.into_command());
    let mut context = ui::UiUpdateContext::default();

    assert!(
        active_sample_load_ticket(&state).is_none(),
        "uncached foreground decode should not start while normalization is active"
    );
    let retry_ticket = state
        .background
        .deferred_sample_load_task
        .active()
        .expect("sample load should be deferred until normalization is idle");
    assert!(state.ui.status.sample.contains("waiting for normalization"));

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::DeferredSampleLoad {
            ticket: retry_ticket,
            path: selected_file.clone(),
            autoplay: true,
            check_cache: false,
            scheduled_at: std::time::Instant::now(),
        },
        &mut context,
    );
    assert!(
        active_sample_load_ticket(&state).is_none(),
        "deferred retry should keep waiting while normalization is still active"
    );
    let ready_ticket = state
        .background
        .deferred_sample_load_task
        .active()
        .expect("sample load retry should be scheduled again");

    state.background.normalization_progress = None;
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::DeferredSampleLoad {
            ticket: ready_ticket,
            path: selected_file,
            autoplay: true,
            check_cache: false,
            scheduled_at: std::time::Instant::now(),
        },
        &mut context,
    );

    assert!(
        active_sample_load_ticket(&state).is_some(),
        "deferred sample load should start once normalization is idle"
    );
}

#[test]
fn uncached_sample_load_does_not_wait_for_unrelated_active_normalization() {
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("unrelated-load.wav");
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1024, -2048, 4096]);
    state.background.normalization_progress = Some(
        crate::native_app::test_support::state::NormalizationProgress {
            task_id: 88,
            label: String::from("1 sample"),
            completed: 0,
            total: 1,
            work_completed: 250,
            work_total: 1_000,
            queued: 0,
            detail: String::from("other.wav"),
        },
    );
    state
        .background
        .normalization_active_paths
        .insert(path.with_file_name("other.wav"));

    let mut context = ui::UiUpdateContext::default();
    state.load_sample(selected_file, &mut context);
    run_command_for_tests(&mut state, context.into_command());

    assert!(
        active_sample_load_ticket(&state).is_some(),
        "unrelated foreground sample loads should not be delayed by normalization"
    );
    assert!(
        state
            .background
            .deferred_sample_load_task
            .active()
            .is_none(),
        "unrelated sample loads should not enter the normalization retry loop"
    );
}

#[test]
fn normalize_wav_file_in_place_skips_already_normalized_wav() {
    let root = std::env::temp_dir().join(format!(
        "wavecrate-default-gui-normalize-skip-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    fs::create_dir_all(&root).expect("create temp root");
    let path = root.join("full-scale.wav");
    write_test_wav_i16(&path, &[0, 32767, -32767, 1234]);
    let before = fs::read(&path).expect("read wav before normalization");

    let outcome = crate::native_app::test_support::waveform::normalize_wav_file_in_place(&path)
        .expect("normalize wav");

    assert_eq!(
        outcome,
        crate::native_app::test_support::waveform::WavNormalizationOutcome::Skipped
    );
    assert_eq!(
        fs::read(&path).expect("read wav after normalization"),
        before,
        "already normalized wavs should not be rewritten"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn normalize_finish_evicts_stale_memory_cache_before_reselect() {
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("normalize-reselect.wav");
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1024, -2048, 4096]);

    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("sample loads");
    let stale = state.waveform.current.clone();
    state.remember_waveform(&stale);
    assert!(state.waveform.cache.entries.contains_key(&path));

    crate::native_app::test_support::waveform::normalize_wav_file_in_place(&path)
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
            detail: selected_file.clone(),
        },
    );
    let source_id = state
        .library
        .folder_browser
        .selected_source_id()
        .to_string();
    let mut context = ui::UiUpdateContext::default();
    state.finish_normalization(
        NormalizationResult {
            task_id: 42,
            source_id,
            loaded_path: path.clone(),
            normalizing_loaded: true,
            was_playing: false,
            restart_ratio: 0.0,
            restart_span: None,
            normalized: vec![path.clone()],
            normalized_evidence: vec![(path.clone(), capture_source_file_evidence(&path))],
            refreshed_files: Vec::new(),
            skipped: Vec::new(),
            failed: Vec::new(),
            harvest_derivations: Vec::new(),
        },
        &mut context,
    );

    assert!(
        !state.waveform.cache.entries.contains_key(&path),
        "normalization must evict the pre-edit memory cache entry before reselect can use it"
    );
    assert!(
        !state
            .waveform
            .cache
            .cached_sample_paths
            .contains(&selected_file),
        "the browser loaded marker should not advertise a stale memory cache entry"
    );
    assert!(
        active_sample_load_ticket(&state).is_some(),
        "normalization should reload the edited sample through the background load worker"
    );

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::SelectSampleWithModifiers {
            path: selected_file,
            modifiers: Default::default(),
        },
        &mut context,
    );

    assert!(
        state
            .background
            .deferred_sample_load_task
            .active()
            .is_none(),
        "direct reselect should not use the deferred navigation load path"
    );
}

#[test]
fn normalize_finish_marks_normalized_samples_harvest_touched() {
    let config_base = tempfile::tempdir().expect("config base");
    let _base_guard = wavecrate::app_dirs::ConfigBaseGuard::set(config_base.path().to_path_buf());
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("normalize-harvest.wav");
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1024, -2048, 4096]);
    state.background.normalization_progress = Some(
        crate::native_app::test_support::state::NormalizationProgress {
            task_id: 42,
            label: String::from("1 sample"),
            completed: 1,
            total: 1,
            work_completed: 1_000,
            work_total: 1_000,
            queued: 0,
            detail: selected_file.clone(),
        },
    );
    let source_id = state
        .library
        .folder_browser
        .selected_source_id()
        .to_string();
    let (source, relative_path) = state
        .library
        .folder_browser
        .sample_source_for_file_path(&path)
        .expect("sample should belong to the active source");
    let harvest_key = wavecrate::sample_sources::HarvestFileKey::new(
        wavecrate::sample_sources::SourceId::from_string(source.id.as_str().to_owned()),
        relative_path,
    );
    let mut context = ui::UiUpdateContext::default();

    state.finish_normalization(
        NormalizationResult {
            task_id: 42,
            source_id,
            loaded_path: path,
            normalizing_loaded: false,
            was_playing: false,
            restart_ratio: 0.0,
            restart_span: None,
            normalized: vec![PathBuf::from(&selected_file)],
            normalized_evidence: vec![(
                PathBuf::from(&selected_file),
                capture_source_file_evidence(Path::new(&selected_file)),
            )],
            refreshed_files: Vec::new(),
            skipped: Vec::new(),
            failed: Vec::new(),
            harvest_derivations: Vec::new(),
        },
        &mut context,
    );

    let harvest_parent = wavecrate::sample_sources::library::harvest_file(&harvest_key)
        .expect("load harvest source")
        .expect("harvest parent");
    assert_eq!(
        harvest_parent.state,
        wavecrate::sample_sources::HarvestState::Touched
    );
}

#[test]
fn bulk_waveform_cache_eviction_removes_pending_and_markers() {
    let mut state = gui_state_for_span_tests();
    let paths = (0..8)
        .map(|index| PathBuf::from(format!("/tmp/wavecrate-normalize-bulk-{index}.wav")))
        .collect::<Vec<_>>();
    let keep = PathBuf::from("/tmp/wavecrate-normalize-keep.wav");

    for path in &paths {
        state.waveform.cache.order.push_back(path.clone());
        state.waveform.cache.warm_pending.push_back(path.clone());
        state
            .waveform
            .cache
            .cached_sample_paths
            .insert(path.display().to_string());
    }
    state.waveform.cache.order.push_back(keep.clone());
    state.waveform.cache.warm_pending.push_back(keep.clone());
    state
        .waveform
        .cache
        .cached_sample_paths
        .insert(keep.display().to_string());

    state.evict_waveform_cache_paths(&paths);

    assert_eq!(
        state.waveform.cache.order.iter().collect::<Vec<_>>(),
        vec![&keep]
    );
    assert_eq!(
        state.waveform.cache.warm_pending.iter().collect::<Vec<_>>(),
        vec![&keep]
    );
    assert_eq!(
        state
            .waveform
            .cache
            .cached_sample_paths
            .iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec![keep.display().to_string()]
    );
}

#[test]
fn normalize_finish_reloads_selected_sample_even_when_another_waveform_was_loaded() {
    let (mut state, source_root, selected_file) =
        native_app_state_with_temp_sample("normalize-source.wav");
    let loaded_path = PathBuf::from(&selected_file);
    let extracted_path = source_root.path().join("normalize-source_extraction.wav");
    write_test_wav_i16(&loaded_path, &[0, 1024, -2048, 4096]);
    write_test_wav_i16(&extracted_path, &[0, 512, -512, 256]);
    state
        .library
        .folder_browser
        .refresh_file_path(&extracted_path);
    let extracted_id = extracted_path.display().to_string();
    state
        .library
        .folder_browser
        .select_file(extracted_id.clone());
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(loaded_path.clone())
            .expect("source sample loads");
    crate::native_app::test_support::waveform::normalize_wav_file_in_place(&extracted_path)
        .expect("normalize selected extraction");
    state.background.normalization_progress = Some(
        crate::native_app::test_support::state::NormalizationProgress {
            task_id: 42,
            label: String::from("1 sample"),
            completed: 1,
            total: 1,
            work_completed: 1_000,
            work_total: 1_000,
            queued: 0,
            detail: extracted_id.clone(),
        },
    );
    let source_id = state
        .library
        .folder_browser
        .selected_source_id()
        .to_string();
    let mut context = ui::UiUpdateContext::default();

    state.finish_normalization(
        NormalizationResult {
            task_id: 42,
            source_id,
            loaded_path,
            normalizing_loaded: false,
            was_playing: false,
            restart_ratio: 0.0,
            restart_span: None,
            normalized: vec![extracted_path.clone()],
            normalized_evidence: vec![(
                extracted_path.clone(),
                capture_source_file_evidence(&extracted_path),
            )],
            refreshed_files: Vec::new(),
            skipped: Vec::new(),
            failed: Vec::new(),
            harvest_derivations: Vec::new(),
        },
        &mut context,
    );

    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(extracted_id.as_str())
    );
    assert!(
        active_sample_load_ticket(&state).is_some(),
        "normalizing a selected extraction should reload that extraction, not keep the previously loaded source audible"
    );
}

#[test]
fn normalize_finish_reloads_current_sample_without_waiting_on_queued_normalization() {
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("normalize-queue-reload.wav");
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1024, -2048, 4096]);

    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("sample loads");
    crate::native_app::test_support::waveform::normalize_wav_file_in_place(&path)
        .expect("normalize wav");
    state.background.normalization_progress = Some(
        crate::native_app::test_support::state::NormalizationProgress {
            task_id: 42,
            label: String::from("1 sample"),
            completed: 1,
            total: 1,
            work_completed: 1_000,
            work_total: 1_000,
            queued: 1,
            detail: selected_file.clone(),
        },
    );
    let source_id = state
        .library
        .folder_browser
        .selected_source_id()
        .to_string();
    let (source_root, source_database_root) = state
        .library
        .folder_browser
        .source_roots(&source_id)
        .expect("source roots");
    state.background.normalization_queue.push_back(
        crate::native_app::app::NormalizationQueueItem {
            paths: vec![path.clone()],
            source_id: source_id.clone(),
            source_root,
            source_database_root,
            harvest_derivations: Vec::new(),
        },
    );

    let mut context = ui::UiUpdateContext::default();
    state.finish_normalization(
        NormalizationResult {
            task_id: 42,
            source_id,
            loaded_path: path,
            normalizing_loaded: true,
            was_playing: false,
            restart_ratio: 0.0,
            restart_span: None,
            normalized: vec![PathBuf::from(&selected_file)],
            normalized_evidence: vec![(
                PathBuf::from(&selected_file),
                capture_source_file_evidence(Path::new(&selected_file)),
            )],
            refreshed_files: Vec::new(),
            skipped: Vec::new(),
            failed: Vec::new(),
            harvest_derivations: Vec::new(),
        },
        &mut context,
    );

    assert!(
        active_sample_load_ticket(&state).is_some(),
        "the normalized current sample should reload immediately even when another normalization task is queued"
    );
    assert!(
        state
            .background
            .deferred_sample_load_task
            .active()
            .is_none(),
        "normalization reload must not enter the retry loop while queued normalization work exists"
    );
    assert!(
        state.background.normalization_progress.is_some(),
        "the queued normalization task should still start after scheduling the reload"
    );
}

#[test]
fn normalize_finish_resumes_loaded_playback_when_current_sample_is_skipped() {
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("normalize-skip-resume.wav");
    if !install_playback_runtime_for_tests(&mut state) {
        return;
    }
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 32767, -32767, 1024]);
    state
        .library
        .folder_browser
        .select_file(selected_file.clone());
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("sample loads");
    state
        .start_playback_current_span(0.20, 0.80)
        .expect("sample playback starts");
    state.stop_audio_output_playback();
    state.waveform.current.stop_playback();
    state.audio.current_playback_span = None;
    state.background.normalization_progress = Some(
        crate::native_app::test_support::state::NormalizationProgress {
            task_id: 42,
            label: String::from("1 sample"),
            completed: 1,
            total: 1,
            work_completed: 1_000,
            work_total: 1_000,
            queued: 0,
            detail: selected_file.clone(),
        },
    );

    let source_id = state
        .library
        .folder_browser
        .selected_source_id()
        .to_string();
    let mut context = ui::UiUpdateContext::default();
    state.finish_normalization(
        NormalizationResult {
            task_id: 42,
            source_id,
            loaded_path: path.clone(),
            normalizing_loaded: true,
            was_playing: true,
            restart_ratio: 0.35,
            restart_span: Some((0.20, 0.80)),
            normalized: Vec::new(),
            normalized_evidence: Vec::new(),
            refreshed_files: Vec::new(),
            skipped: vec![path],
            failed: Vec::new(),
            harvest_derivations: Vec::new(),
        },
        &mut context,
    );

    assert!(
        state.waveform.current.is_playing(),
        "skipped normalization should resume previously playing loaded sample"
    );
    assert_eq!(state.audio.current_playback_span, Some((0.35, 0.80)));
    assert!(
        active_sample_load_ticket(&state).is_none(),
        "skipped normalization should reuse the loaded waveform instead of reloading"
    );
}

#[test]
fn normalize_finish_resumes_loaded_playback_when_current_sample_fails_before_write() {
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("normalize-failed-resume.wav");
    if !install_playback_runtime_for_tests(&mut state) {
        return;
    }
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1024, -1024, 0]);
    state
        .library
        .folder_browser
        .select_file(selected_file.clone());
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("sample loads");
    state
        .start_playback_current_span(0.10, 0.90)
        .expect("sample playback starts");
    state.stop_audio_output_playback();
    state.waveform.current.stop_playback();
    state.audio.current_playback_span = None;
    state.background.normalization_progress = Some(
        crate::native_app::test_support::state::NormalizationProgress {
            task_id: 42,
            label: String::from("1 sample"),
            completed: 1,
            total: 1,
            work_completed: 1_000,
            work_total: 1_000,
            queued: 0,
            detail: selected_file.clone(),
        },
    );

    let source_id = state
        .library
        .folder_browser
        .selected_source_id()
        .to_string();
    let mut context = ui::UiUpdateContext::default();
    state.finish_normalization(
        NormalizationResult {
            task_id: 42,
            source_id,
            loaded_path: path.clone(),
            normalizing_loaded: true,
            was_playing: true,
            restart_ratio: 0.30,
            restart_span: Some((0.10, 0.90)),
            normalized: Vec::new(),
            normalized_evidence: Vec::new(),
            refreshed_files: Vec::new(),
            skipped: Vec::new(),
            failed: vec![crate::native_app::app::NormalizationFailure {
                path,
                error: String::from("Invalid WAV: Failed to read enough bytes."),
            }],
            harvest_derivations: Vec::new(),
        },
        &mut context,
    );

    assert!(
        state.waveform.current.is_playing(),
        "failed normalization should resume previously playing loaded sample when the file was not rewritten"
    );
    assert_eq!(state.audio.current_playback_span, Some((0.30, 0.90)));
    assert_eq!(
        state.ui.status.sample,
        "Could not normalize normalize-failed-resume.wav | Invalid WAV: Failed to read enough bytes."
    );
}

#[test]
fn normalize_finish_reports_failed_file_without_success_count() {
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("normalize-failed.wav");
    let path = PathBuf::from(&selected_file);
    state.background.normalization_progress = Some(
        crate::native_app::test_support::state::NormalizationProgress {
            task_id: 42,
            label: String::from("1 sample"),
            completed: 1,
            total: 1,
            work_completed: 1_000,
            work_total: 1_000,
            queued: 0,
            detail: selected_file.clone(),
        },
    );

    let source_id = state
        .library
        .folder_browser
        .selected_source_id()
        .to_string();
    let mut context = ui::UiUpdateContext::default();
    state.finish_normalization(
        NormalizationResult {
            task_id: 42,
            source_id,
            loaded_path: path.clone(),
            normalizing_loaded: true,
            was_playing: false,
            restart_ratio: 0.0,
            restart_span: None,
            normalized: Vec::new(),
            normalized_evidence: Vec::new(),
            refreshed_files: Vec::new(),
            skipped: Vec::new(),
            failed: vec![crate::native_app::app::NormalizationFailure {
                path,
                error: String::from("Invalid WAV: Failed to read enough bytes."),
            }],
            harvest_derivations: Vec::new(),
        },
        &mut context,
    );

    assert_eq!(
        state.ui.status.sample,
        "Could not normalize normalize-failed.wav | Invalid WAV: Failed to read enough bytes."
    );
    assert!(state.background.normalization_progress.is_none());
}
