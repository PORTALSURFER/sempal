use super::*;
use crate::native_app::test_support::state::GuiMessage;

#[test]
fn crop_shortcut_routes_to_waveform_crop_request() {
    let state = crate::native_app::test_support::state::NativeAppStateFixture::default().build();
    let resolution = crate::native_app::test_support::state::default_gui_shortcuts(&state)
        .resolve(ui::KeyPress::new(ui::KeyCode::C));

    assert_eq!(
        resolution.action,
        Some(GuiMessage::RequestCropWaveformSelection)
    );
    assert!(resolution.handled);
}

#[test]
fn trim_shortcut_routes_to_waveform_trim_request() {
    let state = crate::native_app::test_support::state::NativeAppStateFixture::default().build();
    let resolution = crate::native_app::test_support::state::default_gui_shortcuts(&state)
        .resolve(ui::KeyPress::new(ui::KeyCode::D));

    assert_eq!(
        resolution.action,
        Some(GuiMessage::RequestTrimWaveformSelection)
    );
    assert!(resolution.handled);
}

#[test]
fn reverse_shortcut_routes_to_waveform_reverse_request() {
    let state = crate::native_app::test_support::state::NativeAppStateFixture::default().build();
    let resolution = crate::native_app::test_support::state::default_gui_shortcuts(&state)
        .resolve(ui::KeyPress::new(ui::KeyCode::R));

    assert_eq!(
        resolution.action,
        Some(GuiMessage::RequestReverseWaveformSelection)
    );
    assert!(resolution.handled);
}

#[test]
fn mute_shortcut_routes_to_waveform_mute_request() {
    let state = crate::native_app::test_support::state::NativeAppStateFixture::default().build();
    let resolution = crate::native_app::test_support::state::default_gui_shortcuts(&state)
        .resolve(ui::KeyPress::new(ui::KeyCode::M));

    assert_eq!(
        resolution.action,
        Some(GuiMessage::RequestMuteWaveformSelection)
    );
    assert!(resolution.handled);
}

#[test]
fn mute_shortcut_is_consumed_while_renaming() {
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("mute-rename.wav");
    state.library.folder_browser.select_file(selected_file);
    state
        .library
        .folder_browser
        .begin_rename_selected()
        .expect("begin rename should not fail");

    let resolution = crate::native_app::test_support::state::default_gui_shortcuts(&state)
        .resolve(ui::KeyPress::new(ui::KeyCode::M));

    assert_eq!(resolution.action, None);
    assert!(resolution.handled);
}

#[test]
fn command_extract_shortcut_routes_to_extract_and_trim_request() {
    let state = crate::native_app::test_support::state::NativeAppStateFixture::default().build();
    let resolution = crate::native_app::test_support::state::default_gui_shortcuts(&state)
        .resolve(ui::KeyPress::with_command(ui::KeyCode::E));

    assert_eq!(
        resolution.action,
        Some(GuiMessage::RequestExtractAndTrimWaveformSelection)
    );
    assert!(resolution.handled);
}

#[test]
fn enter_shortcut_routes_to_apply_edit_selection_effects_request() {
    let state = crate::native_app::test_support::state::NativeAppStateFixture::default().build();
    let resolution = crate::native_app::test_support::state::default_gui_shortcuts(&state)
        .resolve(ui::KeyPress::new(ui::KeyCode::Enter));

    assert_eq!(
        resolution.action,
        Some(GuiMessage::RequestApplyEditSelectionEffects)
    );
    assert!(resolution.handled);
}

#[test]
fn enter_confirms_pending_destructive_edit_modal() {
    let (mut state, _source_root, selected_file) = native_app_state_with_temp_sample("confirm.wav");
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000]);
    state.waveform.current = crate::native_app::test_support::state::WaveformState::load_path(path)
        .expect("load waveform");
    state.ui.settings.persisted.controls.destructive_yolo_mode = false;
    select_waveform_range(&mut state, WaveformSelectionKind::Play, 0.25, 0.5);
    state.apply_message(
        GuiMessage::RequestCropWaveformSelection,
        &mut ui::UiUpdateContext::default(),
    );

    let resolution = crate::native_app::test_support::state::default_gui_shortcuts(&state)
        .resolve(ui::KeyPress::new(ui::KeyCode::Enter));

    assert_eq!(
        resolution.action,
        Some(GuiMessage::ConfirmPendingWaveformDestructiveEdit)
    );
    assert!(resolution.handled);
}

#[test]
fn escape_cancels_protected_extraction_target_source_modal() {
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_synthetic_waveform()
        .build();
    let prompt = crate::native_app::app::PendingProtectedExtractionTargetSource {
        action: crate::native_app::app::PendingProtectedExtractionAction::ExtractPlaymarkedRange,
        title: String::from("Target source required"),
        message: String::from("Add a writable target source."),
    };
    state
        .ui
        .browser_interaction
        .pending_protected_extraction_target_source = Some(prompt);

    let resolution = crate::native_app::test_support::state::default_gui_shortcuts(&state)
        .resolve(ui::KeyPress::new(ui::KeyCode::Escape));

    assert_eq!(
        resolution.action,
        Some(GuiMessage::CancelProtectedExtractionTargetSource)
    );
    assert!(resolution.handled);
}

#[test]
fn crop_request_uses_play_selection_when_no_edit_selection_exists() {
    let (mut state, _source_root, selected_file) = native_app_state_with_temp_sample("crop.wav");
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000, 4_000, 5_000, 6_000, 7_000]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("load waveform");
    state.ui.settings.persisted.controls.destructive_yolo_mode = false;

    select_waveform_range(&mut state, WaveformSelectionKind::Play, 0.25, 0.5);

    state.apply_message(
        GuiMessage::RequestCropWaveformSelection,
        &mut ui::UiUpdateContext::default(),
    );

    let pending = state
        .ui
        .browser_interaction
        .pending_waveform_destructive_edit
        .as_ref()
        .expect("crop request should prompt");
    assert_eq!(
        pending.prompt.edit,
        crate::native_app::app::WaveformDestructiveEditKind::CropSelection
    );
    assert!((pending.selection.start() - 0.25).abs() < 0.001);
    assert!((pending.selection.end() - 0.5).abs() < 0.001);
}

#[test]
fn destructive_edit_request_blocks_locked_folder() {
    let (mut state, source_root, selected_file) =
        native_app_state_with_temp_sample("locked-crop.wav");
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000]);
    state.waveform.current = crate::native_app::test_support::state::WaveformState::load_path(path)
        .expect("load waveform");
    state.ui.settings.persisted.controls.destructive_yolo_mode = false;
    let source_root_id = source_root.path().to_string_lossy();
    state
        .library
        .folder_browser
        .toggle_folder_lock(source_root_id.as_ref())
        .expect("lock source root");
    select_waveform_range(&mut state, WaveformSelectionKind::Play, 0.25, 0.5);

    state.apply_message(
        GuiMessage::RequestCropWaveformSelection,
        &mut ui::UiUpdateContext::default(),
    );

    assert!(
        state
            .ui
            .browser_interaction
            .pending_waveform_destructive_edit
            .is_none()
    );
    assert!(state.ui.status.sample.contains("blocked by locked folder"));
}

#[test]
fn protected_extract_and_trim_extracts_to_primary_without_mutating_origin() {
    let config_base = tempfile::tempdir().expect("config base");
    let _base_guard = wavecrate::app_dirs::ConfigBaseGuard::set(config_base.path().to_path_buf());
    let (mut state, source_root, selected_file) =
        native_app_state_with_temp_sample("protected-extract-trim.wav");
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
    let harvest_source_folder = source_root
        .path()
        .file_name()
        .expect("source root folder name");
    let extracted = primary_root
        .path()
        .join("_Harvests")
        .join(harvest_source_folder)
        .join("protected-extract-trim_extraction.wav");
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("load waveform");
    state.ui.settings.persisted.controls.destructive_yolo_mode = false;
    state.audio.loop_playback = true;
    select_waveform_range(&mut state, WaveformSelectionKind::Play, 0.25, 0.5);
    let mut context = ui::UiUpdateContext::default();

    state.apply_message(
        GuiMessage::RequestExtractAndTrimWaveformSelection,
        &mut context,
    );
    run_command_for_tests(&mut state, context.into_command());

    assert!(
        state
            .ui
            .browser_interaction
            .pending_waveform_destructive_edit
            .is_none()
    );
    assert_eq!(
        read_test_wav_f32(&path).len(),
        4,
        "protected origin should not be trimmed"
    );
    assert_samples_close(&read_test_wav_f32(&path), &[0.0, 1_000.0, 2_000.0, 3_000.0]);
    assert_samples_close(&read_test_wav_f32(&extracted), &[1_000.0]);
    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(extracted.to_string_lossy().as_ref())
    );
    assert_eq!(
        state
            .metadata
            .tags_by_file
            .get(&extracted.to_string_lossy().to_string()),
        Some(&vec![String::from("loop")])
    );
    assert_extracted_file_keep_1_rating(&state, &extracted);
    let parent_key = wavecrate::sample_sources::HarvestFileKey::new(
        protected_source.id.clone(),
        PathBuf::from("protected-extract-trim.wav"),
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
        wavecrate::sample_sources::HarvestDerivationOperation::Extract
    );
    assert_eq!(edges[0].child.key.source_id, primary_source.id);
    assert_eq!(
        edges[0].child.key.relative_path,
        PathBuf::from("_Harvests")
            .join(harvest_source_folder)
            .join("protected-extract-trim_extraction.wav")
    );
}

#[test]
fn protected_extract_without_writable_target_opens_target_source_prompt() {
    let (mut state, source_root, selected_file) =
        native_app_state_with_temp_sample("protected-extract-needs-target.wav");
    let protected_source =
        wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()).protected();
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            protected_source.clone(),
        ]);
    state
        .library
        .folder_browser
        .select_file(selected_file.clone());
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000]);
    state.waveform.current = crate::native_app::test_support::state::WaveformState::load_path(path)
        .expect("load waveform");
    select_waveform_range(&mut state, WaveformSelectionKind::Play, 0.25, 0.5);

    state.apply_message(
        GuiMessage::RequestExtractAndTrimWaveformSelection,
        &mut ui::UiUpdateContext::default(),
    );

    let pending = state
        .ui
        .browser_interaction
        .pending_protected_extraction_target_source
        .as_ref()
        .expect("protected extraction should prompt for a writable target source");
    assert_eq!(
        pending.action,
        crate::native_app::app::PendingProtectedExtractionAction::WaveformDestructiveEdit {
            kind: crate::native_app::app::WaveformDestructiveEditKind::ExtractAndTrimSelection,
            target: crate::native_app::app::WaveformDestructiveEditTarget::ActiveSelection,
        }
    );
    assert!(
        pending.message.contains("protected source"),
        "message should explain the protected-source requirement: {}",
        pending.message
    );
    assert!(
        state
            .ui
            .browser_interaction
            .pending_waveform_destructive_edit
            .is_none()
    );
    assert!(state.ui.status.sample.contains("writable target source"));
    assert_eq!(
        state
            .library
            .folder_browser
            .protected_source_error_flash_frames(),
        0
    );
    assert_eq!(
        state.waveform.current.protected_source_error_flash_frames(),
        0
    );

    state.apply_message(
        GuiMessage::CancelProtectedExtractionTargetSource,
        &mut ui::UiUpdateContext::default(),
    );

    assert!(
        state
            .ui
            .browser_interaction
            .pending_protected_extraction_target_source
            .is_none()
    );
}

#[test]
fn blocked_protected_source_destructive_edit_flashes_source_file_and_waveform() {
    let (mut state, source_root, selected_file) =
        native_app_state_with_temp_sample("protected-reverse-blocked.wav");
    let protected_source =
        wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()).protected();
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            protected_source.clone(),
        ]);
    state
        .library
        .folder_browser
        .select_file(selected_file.clone());
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("load waveform");
    state.ui.settings.persisted.controls.destructive_yolo_mode = true;
    select_waveform_range(&mut state, WaveformSelectionKind::Play, 0.25, 0.75);

    state.apply_message(
        GuiMessage::RequestReverseWaveformSelection,
        &mut ui::UiUpdateContext::default(),
    );

    let initial_browser_frames = state
        .library
        .folder_browser
        .protected_source_error_flash_frames();
    let initial_waveform_frames = state.waveform.current.protected_source_error_flash_frames();
    assert!(initial_browser_frames > 0);
    assert!(initial_waveform_frames > 0);
    assert_eq!(
        state.ui.status.sample,
        "Protected source cannot be modified"
    );
    assert_protected_source_error_projected(&state, &selected_file, &protected_source.id);

    state.apply_message(GuiMessage::Frame, &mut ui::UiUpdateContext::default());
    assert!(
        state
            .library
            .folder_browser
            .protected_source_error_flash_frames()
            < initial_browser_frames
    );
    assert!(state.waveform.current.protected_source_error_flash_frames() < initial_waveform_frames);

    state.apply_message(
        GuiMessage::RequestReverseWaveformSelection,
        &mut ui::UiUpdateContext::default(),
    );
    assert_eq!(
        state
            .library
            .folder_browser
            .protected_source_error_flash_frames(),
        initial_browser_frames
    );
    assert_eq!(
        state.waveform.current.protected_source_error_flash_frames(),
        initial_waveform_frames
    );
}

#[test]
fn protected_extract_target_source_dialog_marks_primary_and_resumes_extraction() {
    let config_base = tempfile::tempdir().expect("config base");
    let _base_guard = wavecrate::app_dirs::ConfigBaseGuard::set(config_base.path().to_path_buf());
    let (mut state, source_root, selected_file) =
        native_app_state_with_temp_sample("protected-extract-add-target.wav");
    let target_root = tempfile::tempdir().expect("target source root");
    let protected_source =
        wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()).protected();
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            protected_source.clone(),
        ]);
    state
        .library
        .folder_browser
        .select_file(selected_file.clone());
    let path = PathBuf::from(&selected_file);
    let harvest_source_folder = source_root
        .path()
        .file_name()
        .expect("source root folder name");
    let extracted = target_root
        .path()
        .join("_Harvests")
        .join(harvest_source_folder)
        .join("protected-extract-add-target_extraction.wav");
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("load waveform");
    select_waveform_range(&mut state, WaveformSelectionKind::Play, 0.25, 0.5);
    state.apply_message(
        GuiMessage::RequestExtractAndTrimWaveformSelection,
        &mut ui::UiUpdateContext::default(),
    );
    assert!(
        state
            .ui
            .browser_interaction
            .pending_protected_extraction_target_source
            .is_some()
    );
    let mut context = ui::UiUpdateContext::default();

    state.apply_message(
        GuiMessage::ProtectedExtractionTargetSourceDialogFinished(Ok(
            radiant::runtime::PlatformResponse::Path(target_root.path().to_path_buf()),
        )),
        &mut context,
    );
    run_command_for_tests(&mut state, context.into_command());

    assert!(
        state
            .ui
            .browser_interaction
            .pending_protected_extraction_target_source
            .is_none()
    );
    let target_source_id = state
        .library
        .folder_browser
        .source_id_for_root_path(target_root.path())
        .expect("target source should be configured");
    assert_eq!(
        state.library.folder_browser.source_role(&target_source_id),
        Some(wavecrate::sample_sources::SourceRole::Primary)
    );
    assert_samples_close(&read_test_wav_f32(&path), &[0.0, 1_000.0, 2_000.0, 3_000.0]);
    assert_samples_close(&read_test_wav_f32(&extracted), &[1_000.0]);
}

#[test]
fn protected_crop_selection_renders_crop_copy_to_primary_without_mutating_origin() {
    let config_base = tempfile::tempdir().expect("config base");
    let _base_guard = wavecrate::app_dirs::ConfigBaseGuard::set(config_base.path().to_path_buf());
    let (mut state, source_root, selected_file) =
        native_app_state_with_temp_sample("protected-crop.wav");
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
    let harvest_source_folder = source_root
        .path()
        .file_name()
        .expect("source root folder name");
    let crop_copy = primary_root
        .path()
        .join("_Harvests")
        .join(harvest_source_folder)
        .join("protected-crop_extraction.wav");
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("load waveform");
    state.ui.settings.persisted.controls.destructive_yolo_mode = true;
    select_waveform_range(&mut state, WaveformSelectionKind::Play, 0.25, 0.5);
    let mut context = ui::UiUpdateContext::default();

    state.apply_message(GuiMessage::RequestCropWaveformSelection, &mut context);
    run_command_for_tests(&mut state, context.into_command());

    assert_samples_close(&read_test_wav_f32(&path), &[0.0, 1_000.0, 2_000.0, 3_000.0]);
    assert_samples_close(&read_test_wav_f32(&crop_copy), &[1_000.0]);
    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(crop_copy.to_string_lossy().as_ref())
    );
    let parent_key = wavecrate::sample_sources::HarvestFileKey::new(
        protected_source.id.clone(),
        PathBuf::from("protected-crop.wav"),
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
        wavecrate::sample_sources::HarvestDerivationOperation::CropCopy
    );
    assert_eq!(edges[0].child.key.source_id, primary_source.id);
    assert_eq!(
        edges[0].child.key.relative_path,
        PathBuf::from("_Harvests")
            .join(harvest_source_folder)
            .join("protected-crop_extraction.wav")
    );
}

#[test]
fn normal_harvest_mode_crop_selection_renders_crop_copy_to_primary_without_mutating_origin() {
    let config_base = tempfile::tempdir().expect("config base");
    let _base_guard = wavecrate::app_dirs::ConfigBaseGuard::set(config_base.path().to_path_buf());
    let (mut state, source_root, selected_file) =
        native_app_state_with_temp_sample("normal-harvest-crop.wav");
    let primary_root = tempfile::tempdir().expect("primary source root");
    let origin_source =
        wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf());
    let primary_source =
        wavecrate::sample_sources::SampleSource::new(primary_root.path().to_path_buf()).primary();
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            origin_source.clone(),
            primary_source.clone(),
        ]);
    state
        .library
        .folder_browser
        .select_file(selected_file.clone());
    state.library.folder_browser.set_harvest_filter(
        crate::native_app::sample_library::folder_browser::model::HarvestFilter::NeedsReview,
        true,
    );
    let path = PathBuf::from(&selected_file);
    let harvest_source_folder = source_root
        .path()
        .file_name()
        .expect("source root folder name");
    let crop_copy = primary_root
        .path()
        .join("_Harvests")
        .join(harvest_source_folder)
        .join("normal-harvest-crop_extraction.wav");
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("load waveform");
    state.ui.settings.persisted.controls.destructive_yolo_mode = true;
    select_waveform_range(&mut state, WaveformSelectionKind::Play, 0.25, 0.5);
    let mut context = ui::UiUpdateContext::default();

    state.apply_message(GuiMessage::RequestCropWaveformSelection, &mut context);
    run_command_for_tests(&mut state, context.into_command());

    assert_samples_close(&read_test_wav_f32(&path), &[0.0, 1_000.0, 2_000.0, 3_000.0]);
    assert_samples_close(&read_test_wav_f32(&crop_copy), &[1_000.0]);
    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(crop_copy.to_string_lossy().as_ref())
    );
    let parent_key = wavecrate::sample_sources::HarvestFileKey::new(
        origin_source.id.clone(),
        PathBuf::from("normal-harvest-crop.wav"),
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
        wavecrate::sample_sources::HarvestDerivationOperation::CropCopy
    );
    assert_eq!(edges[0].child.key.source_id, primary_source.id);
    assert_eq!(
        edges[0].child.key.relative_path,
        PathBuf::from("_Harvests")
            .join(harvest_source_folder)
            .join("normal-harvest-crop_extraction.wav")
    );
}

#[test]
fn protected_reverse_selection_renders_reverse_copy_to_primary_without_mutating_origin() {
    let config_base = tempfile::tempdir().expect("config base");
    let _base_guard = wavecrate::app_dirs::ConfigBaseGuard::set(config_base.path().to_path_buf());
    let (mut state, source_root, selected_file) =
        native_app_state_with_temp_sample("protected-reverse.wav");
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
    let harvest_source_folder = source_root
        .path()
        .file_name()
        .expect("source root folder name");
    let reverse_copy = primary_root
        .path()
        .join("_Harvests")
        .join(harvest_source_folder)
        .join("protected-reverse_reverse.wav");
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000, 4_000, 5_000, 6_000, 7_000]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("load waveform");
    state.ui.settings.persisted.controls.destructive_yolo_mode = true;
    select_waveform_range(&mut state, WaveformSelectionKind::Play, 0.25, 0.75);
    let mut context = ui::UiUpdateContext::default();

    state.apply_message(GuiMessage::RequestReverseWaveformSelection, &mut context);
    run_command_for_tests(&mut state, context.into_command());

    assert_samples_close(
        &read_test_wav_f32(&path),
        &[
            0.0, 1_000.0, 2_000.0, 3_000.0, 4_000.0, 5_000.0, 6_000.0, 7_000.0,
        ],
    );
    assert_samples_close(
        &read_test_wav_f32(&reverse_copy),
        &[
            0.0, 1_000.0, 5_000.0, 4_000.0, 3_000.0, 2_000.0, 6_000.0, 7_000.0,
        ],
    );
    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(reverse_copy.to_string_lossy().as_ref())
    );
    let parent_key = wavecrate::sample_sources::HarvestFileKey::new(
        protected_source.id.clone(),
        PathBuf::from("protected-reverse.wav"),
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
        wavecrate::sample_sources::HarvestDerivationOperation::ReverseCopy
    );
    assert_eq!(edges[0].child.key.source_id, primary_source.id);
    assert_eq!(
        edges[0].child.key.relative_path,
        PathBuf::from("_Harvests")
            .join(harvest_source_folder)
            .join("protected-reverse_reverse.wav")
    );
}

#[test]
fn protected_mute_selection_renders_edit_copy_to_primary_without_mutating_origin() {
    let config_base = tempfile::tempdir().expect("config base");
    let _base_guard = wavecrate::app_dirs::ConfigBaseGuard::set(config_base.path().to_path_buf());
    let (mut state, source_root, selected_file) =
        native_app_state_with_temp_sample("protected-mute.wav");
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
    let harvest_source_folder = source_root
        .path()
        .file_name()
        .expect("source root folder name");
    let mute_copy = primary_root
        .path()
        .join("_Harvests")
        .join(harvest_source_folder)
        .join("protected-mute_mute.wav");
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000, 4_000, 5_000, 6_000, 7_000]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("load waveform");
    state.ui.settings.persisted.controls.destructive_yolo_mode = true;
    select_waveform_range(&mut state, WaveformSelectionKind::Play, 0.25, 0.75);
    let mut context = ui::UiUpdateContext::default();

    state.apply_message(GuiMessage::RequestMuteWaveformSelection, &mut context);
    run_command_for_tests(&mut state, context.into_command());

    assert_samples_close(
        &read_test_wav_f32(&path),
        &[
            0.0, 1_000.0, 2_000.0, 3_000.0, 4_000.0, 5_000.0, 6_000.0, 7_000.0,
        ],
    );
    assert_samples_close(
        &read_test_wav_f32(&mute_copy),
        &[0.0, 1_000.0, 0.0, 0.0, 0.0, 0.0, 6_000.0, 7_000.0],
    );
    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(mute_copy.to_string_lossy().as_ref())
    );
    let parent_key = wavecrate::sample_sources::HarvestFileKey::new(
        protected_source.id.clone(),
        PathBuf::from("protected-mute.wav"),
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
        wavecrate::sample_sources::HarvestDerivationOperation::EditCopy
    );
    assert_eq!(edges[0].child.key.source_id, primary_source.id);
    assert_eq!(
        edges[0].child.key.relative_path,
        PathBuf::from("_Harvests")
            .join(harvest_source_folder)
            .join("protected-mute_mute.wav")
    );
}

#[test]
fn protected_sample_slide_renders_slide_copy_to_primary_without_mutating_origin() {
    let config_base = tempfile::tempdir().expect("config base");
    let _base_guard = wavecrate::app_dirs::ConfigBaseGuard::set(config_base.path().to_path_buf());
    let (mut state, source_root, selected_file) =
        native_app_state_with_temp_sample("protected-slide.wav");
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
    let harvest_source_folder = source_root
        .path()
        .file_name()
        .expect("source root folder name");
    let slide_copy = primary_root
        .path()
        .join("_Harvests")
        .join(harvest_source_folder)
        .join("protected-slide_slide.wav");
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("load waveform");
    state.ui.settings.persisted.controls.destructive_yolo_mode = true;

    state.apply_message(
        GuiMessage::Waveform(WaveformInteraction::BeginSampleSlide { visible_ratio: 0.0 }),
        &mut ui::UiUpdateContext::default(),
    );
    apply_message_and_run_command(
        &mut state,
        GuiMessage::Waveform(WaveformInteraction::FinishSampleSlide {
            visible_ratio: 0.25,
        }),
    );

    assert_samples_close(&read_test_wav_f32(&path), &[0.0, 1_000.0, 2_000.0, 3_000.0]);
    assert_samples_close(
        &read_test_wav_f32(&slide_copy),
        &[3_000.0, 0.0, 1_000.0, 2_000.0],
    );
    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(slide_copy.to_string_lossy().as_ref())
    );
    let parent_key = wavecrate::sample_sources::HarvestFileKey::new(
        protected_source.id.clone(),
        PathBuf::from("protected-slide.wav"),
    );
    let edges = wavecrate::sample_sources::library::harvest_derivations_for_parent(&parent_key)
        .expect("load harvest derivations");
    assert_eq!(edges.len(), 1);
    assert_eq!(
        edges[0].operation,
        wavecrate::sample_sources::HarvestDerivationOperation::SlideCopy
    );
    assert_eq!(edges[0].child.key.source_id, primary_source.id);
    assert_eq!(
        edges[0].child.key.relative_path,
        PathBuf::from("_Harvests")
            .join(harvest_source_folder)
            .join("protected-slide_slide.wav")
    );
}

#[test]
fn normal_harvest_mode_reverse_selection_renders_reverse_copy_to_primary_without_mutating_origin() {
    let config_base = tempfile::tempdir().expect("config base");
    let _base_guard = wavecrate::app_dirs::ConfigBaseGuard::set(config_base.path().to_path_buf());
    let (mut state, source_root, selected_file) =
        native_app_state_with_temp_sample("normal-harvest-reverse.wav");
    let primary_root = tempfile::tempdir().expect("primary source root");
    let origin_source =
        wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf());
    let primary_source =
        wavecrate::sample_sources::SampleSource::new(primary_root.path().to_path_buf()).primary();
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            origin_source.clone(),
            primary_source.clone(),
        ]);
    state
        .library
        .folder_browser
        .select_file(selected_file.clone());
    state.library.folder_browser.set_harvest_filter(
        crate::native_app::sample_library::folder_browser::model::HarvestFilter::NeedsReview,
        true,
    );
    let path = PathBuf::from(&selected_file);
    let harvest_source_folder = source_root
        .path()
        .file_name()
        .expect("source root folder name");
    let reverse_copy = primary_root
        .path()
        .join("_Harvests")
        .join(harvest_source_folder)
        .join("normal-harvest-reverse_reverse.wav");
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000, 4_000, 5_000, 6_000, 7_000]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("load waveform");
    state.ui.settings.persisted.controls.destructive_yolo_mode = true;
    select_waveform_range(&mut state, WaveformSelectionKind::Play, 0.25, 0.75);
    let mut context = ui::UiUpdateContext::default();

    state.apply_message(GuiMessage::RequestReverseWaveformSelection, &mut context);
    run_command_for_tests(&mut state, context.into_command());

    assert_samples_close(
        &read_test_wav_f32(&path),
        &[
            0.0, 1_000.0, 2_000.0, 3_000.0, 4_000.0, 5_000.0, 6_000.0, 7_000.0,
        ],
    );
    assert_samples_close(
        &read_test_wav_f32(&reverse_copy),
        &[
            0.0, 1_000.0, 5_000.0, 4_000.0, 3_000.0, 2_000.0, 6_000.0, 7_000.0,
        ],
    );
    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(reverse_copy.to_string_lossy().as_ref())
    );
    let parent_key = wavecrate::sample_sources::HarvestFileKey::new(
        origin_source.id.clone(),
        PathBuf::from("normal-harvest-reverse.wav"),
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
        wavecrate::sample_sources::HarvestDerivationOperation::ReverseCopy
    );
    assert_eq!(edges[0].child.key.source_id, primary_source.id);
    assert_eq!(
        edges[0].child.key.relative_path,
        PathBuf::from("_Harvests")
            .join(harvest_source_folder)
            .join("normal-harvest-reverse_reverse.wav")
    );
}

#[test]
fn protected_trim_selection_is_denied_with_red_feedback_even_with_primary_source() {
    let (mut state, source_root, selected_file) =
        native_app_state_with_temp_sample("protected-trim.wav");
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
    let harvest_source_folder = source_root
        .path()
        .file_name()
        .expect("source root folder name");
    let trim_copy = primary_root
        .path()
        .join("_Harvests")
        .join(harvest_source_folder)
        .join("protected-trim_trim.wav");
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000, 4_000, 5_000, 6_000, 7_000]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("load waveform");
    state.ui.settings.persisted.controls.destructive_yolo_mode = true;
    select_waveform_range(&mut state, WaveformSelectionKind::Play, 0.25, 0.75);
    let mut context = ui::UiUpdateContext::default();

    state.apply_message(GuiMessage::RequestTrimWaveformSelection, &mut context);
    assert!(
        context.into_command().is_empty(),
        "denied trim must not queue background file work"
    );

    assert_samples_close(
        &read_test_wav_f32(&path),
        &[
            0.0, 1_000.0, 2_000.0, 3_000.0, 4_000.0, 5_000.0, 6_000.0, 7_000.0,
        ],
    );
    assert!(!trim_copy.exists(), "denied trim must not create a copy");
    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(selected_file.as_str())
    );
    assert!(
        state
            .ui
            .browser_interaction
            .pending_waveform_destructive_edit
            .is_none()
    );
    assert_eq!(
        state.ui.status.sample,
        "Protected source cannot be modified"
    );
    assert!(
        state.waveform.current.play_selection_denied_flash_frames() > 0,
        "the denied play selection should start its red pulse"
    );
    assert!(
        state
            .library
            .folder_browser
            .protected_source_error_flash_frames()
            > 0,
        "the protected source and file rows should start their red pulse"
    );
    assert!(state.waveform.current.protected_source_error_flash_frames() > 0);
    assert_protected_source_error_projected(&state, &selected_file, &protected_source.id);
}

#[test]
fn protected_apply_edit_effects_renders_edit_copy_to_primary_without_mutating_origin() {
    let config_base = tempfile::tempdir().expect("config base");
    let _base_guard = wavecrate::app_dirs::ConfigBaseGuard::set(config_base.path().to_path_buf());
    let (mut state, source_root, selected_file) =
        native_app_state_with_temp_sample("protected-edit.wav");
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
    let harvest_source_folder = source_root
        .path()
        .file_name()
        .expect("source root folder name");
    let edit_copy = primary_root
        .path()
        .join("_Harvests")
        .join(harvest_source_folder)
        .join("protected-edit_edit.wav");
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000, 4_000, 5_000, 6_000, 7_000]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("load waveform");
    state.ui.settings.persisted.controls.destructive_yolo_mode = true;
    state.waveform.current.set_edit_selection_range(
        wavecrate::selection::SelectionRange::new(0.25, 0.75).with_gain(0.5),
    );
    let mut context = ui::UiUpdateContext::default();

    state.apply_message(GuiMessage::RequestApplyEditSelectionEffects, &mut context);
    run_command_for_tests(&mut state, context.into_command());

    assert_samples_close(
        &read_test_wav_f32(&path),
        &[
            0.0, 1_000.0, 2_000.0, 3_000.0, 4_000.0, 5_000.0, 6_000.0, 7_000.0,
        ],
    );
    assert_samples_close(
        &read_test_wav_f32(&edit_copy),
        &[
            0.0, 1_000.0, 1_000.0, 1_500.0, 2_000.0, 2_500.0, 6_000.0, 7_000.0,
        ],
    );
    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(edit_copy.to_string_lossy().as_ref())
    );
    let parent_key = wavecrate::sample_sources::HarvestFileKey::new(
        protected_source.id.clone(),
        PathBuf::from("protected-edit.wav"),
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
        wavecrate::sample_sources::HarvestDerivationOperation::EditCopy
    );
    assert_eq!(edges[0].child.key.source_id, primary_source.id);
    assert_eq!(
        edges[0].child.key.relative_path,
        PathBuf::from("_Harvests")
            .join(harvest_source_folder)
            .join("protected-edit_edit.wav")
    );
}

#[test]
fn playmark_context_crop_uses_play_selection_even_when_edit_selection_exists() {
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("crop-playmark-menu.wav");
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000, 4_000, 5_000, 6_000, 7_000]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("load waveform");
    state.ui.settings.persisted.controls.destructive_yolo_mode = false;

    select_waveform_range(&mut state, WaveformSelectionKind::Play, 0.25, 0.5);
    state
        .waveform
        .current
        .set_edit_selection_range(wavecrate::selection::SelectionRange::new(0.5, 0.75));

    state.apply_message(
        GuiMessage::RequestCropPlaymarkSelection,
        &mut ui::UiUpdateContext::default(),
    );

    let pending = state
        .ui
        .browser_interaction
        .pending_waveform_destructive_edit
        .as_ref()
        .expect("playmark crop request should prompt");
    assert_eq!(
        pending.prompt.edit,
        crate::native_app::app::WaveformDestructiveEditKind::CropSelection
    );
    assert_range_close(pending.selection, 0.25, 0.5);
}

#[test]
fn trim_request_uses_play_selection_when_no_edit_selection_exists() {
    let (mut state, _source_root, selected_file) = native_app_state_with_temp_sample("trim.wav");
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000, 4_000, 5_000, 6_000, 7_000]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("load waveform");
    state.ui.settings.persisted.controls.destructive_yolo_mode = false;

    select_waveform_range(&mut state, WaveformSelectionKind::Play, 0.25, 0.5);

    state.apply_message(
        GuiMessage::RequestTrimWaveformSelection,
        &mut ui::UiUpdateContext::default(),
    );

    let pending = state
        .ui
        .browser_interaction
        .pending_waveform_destructive_edit
        .as_ref()
        .expect("trim request should prompt");
    assert_eq!(
        pending.prompt.edit,
        crate::native_app::app::WaveformDestructiveEditKind::TrimSelection
    );
    assert!((pending.selection.start() - 0.25).abs() < 0.001);
    assert!((pending.selection.end() - 0.5).abs() < 0.001);
}

#[test]
fn trim_request_uses_edit_selection_before_play_selection() {
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("trim-edit.wav");
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000, 4_000, 5_000, 6_000, 7_000]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("load waveform");
    state.ui.settings.persisted.controls.destructive_yolo_mode = false;

    select_waveform_range(&mut state, WaveformSelectionKind::Play, 0.25, 0.5);
    state
        .waveform
        .current
        .set_edit_selection_range(wavecrate::selection::SelectionRange::new(0.5, 0.75));

    state.apply_message(
        GuiMessage::RequestTrimWaveformSelection,
        &mut ui::UiUpdateContext::default(),
    );

    let pending = state
        .ui
        .browser_interaction
        .pending_waveform_destructive_edit
        .as_ref()
        .expect("trim request should prompt");
    assert_eq!(
        pending.prompt.edit,
        crate::native_app::app::WaveformDestructiveEditKind::TrimSelection
    );
    assert!((pending.selection.start() - 0.5).abs() < 0.001);
    assert!((pending.selection.end() - 0.75).abs() < 0.001);
}

#[test]
fn mute_request_uses_play_selection_when_no_edit_selection_exists() {
    let (mut state, _source_root, selected_file) = native_app_state_with_temp_sample("mute.wav");
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000, 4_000, 5_000, 6_000, 7_000]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("load waveform");
    state.ui.settings.persisted.controls.destructive_yolo_mode = false;

    select_waveform_range(&mut state, WaveformSelectionKind::Play, 0.25, 0.5);

    state.apply_message(
        GuiMessage::RequestMuteWaveformSelection,
        &mut ui::UiUpdateContext::default(),
    );

    let pending = state
        .ui
        .browser_interaction
        .pending_waveform_destructive_edit
        .as_ref()
        .expect("mute request should prompt");
    assert_eq!(
        pending.prompt.edit,
        crate::native_app::app::WaveformDestructiveEditKind::MuteSelection
    );
    assert_range_close(pending.selection, 0.25, 0.5);
}

#[test]
fn mute_request_uses_edit_selection_before_play_selection() {
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("mute-edit.wav");
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000, 4_000, 5_000, 6_000, 7_000]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("load waveform");
    state.ui.settings.persisted.controls.destructive_yolo_mode = false;

    select_waveform_range(&mut state, WaveformSelectionKind::Play, 0.25, 0.5);
    state
        .waveform
        .current
        .set_edit_selection_range(wavecrate::selection::SelectionRange::new(0.5, 0.75));

    state.apply_message(
        GuiMessage::RequestMuteWaveformSelection,
        &mut ui::UiUpdateContext::default(),
    );

    let pending = state
        .ui
        .browser_interaction
        .pending_waveform_destructive_edit
        .as_ref()
        .expect("mute request should prompt");
    assert_eq!(
        pending.prompt.edit,
        crate::native_app::app::WaveformDestructiveEditKind::MuteSelection
    );
    assert_range_close(pending.selection, 0.5, 0.75);
}

#[test]
fn mute_request_without_valid_selection_is_safe_noop() {
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("mute-no-selection.wav");
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("load waveform");

    state.apply_message(
        GuiMessage::RequestMuteWaveformSelection,
        &mut ui::UiUpdateContext::default(),
    );

    assert!(
        state
            .ui
            .browser_interaction
            .pending_waveform_destructive_edit
            .is_none()
    );
    assert_samples_close(&read_test_wav_f32(&path), &[0.0, 1_000.0, 2_000.0, 3_000.0]);
    assert!(
        state
            .ui
            .status
            .sample
            .contains("Mark an edit or play range")
    );
}

#[test]
fn reverse_request_uses_edit_selection_before_play_selection() {
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("reverse-edit.wav");
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000, 4_000, 5_000, 6_000, 7_000]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("load waveform");
    state.ui.settings.persisted.controls.destructive_yolo_mode = false;

    select_waveform_range(&mut state, WaveformSelectionKind::Play, 0.25, 0.5);
    state
        .waveform
        .current
        .set_edit_selection_range(wavecrate::selection::SelectionRange::new(0.5, 0.75));

    state.apply_message(
        GuiMessage::RequestReverseWaveformSelection,
        &mut ui::UiUpdateContext::default(),
    );

    let pending = state
        .ui
        .browser_interaction
        .pending_waveform_destructive_edit
        .as_ref()
        .expect("reverse request should prompt");
    assert_eq!(
        pending.prompt.edit,
        crate::native_app::app::WaveformDestructiveEditKind::ReverseSelection
    );
    assert!(
        pending
            .prompt
            .message
            .contains("reverse the selected region")
    );
    assert!((pending.selection.start() - 0.5).abs() < 0.001);
    assert!((pending.selection.end() - 0.75).abs() < 0.001);
}

#[test]
fn reverse_request_uses_selected_file_when_no_waveform_range_exists() {
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("reverse-file.wav");
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000]);
    state.ui.settings.persisted.controls.destructive_yolo_mode = false;

    state.apply_message(
        GuiMessage::RequestReverseWaveformSelection,
        &mut ui::UiUpdateContext::default(),
    );

    let pending = state
        .ui
        .browser_interaction
        .pending_waveform_destructive_edit
        .as_ref()
        .expect("reverse request should prompt for the selected file");
    assert_eq!(
        pending.prompt.edit,
        crate::native_app::app::WaveformDestructiveEditKind::ReverseSelection
    );
    assert_eq!(pending.absolute_path, path);
    assert!(
        pending
            .prompt
            .message
            .contains("selected file when no region is marked")
    );
    assert_range_close(pending.selection, 0.0, 1.0);
}

#[test]
fn extract_and_trim_request_uses_play_selection_when_no_edit_selection_exists() {
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("extract-trim.wav");
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000, 4_000, 5_000, 6_000, 7_000]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("load waveform");
    state.ui.settings.persisted.controls.destructive_yolo_mode = false;

    select_waveform_range(&mut state, WaveformSelectionKind::Play, 0.25, 0.5);

    state.apply_message(
        GuiMessage::RequestExtractAndTrimWaveformSelection,
        &mut ui::UiUpdateContext::default(),
    );

    let pending = state
        .ui
        .browser_interaction
        .pending_waveform_destructive_edit
        .as_ref()
        .expect("extract-and-trim request should prompt");
    assert_eq!(
        pending.prompt.edit,
        crate::native_app::app::WaveformDestructiveEditKind::ExtractAndTrimSelection
    );
    assert!(
        pending
            .prompt
            .message
            .contains("extract the selected region")
    );
    assert!((pending.selection.start() - 0.25).abs() < 0.001);
    assert!((pending.selection.end() - 0.5).abs() < 0.001);
}

#[test]
fn apply_edit_selection_effects_rewrites_gain_clears_preview_and_flashes() {
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("apply-effects.wav");
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000, 4_000, 5_000, 6_000, 7_000]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("load waveform");
    state.ui.settings.persisted.controls.destructive_yolo_mode = true;
    state.waveform.current.set_edit_selection_range(
        wavecrate::selection::SelectionRange::new(0.25, 0.75).with_gain(0.5),
    );

    apply_message_and_run_command(&mut state, GuiMessage::RequestApplyEditSelectionEffects);

    assert_samples_close(
        &read_test_wav_f32(&path),
        &[
            0.0, 1_000.0, 1_000.0, 1_500.0, 2_000.0, 2_500.0, 6_000.0, 7_000.0,
        ],
    );
    assert!(state.ui.status.sample.contains("Applied edit mark edits"));
    assert!(state.waveform.current.edit_selection_flash_frames() > 0);
    assert!(
        !state
            .waveform
            .current
            .edit_selection()
            .expect("cleared edit selection remains visible")
            .has_edit_effects()
    );

    state.apply_message(
        GuiMessage::UndoTransaction,
        &mut ui::UiUpdateContext::default(),
    );

    assert_samples_close(
        &read_test_wav_f32(&path),
        &[
            0.0, 1_000.0, 2_000.0, 3_000.0, 4_000.0, 5_000.0, 6_000.0, 7_000.0,
        ],
    );
}

#[test]
fn destructive_edit_reload_failure_still_emits_committed_mutation() {
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("reload-failure.wav");
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000, 4_000, 5_000, 6_000, 7_000]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("load waveform");
    state.ui.settings.persisted.controls.destructive_yolo_mode = true;
    select_waveform_range(&mut state, WaveformSelectionKind::Play, 0.25, 0.5);

    let mut request_context = ui::UiUpdateContext::default();
    state.apply_message(
        GuiMessage::RequestCropWaveformSelection,
        &mut request_context,
    );
    let mut completion_followups = Vec::new();
    let completion = crate::native_app::tests::run_worker_message_for_tests(
        request_context.into_command(),
        "gui-waveform-destructive-edit",
    )
    .expect("destructive edit worker completion");
    if matches!(completion, GuiMessage::WaveformDestructiveEditFinished(_)) {
        fs::write(&path, b"not a waveform").expect("corrupt committed waveform");
    }
    let mut completion_context = ui::UiUpdateContext::default();
    state.apply_message(completion, &mut completion_context);
    completion_followups.push(completion_context.into_command());

    assert!(
        commands_emit_committed_file_mutation(completion_followups),
        "the committed edit must be published before the visual reload"
    );
    assert!(
        state
            .ui
            .status
            .sample
            .contains("edit committed but waveform reload failed")
    );
}

#[test]
fn waveform_undo_reload_failure_still_emits_committed_mutation() {
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("undo-reload-failure.wav");
    let browser_size_before_undo = state
        .library
        .folder_browser
        .selected_files()
        .first()
        .expect("browser fixture file")
        .size_bytes;
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000, 4_000, 5_000, 6_000, 7_000]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("load waveform");
    state.ui.settings.persisted.controls.destructive_yolo_mode = false;
    select_waveform_range(&mut state, WaveformSelectionKind::Play, 0.25, 0.5);
    state.apply_message(
        GuiMessage::RequestCropWaveformSelection,
        &mut ui::UiUpdateContext::default(),
    );
    let request = state
        .ui
        .browser_interaction
        .pending_waveform_destructive_edit
        .take()
        .expect("crop request");
    let applied = crate::native_app::waveform_edits::execute_destructive_edit_for_tests(request);
    let backup_path =
        crate::native_app::waveform_edits::destructive_edit_before_backup_path_for_tests(&applied);
    fs::write(&backup_path, b"not a waveform").expect("corrupt undo snapshot");

    let undo_applied = applied.clone();
    let undo_backup_path = backup_path.clone();
    state.begin_transaction("test waveform edit");
    state.register_transaction_action(
        "undo test waveform edit",
        move |transaction| transaction.restore_edited_waveform(&undo_backup_path, &undo_applied),
        |_| Ok(()),
    );
    assert!(state.commit_transaction());

    let mut undo_context = ui::UiUpdateContext::default();
    state.undo_transaction(&mut undo_context);

    assert!(
        commands_emit_committed_file_mutation(vec![undo_context.into_command()]),
        "the restored file must be published even when waveform reload fails"
    );
    assert_eq!(
        state
            .library
            .folder_browser
            .selected_files()
            .first()
            .expect("browser fixture file")
            .size_bytes,
        browser_size_before_undo,
        "undo must not refresh browser metadata before the committed outcome is accepted"
    );
    assert!(state.ui.status.sample.contains("Undo failed:"));
    assert!(
        state.ui.status.sample.contains("Failed to open WAV")
            || state.ui.status.sample.contains("Invalid")
    );
}

#[test]
fn crop_request_rewrites_file_and_undo_restores_original_audio() {
    let (mut state, _source_root, selected_file) = native_app_state_with_temp_sample("crop.wav");
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000, 4_000, 5_000, 6_000, 7_000]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("load waveform");
    state.ui.settings.persisted.controls.destructive_yolo_mode = true;

    select_waveform_range(&mut state, WaveformSelectionKind::Play, 0.25, 0.5);

    apply_message_and_run_command(&mut state, GuiMessage::RequestCropWaveformSelection);

    assert_samples_close(&read_test_wav_f32(&path), &[2_000.0, 3_000.0]);
    assert!(state.ui.status.sample.contains("Cropped"));

    state.apply_message(
        GuiMessage::UndoTransaction,
        &mut ui::UiUpdateContext::default(),
    );

    assert_samples_close(
        &read_test_wav_f32(&path),
        &[
            0.0, 1_000.0, 2_000.0, 3_000.0, 4_000.0, 5_000.0, 6_000.0, 7_000.0,
        ],
    );
}

#[test]
fn mute_request_rewrites_play_selection_without_moving_bounds() {
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("mute-play.wav");
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000, 4_000, 5_000, 6_000, 7_000]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("load waveform");
    state.ui.settings.persisted.controls.destructive_yolo_mode = true;

    select_waveform_range(&mut state, WaveformSelectionKind::Play, 0.25, 0.5);

    apply_message_and_run_command(&mut state, GuiMessage::RequestMuteWaveformSelection);

    assert_samples_close(
        &read_test_wav_f32(&path),
        &[0.0, 1_000.0, 0.0, 0.0, 4_000.0, 5_000.0, 6_000.0, 7_000.0],
    );
    assert_range_close(
        state
            .waveform
            .current
            .play_selection()
            .expect("play selection survives mute"),
        0.25,
        0.5,
    );
    assert!(state.ui.status.sample.contains("Muted"));
}

#[test]
fn mute_request_rewrites_edit_selection_without_moving_bounds() {
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("mute-edit.wav");
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000, 4_000, 5_000, 6_000, 7_000]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("load waveform");
    state.ui.settings.persisted.controls.destructive_yolo_mode = true;

    select_waveform_range(&mut state, WaveformSelectionKind::Play, 0.0, 0.25);
    state
        .waveform
        .current
        .set_edit_selection_range(wavecrate::selection::SelectionRange::new(0.5, 0.75));

    apply_message_and_run_command(&mut state, GuiMessage::RequestMuteWaveformSelection);

    assert_samples_close(
        &read_test_wav_f32(&path),
        &[0.0, 1_000.0, 2_000.0, 3_000.0, 0.0, 0.0, 6_000.0, 7_000.0],
    );
    assert_range_close(
        state
            .waveform
            .current
            .edit_selection()
            .expect("edit selection survives mute"),
        0.5,
        0.75,
    );
    assert!(state.ui.status.sample.contains("Muted"));
}

#[test]
fn crop_request_marks_harvest_origin_touched_without_derivative() {
    let config_base = tempfile::tempdir().expect("config base");
    let _base_guard = wavecrate::app_dirs::ConfigBaseGuard::set(config_base.path().to_path_buf());
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("crop-harvest.wav");
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000, 4_000, 5_000, 6_000, 7_000]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("load waveform");
    state.ui.settings.persisted.controls.destructive_yolo_mode = true;

    select_waveform_range(&mut state, WaveformSelectionKind::Play, 0.25, 0.5);

    apply_message_and_run_command(&mut state, GuiMessage::RequestCropWaveformSelection);

    let (source, relative_path) = state
        .library
        .folder_browser
        .sample_source_for_file_path(&path)
        .expect("source file should stay in source");
    let harvest_key = wavecrate::sample_sources::HarvestFileKey::new(
        wavecrate::sample_sources::SourceId::from_string(source.id.as_str().to_owned()),
        relative_path,
    );
    let harvest_parent = wavecrate::sample_sources::library::harvest_file(&harvest_key)
        .expect("load harvest source")
        .expect("harvest parent");
    assert_eq!(
        harvest_parent.state,
        wavecrate::sample_sources::HarvestState::Touched
    );
    assert!(
        wavecrate::sample_sources::library::harvest_derivations_for_parent(&harvest_key)
            .expect("load harvest derivations")
            .is_empty(),
        "in-place crop should touch the origin without inventing a derivative"
    );
}

#[test]
fn crop_request_refocuses_rating_filtered_curation_sample_for_rerating() {
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("crop-rerate.wav");
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000, 4_000, 5_000, 6_000, 7_000]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("load waveform");
    state.ui.settings.persisted.controls.destructive_yolo_mode = true;
    state.ui.settings.persisted.controls.advance_after_rating = false;

    let mut context = ui::UiUpdateContext::default();
    state.adjust_selected_rating(1, &mut context);
    state.adjust_selected_rating(1, &mut context);
    assert_eq!(
        state
            .library
            .folder_browser
            .selected_audio_files()
            .first()
            .map(|file| file.rating),
        Some(wavecrate::sample_sources::Rating::new(2))
    );
    state.library.folder_browser.apply_message(
        crate::native_app::test_support::state::FolderBrowserMessage::ToggleRatingFilter(1, true),
    );
    state.library.folder_browser.apply_message(
        crate::native_app::test_support::state::FolderBrowserMessage::SetCurationScope(
            crate::native_app::sample_library::folder_browser::model::BrowserCurationScope::All,
            true,
        ),
    );
    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        None,
        "rating filter should hide and clear the K2 row before crop"
    );

    select_waveform_range(&mut state, WaveformSelectionKind::Play, 0.25, 0.5);
    apply_message_and_run_command(&mut state, GuiMessage::RequestCropWaveformSelection);

    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(selected_file.as_str())
    );
    assert_eq!(
        state.library.folder_browser.selected_file_paths(),
        vec![path]
    );

    let mut context = ui::UiUpdateContext::default();
    state.adjust_selected_rating(-1, &mut context);
    let row = state
        .library
        .folder_browser
        .selected_audio_files_matching_tags(&state.metadata.tags_by_file)
        .into_iter()
        .find(|file| file.id == selected_file)
        .expect("cropped file should remain actionable after rerating");
    assert_eq!(row.rating, wavecrate::sample_sources::Rating::KEEP_1);
    assert!(state.ui.status.sample.contains("Rated 1 sample"));
}

#[test]
fn crop_request_pads_virtual_silence_outside_source_bounds() {
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("crop-silence-margin.wav");
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000, 4_000, 5_000, 6_000, 7_000]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("load waveform");
    state.ui.settings.persisted.controls.destructive_yolo_mode = true;
    state.waveform.current.restore_play_selection_state(
        Some(-0.25),
        Some(wavecrate::selection::SelectionRange::new_unclamped(
            -0.25, 1.25,
        )),
        Vec::new(),
    );

    apply_message_and_run_command(&mut state, GuiMessage::RequestCropWaveformSelection);

    assert_samples_close(
        &read_test_wav_f32(&path),
        &[
            0.0, 0.0, 0.0, 1_000.0, 2_000.0, 3_000.0, 4_000.0, 5_000.0, 6_000.0, 7_000.0, 0.0, 0.0,
        ],
    );
    assert!(state.ui.status.sample.contains("Cropped"));
}

#[test]
fn trim_request_rewrites_file_and_undo_restores_original_audio() {
    let (mut state, _source_root, selected_file) = native_app_state_with_temp_sample("trim.wav");
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000, 4_000, 5_000, 6_000, 7_000]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("load waveform");
    state.ui.settings.persisted.controls.destructive_yolo_mode = true;

    select_waveform_range(&mut state, WaveformSelectionKind::Play, 0.25, 0.5);

    apply_message_and_run_command(&mut state, GuiMessage::RequestTrimWaveformSelection);

    assert_samples_close(
        &read_test_wav_f32(&path),
        &[0.0, 1_000.0, 4_000.0, 5_000.0, 6_000.0, 7_000.0],
    );
    assert!(state.ui.status.sample.contains("Trimmed"));

    state.apply_message(
        GuiMessage::UndoTransaction,
        &mut ui::UiUpdateContext::default(),
    );

    assert_samples_close(
        &read_test_wav_f32(&path),
        &[
            0.0, 1_000.0, 2_000.0, 3_000.0, 4_000.0, 5_000.0, 6_000.0, 7_000.0,
        ],
    );
}

#[test]
fn reverse_request_rewrites_selection_and_undo_restores_original_audio() {
    let (mut state, _source_root, selected_file) = native_app_state_with_temp_sample("reverse.wav");
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000, 4_000, 5_000, 6_000, 7_000]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("load waveform");
    state.ui.settings.persisted.controls.destructive_yolo_mode = true;

    select_waveform_range(&mut state, WaveformSelectionKind::Play, 0.25, 0.75);

    apply_message_and_run_command(&mut state, GuiMessage::RequestReverseWaveformSelection);

    assert_samples_close(
        &read_test_wav_f32(&path),
        &[
            0.0, 1_000.0, 5_000.0, 4_000.0, 3_000.0, 2_000.0, 6_000.0, 7_000.0,
        ],
    );
    assert!(state.ui.status.sample.contains("Reversed"));
    assert_range_close(
        state
            .waveform
            .current
            .play_selection()
            .expect("play selection should remain visible"),
        0.25,
        0.75,
    );

    state.apply_message(
        GuiMessage::UndoTransaction,
        &mut ui::UiUpdateContext::default(),
    );

    assert_samples_close(
        &read_test_wav_f32(&path),
        &[
            0.0, 1_000.0, 2_000.0, 3_000.0, 4_000.0, 5_000.0, 6_000.0, 7_000.0,
        ],
    );
}

#[test]
fn reverse_request_rewrites_selected_file_and_undo_restores_original_audio() {
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("reverse-file.wav");
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000, 4_000, 5_000]);
    state.ui.settings.persisted.controls.destructive_yolo_mode = true;

    apply_message_and_run_command(&mut state, GuiMessage::RequestReverseWaveformSelection);

    assert_samples_close(
        &read_test_wav_f32(&path),
        &[5_000.0, 4_000.0, 3_000.0, 2_000.0, 1_000.0, 0.0],
    );
    assert!(state.ui.status.sample.contains("Reversed"));
    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(selected_file.as_str())
    );

    state.apply_message(
        GuiMessage::UndoTransaction,
        &mut ui::UiUpdateContext::default(),
    );

    assert_samples_close(
        &read_test_wav_f32(&path),
        &[0.0, 1_000.0, 2_000.0, 3_000.0, 4_000.0, 5_000.0],
    );

    state.apply_message(
        GuiMessage::RedoTransaction,
        &mut ui::UiUpdateContext::default(),
    );

    assert_samples_close(
        &read_test_wav_f32(&path),
        &[5_000.0, 4_000.0, 3_000.0, 2_000.0, 1_000.0, 0.0],
    );
}

#[test]
fn trim_request_preserves_remaining_marks_after_reloading_waveform() {
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("trim-marks.wav");
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000, 4_000, 5_000, 6_000, 7_000]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("load waveform");
    state.ui.settings.persisted.controls.destructive_yolo_mode = true;

    let anchor = wavecrate::selection::SelectionRange::new(0.0, 0.25);
    let deleted_repeat = wavecrate::selection::SelectionRange::new(0.25, 0.5);
    let remaining_repeat = wavecrate::selection::SelectionRange::new(0.75, 1.0);
    state.waveform.current.set_play_selection_range(0.0, 0.25);
    state.waveform.current.start_similar_sections(anchor);
    state
        .waveform
        .current
        .finish_similar_sections_scan(vec![deleted_repeat, remaining_repeat]);
    state
        .waveform
        .current
        .set_edit_selection_range(deleted_repeat);

    apply_message_and_run_command(&mut state, GuiMessage::RequestTrimWaveformSelection);

    assert_samples_close(
        &read_test_wav_f32(&path),
        &[0.0, 1_000.0, 4_000.0, 5_000.0, 6_000.0, 7_000.0],
    );
    assert!(
        state.waveform.current.similar_sections_enabled(),
        "similar-section mode should stay active after trim reload"
    );
    assert_range_close(
        state
            .waveform
            .current
            .play_selection()
            .expect("anchor play selection should stay visible"),
        0.0,
        2.0 / 6.0,
    );
    assert_eq!(
        state.waveform.current.edit_selection(),
        None,
        "the deleted edit selection should be removed from the overlays"
    );
    let ranges = state.waveform.current.similar_section_ranges();
    assert_eq!(ranges.len(), 1);
    assert_range_close(ranges[0], 4.0 / 6.0, 1.0);
}

#[test]
fn extract_and_trim_request_extracts_selection_trims_source_and_undo_redo_roundtrips() {
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("extract-trim.wav");
    let path = PathBuf::from(&selected_file);
    let extracted = path.with_file_name("extract-trim_extraction.wav");
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000, 4_000, 5_000, 6_000, 7_000]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("load waveform");
    state.ui.settings.persisted.controls.destructive_yolo_mode = true;
    state.audio.loop_playback = true;

    select_waveform_range(&mut state, WaveformSelectionKind::Play, 0.25, 0.5);

    apply_message_and_run_command(
        &mut state,
        GuiMessage::RequestExtractAndTrimWaveformSelection,
    );

    assert_samples_close(&read_test_wav_f32(&extracted), &[0.0, 0.0]);
    assert_eq!(
        state
            .metadata
            .tags_by_file
            .get(&extracted.to_string_lossy().to_string()),
        Some(&vec![String::from("loop")])
    );
    assert_extracted_file_keep_1_rating(&state, &extracted);
    let (source, relative_path) = state
        .library
        .folder_browser
        .sample_source_for_file_path(&path)
        .expect("source file should stay in source");
    let harvest_key = wavecrate::sample_sources::HarvestFileKey::new(
        wavecrate::sample_sources::SourceId::from_string(source.id.as_str().to_owned()),
        relative_path,
    );
    let harvest_parent = wavecrate::sample_sources::library::harvest_file(&harvest_key)
        .expect("load harvest source")
        .expect("harvest parent");
    assert_eq!(
        harvest_parent.state,
        wavecrate::sample_sources::HarvestState::Touched
    );
    let edges = wavecrate::sample_sources::library::harvest_derivations_for_parent(&harvest_key)
        .expect("load harvest derivations");
    assert_eq!(edges.len(), 1);
    assert_eq!(
        edges[0].operation,
        wavecrate::sample_sources::HarvestDerivationOperation::Extract
    );
    assert_eq!(
        edges[0].child.key.relative_path,
        PathBuf::from("extract-trim_extraction.wav")
    );
    let source_range = edges[0]
        .source_range
        .expect("extract-and-trim should record the source range");
    assert!(
        (source_range.start_seconds - (2.0 / 48_000.0)).abs() < 0.000_001,
        "source range start should use the original pre-trim duration"
    );
    assert!(
        (source_range.end_seconds - (4.0 / 48_000.0)).abs() < 0.000_001,
        "source range end should use the original pre-trim duration"
    );
    assert_samples_close(
        &read_test_wav_f32(&path),
        &[0.0, 1_000.0, 4_000.0, 5_000.0, 6_000.0, 7_000.0],
    );
    assert!(state.ui.status.sample.contains("Extracted and trimmed"));

    state.apply_message(
        GuiMessage::UndoTransaction,
        &mut ui::UiUpdateContext::default(),
    );

    assert!(
        !extracted.exists(),
        "undo should remove the generated extraction file"
    );
    assert_samples_close(
        &read_test_wav_f32(&path),
        &[
            0.0, 1_000.0, 2_000.0, 3_000.0, 4_000.0, 5_000.0, 6_000.0, 7_000.0,
        ],
    );

    state.apply_message(
        GuiMessage::RedoTransaction,
        &mut ui::UiUpdateContext::default(),
    );

    assert_samples_close(&read_test_wav_f32(&extracted), &[0.0, 0.0]);
    assert_samples_close(
        &read_test_wav_f32(&path),
        &[0.0, 1_000.0, 4_000.0, 5_000.0, 6_000.0, 7_000.0],
    );
}

#[test]
fn sample_slide_positive_offset_wraps_end_to_beginning() {
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("slide-positive.wav");
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("load waveform");
    state.ui.settings.persisted.controls.destructive_yolo_mode = true;

    state.apply_message(
        GuiMessage::Waveform(WaveformInteraction::BeginSampleSlide { visible_ratio: 0.0 }),
        &mut ui::UiUpdateContext::default(),
    );
    apply_message_and_run_command(
        &mut state,
        GuiMessage::Waveform(WaveformInteraction::FinishSampleSlide {
            visible_ratio: 0.25,
        }),
    );

    assert_samples_close(&read_test_wav_f32(&path), &[3_000.0, 0.0, 1_000.0, 2_000.0]);
    assert_eq!(
        state.waveform.current.frames(),
        4,
        "sample slide should preserve duration"
    );
    assert!(state.ui.status.sample.contains("Slid"));
}

#[test]
fn sample_slide_negative_offset_wraps_beginning_to_end() {
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("slide-negative.wav");
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16(&path, &[0, 1_000, 2_000, 3_000]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("load waveform");
    state.ui.settings.persisted.controls.destructive_yolo_mode = true;

    state.apply_message(
        GuiMessage::Waveform(WaveformInteraction::BeginSampleSlide { visible_ratio: 0.5 }),
        &mut ui::UiUpdateContext::default(),
    );
    apply_message_and_run_command(
        &mut state,
        GuiMessage::Waveform(WaveformInteraction::FinishSampleSlide {
            visible_ratio: 0.25,
        }),
    );

    assert_samples_close(&read_test_wav_f32(&path), &[1_000.0, 2_000.0, 3_000.0, 0.0]);
    assert_eq!(
        state.waveform.current.frames(),
        4,
        "sample slide should preserve duration"
    );
}

#[test]
fn sample_slide_wraps_stereo_audio_by_frame() {
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("slide-stereo.wav");
    let path = PathBuf::from(&selected_file);
    write_test_wav_i16_stereo_for_destructive_tests(
        &path,
        &[(1_000, 10_000), (2_000, 20_000), (3_000, 30_000)],
    );
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(path.clone())
            .expect("load waveform");
    state.ui.settings.persisted.controls.destructive_yolo_mode = true;

    state.apply_message(
        GuiMessage::Waveform(WaveformInteraction::BeginSampleSlide { visible_ratio: 0.0 }),
        &mut ui::UiUpdateContext::default(),
    );
    apply_message_and_run_command(
        &mut state,
        GuiMessage::Waveform(WaveformInteraction::FinishSampleSlide {
            visible_ratio: 1.0 / 3.0,
        }),
    );

    assert_samples_close(
        &read_test_wav_f32(&path),
        &[3_000.0, 30_000.0, 1_000.0, 10_000.0, 2_000.0, 20_000.0],
    );
    assert_eq!(state.waveform.current.channels(), 2);
    assert_eq!(state.waveform.current.frames(), 3);
}

fn select_waveform_range(
    state: &mut crate::native_app::test_support::state::NativeAppState,
    kind: WaveformSelectionKind,
    start: f32,
    end: f32,
) {
    state.apply_message(
        GuiMessage::Waveform(WaveformInteraction::BeginSelection {
            kind,
            visible_ratio: start,
        }),
        &mut ui::UiUpdateContext::default(),
    );
    state.apply_message(
        GuiMessage::Waveform(WaveformInteraction::UpdateSelection { visible_ratio: end }),
        &mut ui::UiUpdateContext::default(),
    );
    state.apply_message(
        GuiMessage::Waveform(WaveformInteraction::FinishSelection { visible_ratio: end }),
        &mut ui::UiUpdateContext::default(),
    );
}

fn assert_protected_source_error_projected(
    state: &crate::native_app::test_support::state::NativeAppState,
    selected_file: &str,
    protected_source_id: &wavecrate::sample_sources::SourceId,
) {
    let tags_by_file = std::collections::HashMap::new();
    let cached_sample_paths = std::collections::HashSet::new();
    let visible = state.library.folder_browser.visible_samples(
        crate::native_app::sample_library::folder_browser::projection::VisibleSampleQuery {
            tags_by_file: &tags_by_file,
            cached_sample_paths: &cached_sample_paths,
        },
    );
    assert!(
        visible
            .rows
            .iter()
            .any(|row| row.file.id == selected_file && row.protected_source_error_flash),
        "selected protected file row should carry protected-source error flash"
    );

    let source_selector = crate::native_app::app_chrome::view_models::library_sidebar::
        SourceSelectorViewModel::from_folder_browser(&state.library.folder_browser, false);
    assert!(
        source_selector.rows.iter().any(|row| {
            row.id == protected_source_id.as_str() && row.protected_source_error_flash
        }),
        "protected source row should carry protected-source error flash"
    );
}

fn apply_message_and_run_command(
    state: &mut crate::native_app::test_support::state::NativeAppState,
    message: GuiMessage,
) {
    let mut context = ui::UiUpdateContext::default();
    state.apply_message(message, &mut context);
    run_command_for_tests(state, context.into_command());
}

fn commands_emit_committed_file_mutation(commands: Vec<Command<GuiMessage>>) -> bool {
    commands.into_iter().any(|command| {
        crate::native_app::tests::run_command_and_capture_messages_for_tests(command)
            .into_iter()
            .any(|message| matches!(message, GuiMessage::CommittedFileMutationRequested(_)))
    })
}

fn assert_extracted_file_keep_1_rating(
    state: &crate::native_app::test_support::state::NativeAppState,
    extracted: &std::path::Path,
) {
    let file_id = extracted.to_string_lossy().to_string();
    let row = state
        .library
        .folder_browser
        .selected_audio_files()
        .into_iter()
        .find(|file| file.id == file_id)
        .expect("extracted file should be visible in the browser");
    assert_eq!(row.rating, wavecrate::sample_sources::Rating::KEEP_1);
    assert!(!row.rating_locked);

    let (source_root, source_database_root, relative_path) = state
        .library
        .folder_browser
        .source_database_relative_file_path(extracted)
        .expect("extracted file should belong to a source");
    let db = wavecrate::sample_sources::SourceDatabase::open_for_ui_read_with_database_root(
        source_root,
        &source_database_root,
    )
    .expect("source database should open");
    let persisted = db
        .list_files()
        .expect("source database files should list")
        .into_iter()
        .find(|entry| entry.relative_path == relative_path)
        .expect("extracted file should be persisted in the source database");
    assert_eq!(persisted.tag, wavecrate::sample_sources::Rating::KEEP_1);
    assert!(!persisted.locked);
}

fn assert_samples_close(actual: &[f32], expected_i16: &[f32]) {
    assert_eq!(actual.len(), expected_i16.len(), "sample length mismatch");
    for (actual, expected) in actual.iter().zip(expected_i16.iter()) {
        let expected = *expected / 32_768.0;
        assert!(
            (*actual - expected).abs() < 0.000_1,
            "expected {expected}, got {actual}"
        );
    }
}

fn assert_range_close(range: wavecrate::selection::SelectionRange, start: f32, end: f32) {
    assert!(
        (range.start() - start).abs() < 0.001,
        "expected range start {start}, got {}",
        range.start()
    );
    assert!(
        (range.end() - end).abs() < 0.001,
        "expected range end {end}, got {}",
        range.end()
    );
}

fn write_test_wav_i16_stereo_for_destructive_tests(path: &std::path::Path, frames: &[(i16, i16)]) {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 48_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).expect("create wav");
    for (left, right) in frames {
        writer.write_sample(*left).expect("write left sample");
        writer.write_sample(*right).expect("write right sample");
    }
    writer.finalize().expect("finalize wav");
}
