use super::test_support::{
    sample_browser::DEFAULT_FOLDER_WIDTH,
    state::{NativeAppState, WaveformInteraction},
};
use super::waveform::{WaveformSelectionEdge, WaveformSelectionKind};
use radiant::{
    gui::types::{Point, Rect, Vector2},
    prelude::{self as ui, IntoView},
    runtime::{
        Command, DeclarativeOwnedCommandRuntimeBridge, Event, PaintTextInput, RuntimeBridge,
        RuntimeHostCapabilities, RuntimeTaskHost, SurfaceFrame, SurfaceRuntime,
        TransientOverlayContext, UiSurface, UiUpdateHandlerDiagnosticsPolicy,
    },
    widgets::{DragHandleMessage, PointerButton, PointerModifiers, WidgetInput, WidgetKey},
};
use std::{fs, path::PathBuf, time::Duration};

mod audio_settings_controls;
mod audio_settings_dropdowns;
mod browser_context_menu;
mod cache_cleanup;
mod collections;
mod config_sources;
mod file_operations;
mod folder_layout;
mod metadata_tag_tests;
mod native_file_drop;
mod native_file_open;
mod release_update;
mod sample_browser;
/// Sample workspace projection layout contract tests.
mod sample_workspace;
mod shortcuts_context;
mod startup;
mod status_bar;
mod toolbar_playback;
mod transactions;
#[path = "tests/waveform_destructive_edit_tests.rs"]
mod waveform_destructive_edit_tests;
mod waveform_panel;
mod waveform_playback;
mod window_chrome;

fn gui_state_for_span_tests() -> NativeAppState {
    super::test_support::state::NativeAppStateFixture::default()
        .with_synthetic_waveform()
        .with_sample_status("")
        .build()
}

fn test_audio_output_enabled() -> bool {
    std::env::var("WAVECRATE_TEST_AUDIO_OUTPUT")
        .ok()
        .is_some_and(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
}

type NativeRuntimeForTests = SurfaceRuntime<
    DeclarativeOwnedCommandRuntimeBridge<
        NativeAppState,
        super::test_support::state::GuiMessage,
        fn(&mut NativeAppState) -> UiSurface<super::test_support::state::GuiMessage>,
        fn(
            &mut NativeAppState,
            super::test_support::state::GuiMessage,
        ) -> Command<super::test_support::state::GuiMessage>,
    >,
    super::test_support::state::GuiMessage,
>;

struct CaptureBridge {
    messages: Vec<super::test_support::state::GuiMessage>,
    target_name: Option<&'static str>,
}

impl RuntimeBridge<super::test_support::state::GuiMessage> for CaptureBridge {
    fn project_surface(
        &mut self,
    ) -> std::sync::Arc<UiSurface<super::test_support::state::GuiMessage>> {
        std::sync::Arc::new(ui::empty::<super::test_support::state::GuiMessage>().into_surface())
    }

    fn pull_surface(&mut self) -> UiSurface<super::test_support::state::GuiMessage> {
        ui::empty::<super::test_support::state::GuiMessage>().into_surface()
    }

    fn update(
        &mut self,
        message: super::test_support::state::GuiMessage,
    ) -> Command<super::test_support::state::GuiMessage> {
        self.messages.push(message);
        Command::none()
    }

    fn host_capabilities(
        &self,
    ) -> RuntimeHostCapabilities<Self, super::test_support::state::GuiMessage> {
        RuntimeHostCapabilities::new().with_tasks()
    }
}

impl RuntimeTaskHost<super::test_support::state::GuiMessage> for CaptureBridge {
    fn spawn_worker_task(
        &mut self,
        name: &'static str,
        _priority: radiant::prelude::TaskPriority,
        _is_cancelled: Option<Box<dyn Fn() -> bool + Send + Sync + 'static>>,
        work: Box<dyn FnOnce() + Send + 'static>,
    ) -> bool {
        if self.target_name.is_some_and(|target| target != name) {
            return false;
        }
        work();
        true
    }
}

fn native_runtime_for_tests(mut state: NativeAppState, viewport: Vector2) -> NativeRuntimeForTests {
    super::test_support::sample_browser::prepare_sample_browser_view(&mut state);
    let mut runtime = radiant::runtime::SurfaceRuntime::new(
        radiant::runtime::declarative_owned_command_runtime_bridge(
            state,
            project_gui_surface_for_tests
                as fn(&mut NativeAppState) -> UiSurface<super::test_support::state::GuiMessage>,
            reduce_gui_message_for_tests
                as fn(
                    &mut NativeAppState,
                    super::test_support::state::GuiMessage,
                ) -> Command<super::test_support::state::GuiMessage>,
        ),
        viewport,
    );
    apply_strict_update_diagnostics(&mut runtime);
    runtime
}

fn apply_strict_update_diagnostics<Bridge, Message>(runtime: &mut SurfaceRuntime<Bridge, Message>)
where
    Bridge: RuntimeBridge<Message>,
{
    runtime.set_update_handler_diagnostics_policy(UiUpdateHandlerDiagnosticsPolicy::panic_at(
        Duration::from_millis(250),
    ));
}

fn project_gui_surface_for_tests(
    state: &mut NativeAppState,
) -> UiSurface<super::test_support::state::GuiMessage> {
    super::test_support::state::view(&*state).into_surface()
}

fn reduce_gui_message_for_tests(
    state: &mut NativeAppState,
    message: super::test_support::state::GuiMessage,
) -> Command<super::test_support::state::GuiMessage> {
    let mut context = ui::UiUpdateContext::default();
    state.handle_message(message, &mut context);
    context.into_command()
}

fn metadata_message(
    message: super::test_support::state::MetadataMessage,
) -> super::test_support::state::GuiMessage {
    super::test_support::state::GuiMessage::Metadata(message)
}

fn focus_metadata_tag_input() -> super::test_support::state::GuiMessage {
    metadata_message(super::test_support::state::MetadataMessage::FocusMetadataTagInput)
}

fn metadata_tag_input(
    message: radiant::widgets::TextInputMessage,
) -> super::test_support::state::GuiMessage {
    metadata_message(super::test_support::state::MetadataMessage::MetadataTagInput(message))
}

fn cancel_metadata_tag_entry() -> super::test_support::state::GuiMessage {
    metadata_message(super::test_support::state::MetadataMessage::CancelMetadataTagEntry)
}

fn move_metadata_tag_completion(delta: i32) -> super::test_support::state::GuiMessage {
    metadata_message(super::test_support::state::MetadataMessage::MoveMetadataTagCompletion(delta))
}

fn hover_metadata_tag_completion(value: String) -> super::test_support::state::GuiMessage {
    metadata_message(super::test_support::state::MetadataMessage::HoverMetadataTagCompletion(value))
}

fn clear_metadata_tag_drop_category_unless(
    category_id: String,
) -> super::test_support::state::GuiMessage {
    metadata_message(
        super::test_support::state::MetadataMessage::ClearMetadataTagDropCategoryUnless {
            category_id,
        },
    )
}

fn select_metadata_tag_completion(value: String) -> super::test_support::state::GuiMessage {
    metadata_message(
        super::test_support::state::MetadataMessage::SelectMetadataTagCompletion(value),
    )
}

fn toggle_metadata_tag_library() -> super::test_support::state::GuiMessage {
    metadata_message(super::test_support::state::MetadataMessage::ToggleMetadataTagLibrary)
}

fn toggle_metadata_tag_category(category_id: String) -> super::test_support::state::GuiMessage {
    metadata_message(
        super::test_support::state::MetadataMessage::ToggleMetadataTagCategory(category_id),
    )
}

fn select_metadata_tag(tag: String) -> super::test_support::state::GuiMessage {
    metadata_message(super::test_support::state::MetadataMessage::SelectMetadataTag(tag))
}

fn toggle_metadata_tag(tag: String) -> super::test_support::state::GuiMessage {
    metadata_message(super::test_support::state::MetadataMessage::ToggleMetadataTag(tag))
}

fn remove_metadata_tag(tag: String) -> super::test_support::state::GuiMessage {
    metadata_message(super::test_support::state::MetadataMessage::RemoveMetadataTag(tag))
}

fn toggle_metadata_tag_for_files(
    tag: String,
    file_ids: Vec<String>,
) -> super::test_support::state::GuiMessage {
    metadata_message(
        super::test_support::state::MetadataMessage::ToggleMetadataTagForFiles { tag, file_ids },
    )
}

fn delete_selected_metadata_tag() -> super::test_support::state::GuiMessage {
    metadata_message(super::test_support::state::MetadataMessage::DeleteSelectedMetadataTag)
}

fn toggle_sample_name_view_mode() -> super::test_support::state::GuiMessage {
    metadata_message(super::test_support::state::MetadataMessage::ToggleSampleNameViewMode)
}

fn native_app_state_with_temp_sample(name: &str) -> (NativeAppState, tempfile::TempDir, String) {
    native_app_state_with_temp_sample_in(name, tempfile::tempdir().expect("source root"))
}

fn native_app_state_with_temp_sample_in(
    name: &str,
    source_root: tempfile::TempDir,
) -> (NativeAppState, tempfile::TempDir, String) {
    let mut state = gui_state_for_span_tests();
    let app_root = source_root
        .path()
        .parent()
        .expect("source parent")
        .join(format!(".wavecrate-test-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&app_root).expect("source app root");
    let file = fs::File::open(&app_root).expect("source app root open");
    let identity =
        wavecrate_library::filesystem_identity::stable_filesystem_identity_from_open_file(&file)
            .expect("source app root identity");
    state.background.waveform_recovery_root = Some(
        crate::native_app::transaction_history::operation_journal::RecoveryRootCapability {
            path: app_root,
            file: std::sync::Arc::new(file),
            identity,
        },
    );
    let sample_path = source_root.path().join(name);
    fs::write(&sample_path, []).expect("sample file");
    state.library.folder_browser =
        super::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
        ]);
    let selected_file = sample_path.display().to_string();
    state
        .library
        .folder_browser
        .select_file(selected_file.clone());
    (state, source_root, selected_file)
}

fn submit_folder_browser_rename_for_tests(state: &mut NativeAppState, value: impl Into<String>) {
    let result = state
        .library
        .folder_browser
        .apply_rename_input(radiant::widgets::TextInputMessage::Submitted {
            value: value.into(),
        })
        .expect("rename result");
    match result {
        super::sample_library::folder_browser::commands::RenameInputResult::Status(result) => {
            state.ui.status.sample = result.status;
        }
        super::sample_library::folder_browser::commands::RenameInputResult::Commit(request) => {
            let completion =
                super::sample_library::folder_browser::commands::execute_rename_commit_request(
                    request,
                );
            let mut context = ui::UiUpdateContext::default();
            state.finish_folder_browser_rename(completion, &mut context);
            run_command_for_tests(state, context.into_command());
        }
    }
}

fn run_command_for_tests(
    state: &mut NativeAppState,
    command: Command<super::test_support::state::GuiMessage>,
) {
    let continues_committed_mutation = |message: &super::test_support::state::GuiMessage| {
        matches!(
            message,
            super::test_support::state::GuiMessage::CommittedFileMutationRequested(_)
                | super::test_support::state::GuiMessage::CommittedFileMutationFinished(_)
                | super::test_support::state::GuiMessage::HistoryFileIoRequested(_)
                | super::test_support::state::GuiMessage::HistoryFileIoFinished(_)
                | super::test_support::state::GuiMessage::NormalizationFinished(_)
                | super::test_support::state::GuiMessage::ExternalWaveformFileDropFinished {
                    ..
                }
                | super::test_support::state::GuiMessage::ContextSampleSameFinished { .. }
                | super::test_support::state::GuiMessage::ContextSampleDoubleFinished { .. }
                | super::test_support::state::GuiMessage::SelectedFilesCopyFinished { .. }
                | super::test_support::state::GuiMessage::WaveformSelectionCopyFinished { .. }
                | super::test_support::state::GuiMessage::FolderMoveFinished { .. }
                | super::test_support::state::GuiMessage::FileMoveConflictFinished { .. }
                | super::test_support::state::GuiMessage::TrashMoveFinished { .. }
                | super::test_support::state::GuiMessage::FolderBrowserRenameFinished(_)
                | super::test_support::state::GuiMessage::WaveformDestructiveEditFinished(_)
                | super::test_support::state::GuiMessage::WaveformSelectionCopyExtracted { .. }
                | super::test_support::state::GuiMessage::PlaySelectionExtractionFinished { .. }
                | super::test_support::state::GuiMessage::HarvestSelectionDerivationPersisted(_)
                | super::test_support::state::GuiMessage::SelectedWholeFilesHarvestExtractionFinished {
                    ..
                }
                | super::test_support::state::GuiMessage::RatingPersisted(_)
        )
    };
    let mut commands = std::collections::VecDeque::from([command]);
    let mut processed = 0usize;
    while let Some(command) = commands.pop_front() {
        for message in run_command_and_capture_messages_for_tests(command) {
            let continues = continues_committed_mutation(&message);
            let mut context = ui::UiUpdateContext::default();
            if let super::test_support::state::GuiMessage::CommittedFileMutationRequested(work) =
                message
            {
                state.finish_committed_file_mutation_for_tests(work, &mut context);
                let followup = context.into_command();
                if continues && !followup.is_empty() {
                    commands.push_back(followup);
                }
                continue;
            }
            state.apply_message(message, &mut context);
            let followup = context.into_command();
            if continues && !followup.is_empty() {
                commands.push_back(followup);
            }
        }
        processed += 1;
        assert!(
            processed <= 1_000,
            "test command follow-up loop did not settle"
        );
    }
}

fn run_worker_message_for_tests(
    command: Command<super::test_support::state::GuiMessage>,
    target_name: &'static str,
) -> Option<super::test_support::state::GuiMessage> {
    if command.business_task_priority(target_name).is_none() {
        return None;
    }
    let messages = run_command_and_capture_messages_for_target_tests(command, target_name);
    match messages.len() {
        0 => None,
        1 => messages.into_iter().next(),
        count => panic!("worker task {target_name} emitted {count} messages; expected exactly one"),
    }
}

fn run_command_and_capture_messages_for_tests(
    command: Command<super::test_support::state::GuiMessage>,
) -> Vec<super::test_support::state::GuiMessage> {
    let mut runtime = radiant::runtime::SurfaceRuntime::new(
        CaptureBridge {
            messages: Vec::new(),
            target_name: None,
        },
        Vector2::new(900.0, 620.0),
    );
    apply_strict_update_diagnostics(&mut runtime);
    let _ = runtime.execute_command(command);
    let mut processed = 0usize;
    let mut idle_drains = 0usize;
    while idle_drains < 2 {
        processed += 1;
        assert!(processed <= 1_000, "test command capture did not settle");
        std::thread::sleep(Duration::from_millis(1));
        let outcome = runtime.drain_runtime_messages();
        if outcome.messages_dispatched == 0 && !outcome.runtime_work_remaining {
            idle_drains += 1;
        } else {
            idle_drains = 0;
        }
    }
    runtime.into_bridge().messages
}

fn run_command_and_capture_messages_for_target_tests(
    command: Command<super::test_support::state::GuiMessage>,
    target_name: &'static str,
) -> Vec<super::test_support::state::GuiMessage> {
    let mut runtime = radiant::runtime::SurfaceRuntime::new(
        CaptureBridge {
            messages: Vec::new(),
            target_name: Some(target_name),
        },
        Vector2::new(900.0, 620.0),
    );
    apply_strict_update_diagnostics(&mut runtime);
    let _ = runtime.execute_command(command);
    let mut processed = 0usize;
    let mut idle_drains = 0usize;
    while idle_drains < 2 {
        processed += 1;
        assert!(processed <= 1_000, "targeted worker capture did not settle");
        std::thread::sleep(Duration::from_millis(1));
        let outcome = runtime.drain_runtime_messages();
        if outcome.messages_dispatched == 0 && !outcome.runtime_work_remaining {
            idle_drains += 1;
        } else {
            idle_drains = 0;
        }
    }
    runtime.into_bridge().messages
}

#[test]
fn targeted_worker_capture_returns_requested_task_and_skips_later_decoy() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    let decoy_ran = Arc::new(AtomicBool::new(false));
    let mut context = ui::UiUpdateContext::default();
    context
        .business()
        .background("capture-target")
        .run(|_| 7_u8, |_| super::test_support::state::GuiMessage::Frame);
    let decoy_ran_in_worker = Arc::clone(&decoy_ran);
    context.business().background("capture-decoy").run(
        move |_| {
            decoy_ran_in_worker.store(true, Ordering::SeqCst);
            9_u8
        },
        |_| super::test_support::state::GuiMessage::ToggleShortcutHelp,
    );

    let messages =
        run_command_and_capture_messages_for_target_tests(context.into_command(), "capture-target");
    assert_eq!(
        messages,
        vec![super::test_support::state::GuiMessage::Frame]
    );
    assert!(!decoy_ran.load(Ordering::SeqCst));
}

pub(crate) fn run_command_collecting_followups_for_tests(
    state: &mut NativeAppState,
    command: Command<super::test_support::state::GuiMessage>,
) -> Vec<Command<super::test_support::state::GuiMessage>> {
    let mut followups = Vec::new();
    for message in run_command_and_capture_messages_for_tests(command) {
        let mut context = ui::UiUpdateContext::default();
        state.apply_message(message, &mut context);
        let followup = context.into_command();
        if !followup.is_empty() {
            followups.push(followup);
        }
    }
    followups
}

fn start_deferred_sample_load_for_tests(
    state: &mut NativeAppState,
    path: String,
    autoplay: bool,
    context: &mut ui::UiUpdateContext<super::test_support::state::GuiMessage>,
) {
    let Some(ticket) = state.background.deferred_sample_load_task.active() else {
        assert!(
            state.active_sample_load_task().is_some(),
            "sample load queued"
        );
        return;
    };
    state.apply_message(
        super::test_support::state::GuiMessage::DeferredSampleLoad {
            ticket,
            path,
            autoplay,
            check_cache: false,
            scheduled_at: std::time::Instant::now(),
        },
        context,
    );
}

fn temp_gui_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "{name}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    fs::create_dir_all(&root).expect("create temp root");
    root
}

fn write_test_wav_i16(path: &std::path::Path, samples: &[i16]) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 48_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).expect("create wav");
    for sample in samples {
        writer.write_sample(*sample).expect("write sample");
    }
    writer.finalize().expect("finalize wav");
}

fn write_sparse_test_wav_i16(path: &std::path::Path, channels: u16, frames: u32) {
    let channels = channels.max(1);
    let sample_rate = 48_000_u32;
    let bits_per_sample = 16_u16;
    let block_align = channels * (bits_per_sample / 8);
    let byte_rate = sample_rate * u32::from(block_align);
    let data_bytes = frames
        .checked_mul(u32::from(block_align))
        .expect("test wav data size");
    let riff_size = 36_u32.checked_add(data_bytes).expect("test wav riff size");
    let mut file = fs::File::create(path).expect("create sparse wav");
    use std::io::Write;
    file.write_all(b"RIFF").expect("write riff");
    file.write_all(&riff_size.to_le_bytes())
        .expect("write riff size");
    file.write_all(b"WAVE").expect("write wave");
    file.write_all(b"fmt ").expect("write fmt");
    file.write_all(&16_u32.to_le_bytes())
        .expect("write fmt size");
    file.write_all(&1_u16.to_le_bytes())
        .expect("write pcm format");
    file.write_all(&channels.to_le_bytes())
        .expect("write channels");
    file.write_all(&sample_rate.to_le_bytes())
        .expect("write sample rate");
    file.write_all(&byte_rate.to_le_bytes())
        .expect("write byte rate");
    file.write_all(&block_align.to_le_bytes())
        .expect("write block align");
    file.write_all(&bits_per_sample.to_le_bytes())
        .expect("write bits");
    file.write_all(b"data").expect("write data chunk");
    file.write_all(&data_bytes.to_le_bytes())
        .expect("write data size");
    file.set_len(44_u64 + u64::from(data_bytes))
        .expect("extend sparse wav");
}

fn read_test_wav_i16(path: &std::path::Path) -> Vec<i16> {
    let mut reader = hound::WavReader::open(path).expect("open wav");
    reader
        .samples::<i16>()
        .collect::<Result<Vec<_>, _>>()
        .expect("read i16 samples")
}

fn read_test_wav_f32(path: &std::path::Path) -> Vec<f32> {
    let mut reader = hound::WavReader::open(path).expect("open wav");
    let spec = reader.spec();
    match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<Vec<_>, _>>()
            .expect("read float samples"),
        hound::SampleFormat::Int => {
            let scale = (1_i64 << spec.bits_per_sample.saturating_sub(1)).max(1) as f32;
            reader
                .samples::<i32>()
                .map(|sample| sample.map(|value| value as f32 / scale))
                .collect::<Result<Vec<_>, _>>()
                .expect("read int samples")
        }
    }
}

fn frame_has_clip_height(frame: &SurfaceFrame, expected: f32) -> bool {
    frame
        .paint_plan
        .clip_starts()
        .any(|clip| (clip.rect.height() - expected).abs() < 0.01)
}

fn text_input_widget_id(frame: &SurfaceFrame) -> Option<u64> {
    metadata_tag_text_input(frame).map(|input| input.widget_id)
}

fn metadata_tag_text_input(frame: &SurfaceFrame) -> Option<&PaintTextInput> {
    frame.paint_plan.text_inputs().find(|input| {
        input.widget_id == crate::native_app::test_support::metadata_sidebar::METADATA_TAG_INPUT_ID
    })
}
