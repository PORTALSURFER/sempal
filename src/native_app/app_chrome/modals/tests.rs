use super::projection::TransactionListProjection;
use crate::native_app::app::UnsupportedFilesDialogState;
use crate::native_app::test_support::state::NativeAppState;
use radiant::prelude::{self as ui, IntoView};
use std::path::PathBuf;

#[test]
fn transaction_list_projection_formats_summary_and_rows() {
    let mut state = NativeAppState::load_default().expect("default state loads");

    let empty = TransactionListProjection::from_state(&state);
    assert_eq!(empty.summary, "no undo | no redo | closed");
    assert!(empty.rows.is_empty());

    state.register_transaction_action("Rename sample", |_| Ok(()), |_| Ok(()));
    state.begin_transaction("Open batch");
    state.register_transaction_action("First action", |_| Ok(()), |_| Ok(()));

    let projection = TransactionListProjection::from_state(&state);
    assert_eq!(
        projection.summary,
        "undo ready | no redo | open transaction"
    );
    assert_eq!(projection.rows.len(), 2);
    assert_eq!(projection.rows[0].order_label, "Draft");
    assert_eq!(projection.rows[0].label, "Open batch");
    assert_eq!(projection.rows[0].action_summary, "1 action: First action");
    assert_eq!(projection.rows[0].state.label(), "Open");
    assert_eq!(projection.rows[1].order_label, "#1");
    assert_eq!(projection.rows[1].label, "Rename sample");
    assert_eq!(projection.rows[1].action_summary, "1 action: Rename sample");
    assert_eq!(projection.rows[1].state.label(), "Undo");
}

#[test]
fn transaction_list_modal_uses_registered_modal_identity() {
    let mut state = NativeAppState::load_default().expect("default state loads");
    state.ui.chrome.transaction_list_open = true;

    let frame = crate::native_app::app_chrome::modals::transaction_list(&state)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(520.0, 360.0));

    assert!(
        frame
            .layout
            .rects
            .contains_key(&super::identity::TRANSACTION_LIST_MODAL_ID),
        "transaction list modal should keep the registered automation/test id"
    );
}

#[test]
fn trash_folder_setup_modal_explains_recovery_and_offers_choice() {
    let frame = crate::native_app::app_chrome::modals::trash_folder_setup()
        .view_frame_at_size_with_default_theme(ui::Vector2::new(560.0, 240.0));

    assert!(frame.paint_plan.contains_text("Trash Folder Required"));
    assert!(frame.paint_plan.contains_text("Choose folder"));
    assert!(frame.paint_plan.contains_text("Cancel"));
}

#[test]
fn unsupported_files_modal_paints_authoritative_paths_and_safe_actions() {
    let mut state = NativeAppState::load_default().expect("default state loads");
    state.ui.browser_interaction.unsupported_files_dialog = Some(UnsupportedFilesDialogState {
        source_id: String::from("source"),
        source_label: String::from("Samples"),
        loading: false,
        paths: vec![PathBuf::from("/samples/broken.aiff")],
        error: None,
    });

    let frame = crate::native_app::app_chrome::modals::unsupported_files(&state)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(820.0, 540.0));

    assert!(frame.paint_plan.contains_text("/samples/broken.aiff"));
    assert!(frame.paint_plan.contains_text("Reveal"));
    assert!(frame.paint_plan.contains_text("Move to Trash"));
}
