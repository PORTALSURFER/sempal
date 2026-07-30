use super::gui_state_for_span_tests;
use crate::native_app::sample_library::committed_file_mutations::PreparedCommittedFileMutationChange;
use crate::native_app::transaction_history::{
    HistoryFileAction, HistoryFileIoDirection, HistoryFileIoOutput, HistoryFileIoResult,
};
use crate::native_app::{
    test_support::state::{GuiMessage, WaveformInteraction},
    waveform::{
        PlaymarkLabelMessage, WaveformEditFadeHandle, WaveformEditFadeOuterGainHandle,
        WaveformSelectionEdge, WaveformSelectionKind,
    },
};
use radiant::prelude::{self as ui, IntoView};
use wavecrate::selection::SelectionRange;

#[test]
fn undo_file_history_without_source_restores_stack_and_clears_in_flight() {
    let mut state = gui_state_for_span_tests();
    state.register_file_transaction_action(
        "Missing source move",
        HistoryFileAction::FolderMove {
            source_root: "/missing".into(),
            source_database_root: "/missing/.db".into(),
            moves: vec![("/missing/old".into(), "/missing/new".into())],
        },
        HistoryFileAction::FolderMove {
            source_root: "/missing".into(),
            source_database_root: "/missing/.db".into(),
            moves: vec![("/missing/new".into(), "/missing/old".into())],
        },
    );
    let mut context = ui::UiUpdateContext::default();
    state.undo_transaction(&mut context);
    state.finish_history_file_io(
        HistoryFileIoResult {
            execution_id: 1,
            transaction_id: 1,
            direction: HistoryFileIoDirection::Undo,
            through_target: None,
            result: Ok(HistoryFileIoOutput {
                changes: vec![PreparedCommittedFileMutationChange::created(
                    "/missing/new".into(),
                    wavecrate::sample_sources::SourceFileEvidence::Missing,
                )],
                failures: Vec::new(),
                waveform_paths: Vec::new(),
            }),
        },
        &mut context,
    );
    assert!(state.transactions.history.can_undo());
    assert!(!state.transactions.history.file_io_in_flight());
    assert!(state.transactions.pending_history_commit.is_none());
}

#[test]
fn transaction_group_undoes_and_redoes_as_one_entry() {
    let mut state = gui_state_for_span_tests();
    state.audio.volume = 0.1;
    state.begin_transaction("Grouped edit");
    state.register_transaction_action(
        "first",
        |transaction| {
            transaction.set_audio_volume(0.1);
            Ok(())
        },
        |transaction| {
            transaction.set_audio_volume(0.4);
            Ok(())
        },
    );
    state.register_transaction_action(
        "second",
        |transaction| {
            transaction.set_audio_volume(0.4);
            Ok(())
        },
        |transaction| {
            transaction.set_audio_volume(0.8);
            Ok(())
        },
    );
    assert!(state.commit_transaction());

    state.audio.volume = 0.8;
    state.undo_transaction(&mut radiant::prelude::UiUpdateContext::default());
    assert_eq!(state.ui.status.sample, "Undid Grouped edit");
    assert_eq!(state.audio.volume, 0.1);
    state.redo_transaction(&mut radiant::prelude::UiUpdateContext::default());
    assert_eq!(state.ui.status.sample, "Redid Grouped edit");
    assert_eq!(state.audio.volume, 0.8);
}

#[test]
fn playmark_label_length_commit_registers_one_undoable_frame_exact_transaction() {
    let mut state = gui_state_for_span_tests();
    let mut context = ui::UiUpdateContext::default();
    let total_frames = state.waveform.current.frames();
    let before = SelectionRange::from_frame_bounds(total_frames, 12_000, 18_000);
    state
        .waveform
        .current
        .set_play_selection_range(before.start(), before.end());

    state.apply_message(
        GuiMessage::PlaymarkLabel(PlaymarkLabelMessage::BeginEdit),
        &mut context,
    );
    state.apply_message(
        GuiMessage::PlaymarkLabel(PlaymarkLabelMessage::Commit(String::from("500ms"))),
        &mut context,
    );

    let after = state
        .waveform
        .current
        .play_selection()
        .expect("playmark selection after label commit");
    assert_eq!(
        after.frame_bounds(total_frames).end_frame - after.frame_bounds(total_frames).start_frame,
        24_000
    );
    let items = state.transactions.history.list_items();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label, "Change play mark selection");

    state.apply_message(GuiMessage::UndoTransaction, &mut context);
    assert_eq!(state.waveform.current.play_selection(), Some(before));
    state.apply_message(GuiMessage::RedoTransaction, &mut context);
    assert_eq!(state.waveform.current.play_selection(), Some(after));
}

#[test]
fn waveform_fade_drag_registers_one_transaction() {
    let mut state = gui_state_for_span_tests();
    let mut context = ui::UiUpdateContext::default();
    let before = SelectionRange::new(0.2, 0.6).with_fade_out(0.25, 0.2);
    state.waveform.current.set_edit_selection_range(before);

    state.apply_message(
        GuiMessage::Waveform(WaveformInteraction::BeginEditFade {
            handle: WaveformEditFadeHandle::OutStart,
            visible_ratio: 0.5,
        }),
        &mut context,
    );
    state.apply_message(
        GuiMessage::Waveform(WaveformInteraction::UpdateSelection {
            visible_ratio: 0.45,
        }),
        &mut context,
    );
    state.apply_message(
        GuiMessage::Waveform(WaveformInteraction::FinishSelection {
            visible_ratio: 0.45,
        }),
        &mut context,
    );

    let after = state
        .waveform
        .current
        .edit_selection()
        .expect("edit selection after fade drag");
    assert_ne!(after, before);
    let items = state.transactions.history.list_items();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label, "Waveform fade");
    assert_eq!(items[0].action_labels, vec![String::from("Waveform fade")]);

    state.apply_message(GuiMessage::UndoTransaction, &mut context);
    assert_eq!(state.waveform.current.edit_selection(), Some(before));
    assert_eq!(state.ui.status.sample, "Undid Waveform fade");

    state.apply_message(GuiMessage::RedoTransaction, &mut context);
    assert_eq!(state.waveform.current.edit_selection(), Some(after));
    assert_eq!(state.ui.status.sample, "Redid Waveform fade");
}

#[test]
fn waveform_fade_outer_gain_drag_registers_one_transaction() {
    let mut state = gui_state_for_span_tests();
    let mut context = ui::UiUpdateContext::default();
    let before = SelectionRange::new(0.2, 0.6)
        .with_fade_in(0.25, 0.2)
        .with_fade_in_mute(0.2)
        .with_fade_in_outer_gain(0.25);
    state.waveform.current.set_edit_selection_range(before);

    state.apply_message(
        GuiMessage::Waveform(WaveformInteraction::BeginEditFadeOuterGain {
            handle: WaveformEditFadeOuterGainHandle::In,
            vertical_ratio: 0.25,
        }),
        &mut context,
    );
    state.apply_message(
        GuiMessage::Waveform(WaveformInteraction::UpdateEditFadeOuterGain {
            vertical_ratio: 0.5,
        }),
        &mut context,
    );
    state.apply_message(
        GuiMessage::Waveform(WaveformInteraction::FinishEditFadeOuterGain {
            vertical_ratio: 0.5,
        }),
        &mut context,
    );

    let after = state
        .waveform
        .current
        .edit_selection()
        .expect("edit selection after outer gain drag");
    assert_ne!(after, before);
    assert_eq!(state.transactions.history.list_items().len(), 1);

    state.apply_message(GuiMessage::UndoTransaction, &mut context);
    assert_eq!(state.waveform.current.edit_selection(), Some(before));

    state.apply_message(GuiMessage::RedoTransaction, &mut context);
    assert_eq!(state.waveform.current.edit_selection(), Some(after));
}

#[test]
fn waveform_edit_gain_drag_registers_one_transaction() {
    let mut state = gui_state_for_span_tests();
    let mut context = ui::UiUpdateContext::default();
    let before = SelectionRange::new(0.2, 0.6).with_gain(0.5);
    state.waveform.current.set_edit_selection_range(before);

    state.apply_message(
        GuiMessage::Waveform(WaveformInteraction::BeginEditGain { pointer_y: 20.0 }),
        &mut context,
    );
    state.apply_message(
        GuiMessage::Waveform(WaveformInteraction::UpdateEditGain { pointer_y: -20.0 }),
        &mut context,
    );
    state.apply_message(
        GuiMessage::Waveform(WaveformInteraction::UpdateEditGain { pointer_y: 80.0 }),
        &mut context,
    );
    assert!(
        state.transactions.history.list_items().is_empty(),
        "live preview updates should not create undo history entries"
    );

    state.apply_message(
        GuiMessage::Waveform(WaveformInteraction::FinishEditGain { pointer_y: 80.0 }),
        &mut context,
    );

    let after = state
        .waveform
        .current
        .edit_selection()
        .expect("edit selection after gain drag");
    assert_ne!(after, before);
    let items = state.transactions.history.list_items();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label, "Editmark volume");
    assert_eq!(
        items[0].action_labels,
        vec![String::from("Editmark volume")]
    );

    state.apply_message(GuiMessage::UndoTransaction, &mut context);
    assert_eq!(state.waveform.current.edit_selection(), Some(before));
    assert_eq!(state.ui.status.sample, "Undid Editmark volume");

    state.apply_message(GuiMessage::RedoTransaction, &mut context);
    assert_eq!(state.waveform.current.edit_selection(), Some(after));
    assert_eq!(state.ui.status.sample, "Redid Editmark volume");
}

#[test]
fn editmark_resize_drag_registers_one_transaction() {
    let mut state = gui_state_for_span_tests();
    let mut context = ui::UiUpdateContext::default();
    let before = SelectionRange::new(0.2, 0.6)
        .with_gain(0.5)
        .with_fade_in(0.25, 0.2);
    state.waveform.current.set_edit_selection_range(before);

    state.apply_message(
        GuiMessage::Waveform(WaveformInteraction::BeginSelectionResize {
            kind: WaveformSelectionKind::Edit,
            edge: WaveformSelectionEdge::End,
            visible_ratio: 0.6,
        }),
        &mut context,
    );
    state.apply_message(
        GuiMessage::Waveform(WaveformInteraction::UpdateSelection { visible_ratio: 0.7 }),
        &mut context,
    );
    state.apply_message(
        GuiMessage::Waveform(WaveformInteraction::UpdateSelection { visible_ratio: 0.8 }),
        &mut context,
    );
    assert!(
        state.transactions.history.list_items().is_empty(),
        "live resize preview updates should not create undo history entries"
    );

    state.apply_message(
        GuiMessage::Waveform(WaveformInteraction::FinishSelection { visible_ratio: 0.8 }),
        &mut context,
    );

    let after = state
        .waveform
        .current
        .edit_selection()
        .expect("edit selection after resize");
    assert_ne!(after, before);
    assert!((after.start() - 0.2).abs() < 0.001);
    assert!((after.end() - 0.8).abs() < 0.001);
    assert!((after.gain() - 0.5).abs() < 0.001);
    let items = state.transactions.history.list_items();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label, "Editmark resize");
    assert_eq!(
        items[0].action_labels,
        vec![String::from("Editmark resize")]
    );

    state.apply_message(GuiMessage::UndoTransaction, &mut context);
    assert_eq!(state.waveform.current.edit_selection(), Some(before));
    assert_eq!(state.ui.status.sample, "Undid Editmark resize");

    state.apply_message(GuiMessage::RedoTransaction, &mut context);
    assert_eq!(state.waveform.current.edit_selection(), Some(after));
    assert_eq!(state.ui.status.sample, "Redid Editmark resize");
}

#[test]
fn editmark_move_drag_registers_one_transaction() {
    let mut state = gui_state_for_span_tests();
    let mut context = ui::UiUpdateContext::default();
    let before = SelectionRange::new(0.2, 0.6).with_fade_out(0.25, 0.7);
    state.waveform.current.set_edit_selection_range(before);

    state.apply_message(
        GuiMessage::Waveform(WaveformInteraction::BeginSelectionMove {
            kind: WaveformSelectionKind::Edit,
            visible_ratio: 0.4,
        }),
        &mut context,
    );
    state.apply_message(
        GuiMessage::Waveform(WaveformInteraction::UpdateSelection { visible_ratio: 0.5 }),
        &mut context,
    );
    assert!(
        state.transactions.history.list_items().is_empty(),
        "live move preview updates should not create undo history entries"
    );
    state.apply_message(
        GuiMessage::Waveform(WaveformInteraction::FinishSelection { visible_ratio: 0.5 }),
        &mut context,
    );

    let after = state
        .waveform
        .current
        .edit_selection()
        .expect("edit selection after move");
    assert_ne!(after, before);
    let items = state.transactions.history.list_items();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label, "Editmark move");

    state.apply_message(GuiMessage::UndoTransaction, &mut context);
    assert_eq!(state.waveform.current.edit_selection(), Some(before));

    state.apply_message(GuiMessage::RedoTransaction, &mut context);
    assert_eq!(state.waveform.current.edit_selection(), Some(after));
}

#[test]
fn no_op_editmark_resize_drag_does_not_register_transaction() {
    let mut state = gui_state_for_span_tests();
    let mut context = ui::UiUpdateContext::default();
    let selection = SelectionRange::new(0.2, 0.6);
    state.waveform.current.set_edit_selection_range(selection);

    state.apply_message(
        GuiMessage::Waveform(WaveformInteraction::BeginSelectionResize {
            kind: WaveformSelectionKind::Edit,
            edge: WaveformSelectionEdge::End,
            visible_ratio: 0.6,
        }),
        &mut context,
    );
    state.apply_message(
        GuiMessage::Waveform(WaveformInteraction::FinishSelection { visible_ratio: 0.6 }),
        &mut context,
    );

    assert_eq!(state.waveform.current.edit_selection(), Some(selection));
    assert!(state.transactions.history.list_items().is_empty());
}

#[test]
fn editmark_resize_transaction_preserves_boundary_validation() {
    let mut state = gui_state_for_span_tests();
    let mut context = ui::UiUpdateContext::default();
    let before = SelectionRange::new(0.2, 0.6).with_gain(0.5);
    state.waveform.current.set_edit_selection_range(before);

    state.apply_message(
        GuiMessage::Waveform(WaveformInteraction::BeginSelectionResize {
            kind: WaveformSelectionKind::Edit,
            edge: WaveformSelectionEdge::Start,
            visible_ratio: 0.2,
        }),
        &mut context,
    );
    state.apply_message(
        GuiMessage::Waveform(WaveformInteraction::FinishSelection {
            visible_ratio: -0.5,
        }),
        &mut context,
    );

    let after = state
        .waveform
        .current
        .edit_selection()
        .expect("clamped edit selection after resize");
    assert!((after.start() - 0.0).abs() < 0.001);
    assert!((after.end() - 0.6).abs() < 0.001);
    assert!((after.gain() - 0.5).abs() < 0.001);
    assert_eq!(state.transactions.history.list_items().len(), 1);

    state.apply_message(GuiMessage::UndoTransaction, &mut context);
    assert_eq!(state.waveform.current.edit_selection(), Some(before));

    state.apply_message(GuiMessage::RedoTransaction, &mut context);
    assert_eq!(state.waveform.current.edit_selection(), Some(after));
}

#[test]
fn no_op_waveform_edit_gain_drag_does_not_register_transaction() {
    let mut state = gui_state_for_span_tests();
    let mut context = ui::UiUpdateContext::default();
    let selection = SelectionRange::new(0.2, 0.6).with_gain(0.5);
    state.waveform.current.set_edit_selection_range(selection);

    state.apply_message(
        GuiMessage::Waveform(WaveformInteraction::BeginEditGain { pointer_y: 20.0 }),
        &mut context,
    );
    state.apply_message(
        GuiMessage::Waveform(WaveformInteraction::FinishEditGain { pointer_y: 20.0 }),
        &mut context,
    );

    assert_eq!(state.waveform.current.edit_selection(), Some(selection));
    assert!(state.transactions.history.list_items().is_empty());
}

#[test]
fn no_op_waveform_fade_clear_silence_does_not_register_transaction() {
    let mut state = gui_state_for_span_tests();
    let mut context = ui::UiUpdateContext::default();
    let selection = SelectionRange::new(0.2, 0.6).with_fade_out(0.25, 0.2);
    state.waveform.current.set_edit_selection_range(selection);

    state.apply_message(
        GuiMessage::Waveform(WaveformInteraction::ClearEditFadeSilence {
            handle: WaveformEditFadeHandle::OutOuterEnd,
        }),
        &mut context,
    );

    assert_eq!(state.waveform.current.edit_selection(), Some(selection));
    assert!(state.transactions.history.list_items().is_empty());
}

#[test]
fn transaction_list_modal_open_close_updates_chrome_state() {
    let mut state = gui_state_for_span_tests();
    let mut context = ui::UiUpdateContext::default();

    assert!(!state.ui.chrome.transaction_list_open);
    state.apply_message(GuiMessage::ToggleTransactionList, &mut context);
    assert!(state.ui.chrome.transaction_list_open);

    state.apply_message(GuiMessage::CloseTransactionList, &mut context);
    assert!(!state.ui.chrome.transaction_list_open);
}

#[test]
fn transaction_list_target_undo_and_redo_walk_through_selected_row() {
    let mut state = gui_state_for_span_tests();
    let mut context = ui::UiUpdateContext::default();
    state.audio.volume = 0.3;
    state.register_transaction_action(
        "First volume",
        |transaction| {
            transaction.set_audio_volume(0.0);
            Ok(())
        },
        |transaction| {
            transaction.set_audio_volume(0.1);
            Ok(())
        },
    );
    state.register_transaction_action(
        "Second volume",
        |transaction| {
            transaction.set_audio_volume(0.1);
            Ok(())
        },
        |transaction| {
            transaction.set_audio_volume(0.2);
            Ok(())
        },
    );
    state.register_transaction_action(
        "Third volume",
        |transaction| {
            transaction.set_audio_volume(0.2);
            Ok(())
        },
        |transaction| {
            transaction.set_audio_volume(0.3);
            Ok(())
        },
    );

    state.apply_message(GuiMessage::UndoTransactionsThrough(2), &mut context);

    assert_eq!(state.audio.volume, 0.1);
    assert_eq!(state.ui.status.sample, "Undid 2 through Second volume");

    state.apply_message(GuiMessage::RedoTransactionsThrough(3), &mut context);

    assert_eq!(state.audio.volume, 0.3);
    assert_eq!(state.ui.status.sample, "Redid 2 through Third volume");
}

#[test]
fn transaction_list_modal_renders_registered_transactions() {
    let mut state = gui_state_for_span_tests();
    state.ui.chrome.transaction_list_open = true;
    state.register_transaction_action("Rename sample", |_| Ok(()), |_| Ok(()));
    state.begin_transaction("Open batch");
    state.register_transaction_action("First action", |_| Ok(()), |_| Ok(()));

    let frame = crate::native_app::test_support::state::view(&state)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(960.0, 540.0));

    assert!(frame.paint_plan.contains_text("Transactions"));
    assert!(frame.paint_plan.contains_text("Rename sample"));
    assert!(frame.paint_plan.contains_text("Open batch"));
    assert!(frame.paint_plan.contains_text("Open"));
    assert!(frame.paint_plan.contains_text("Undo"));
}
