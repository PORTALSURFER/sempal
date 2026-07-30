mod identity;
mod projection;
#[cfg(test)]
mod tests;

use crate::native_app::{
    app::{
        FileMoveConflictResolution, FileMoveConflictResolutionRequest, GuiMessage,
        ShortcutHelpItem, ShortcutHelpSection,
    },
    transaction_history::TransactionListState,
};
use radiant::prelude as ui;

use self::projection::{
    FileMoveConflictProjection, FolderDeleteConfirmationProjection,
    ProtectedExtractionTargetSourceProjection, ShortcutHelpProjection, TransactionListProjection,
    TransactionListRowProjection, WaveformDestructiveEditProjection,
};
use crate::native_app::app::NativeAppState;

const SHORTCUT_HELP_MODAL_WIDTH: f32 = 860.0;
const SHORTCUT_HELP_MODAL_HEIGHT: f32 = 640.0;
const SHORTCUT_HELP_KEY_WIDTH: f32 = 190.0;
const TRANSACTION_LIST_MODAL_WIDTH: f32 = 760.0;
const TRANSACTION_LIST_MODAL_HEIGHT: f32 = 520.0;
const TRANSACTION_LIST_ACTION_WIDTH: f32 = 70.0;
const UNSUPPORTED_FILE_ACTION_WIDTH: f32 = 112.0;

pub(in crate::native_app) fn transaction_list(state: &NativeAppState) -> ui::View<GuiMessage> {
    let projection = TransactionListProjection::from_state(state);
    let summary = transaction_list_summary(&projection);
    let list = if projection.rows.is_empty() {
        ui::column([
            ui::text_line(projection.empty_title, 24.0).fill_width(),
            ui::text_line(projection.empty_detail, 22.0).fill_width(),
        ])
        .spacing(4.0)
        .fill_width()
    } else {
        ui::scroll(
            ui::column(
                projection
                    .rows
                    .into_iter()
                    .map(transaction_list_row)
                    .collect::<Vec<_>>(),
            )
            .spacing(4.0)
            .fill_width(),
        )
        .fill_width()
        .fill_height()
    };
    let content = ui::column([summary, list])
        .spacing(6.0)
        .fill_width()
        .fill_height();

    ui::closeable_dialog_layer(
        "Transactions",
        content,
        ui::WidgetTone::Neutral,
        ui::Vector2::new(TRANSACTION_LIST_MODAL_WIDTH, TRANSACTION_LIST_MODAL_HEIGHT),
        GuiMessage::CloseTransactionList,
    )
    .id(identity::TRANSACTION_LIST_MODAL_ID)
}

pub(in crate::native_app) fn shortcut_help(state: &NativeAppState) -> ui::View<GuiMessage> {
    let projection = ShortcutHelpProjection::from_state(state);
    let body = ui::scroll(
        ui::column(
            projection
                .sections
                .into_iter()
                .map(shortcut_help_section_view)
                .collect::<Vec<_>>(),
        )
        .spacing(8.0)
        .fill_width(),
    )
    .fill_width()
    .fill_height();
    let content = ui::column([
        ui::text_line(projection.intro, 20.0)
            .muted_text()
            .fill_width(),
        body,
    ])
    .spacing(6.0)
    .fill_width()
    .fill_height();

    ui::closeable_dialog_layer(
        "Shortcuts",
        content,
        ui::WidgetTone::Neutral,
        ui::Vector2::new(SHORTCUT_HELP_MODAL_WIDTH, SHORTCUT_HELP_MODAL_HEIGHT),
        GuiMessage::CloseShortcutHelp,
    )
    .key(identity::SHORTCUT_HELP_MODAL_KEY)
}

pub(in crate::native_app) fn file_move_conflict(state: &NativeAppState) -> ui::View<GuiMessage> {
    let projection = FileMoveConflictProjection::from_state(state);
    let apply_to_remaining = projection.apply_to_remaining;
    let content = ui::column([
        ui::text_line(projection.summary, 22.0).fill_width(),
        ui::text_line(projection.file_name, 24.0).fill_width(),
        ui::text_line(projection.destination, 20.0).fill_width(),
        ui::row([
            ui::checkbox(apply_to_remaining)
                .message(GuiMessage::SetFileMoveConflictApplyToRemaining)
                .width(20.0)
                .height(20.0),
            ui::text_line("Apply to all remaining conflicts", 20.0).fill_width(),
        ])
        .spacing(6.0)
        .fill_width()
        .height(22.0),
        ui::button_row([
            ui::button("Overwrite")
                .danger()
                .message(GuiMessage::ResolveFileMoveConflict(
                    FileMoveConflictResolutionRequest::new(
                        FileMoveConflictResolution::Overwrite,
                        apply_to_remaining,
                    ),
                ))
                .width(92.0),
            ui::button("Rename")
                .primary()
                .message(GuiMessage::ResolveFileMoveConflict(
                    FileMoveConflictResolutionRequest::new(
                        FileMoveConflictResolution::Rename,
                        apply_to_remaining,
                    ),
                ))
                .width(78.0),
            ui::button("Skip")
                .message(GuiMessage::ResolveFileMoveConflict(
                    FileMoveConflictResolutionRequest::new(
                        FileMoveConflictResolution::Skip,
                        apply_to_remaining,
                    ),
                ))
                .width(64.0),
        ]),
    ])
    .spacing(6.0)
    .fill_width()
    .fill_height();

    ui::closeable_dialog_layer(
        "File Move Conflict",
        content,
        ui::WidgetTone::Warning,
        ui::Vector2::new(430.0, 210.0),
        GuiMessage::CancelFileMoveConflicts,
    )
    .key(identity::FILE_MOVE_CONFLICT_MODAL_KEY)
}

pub(in crate::native_app) fn folder_delete_confirmation(
    state: &NativeAppState,
) -> ui::View<GuiMessage> {
    let projection = FolderDeleteConfirmationProjection::from_state(state);
    let content = ui::column([
        ui::text_line(projection.name, 24.0).fill_width(),
        ui::text_line(projection.question, 20.0).fill_width(),
        ui::text_line(projection.detail, 20.0).fill_width(),
        ui::button_row([
            ui::button("Delete Folder")
                .danger()
                .message(GuiMessage::ConfirmContextFolderDelete)
                .width(112.0),
            ui::button("Cancel")
                .message(GuiMessage::CancelContextFolderDelete)
                .width(72.0),
        ]),
    ])
    .spacing(6.0)
    .fill_width()
    .fill_height();

    ui::closeable_dialog_layer(
        "Delete Folder",
        content,
        ui::WidgetTone::Warning,
        ui::Vector2::new(440.0, 190.0),
        GuiMessage::CancelContextFolderDelete,
    )
    .key(identity::FOLDER_DELETE_CONFIRMATION_MODAL_KEY)
}

pub(in crate::native_app) fn waveform_destructive_edit_confirmation(
    state: &NativeAppState,
) -> ui::View<GuiMessage> {
    let projection = WaveformDestructiveEditProjection::from_state(state);
    let content = ui::column([
        ui::text_line(projection.title, 24.0).fill_width(),
        ui::text_line(projection.message, 20.0).fill_width(),
        ui::button_row([
            ui::button("Apply Edit")
                .danger()
                .message(GuiMessage::ConfirmPendingWaveformDestructiveEdit)
                .width(92.0),
            ui::button("Cancel")
                .message(GuiMessage::CancelPendingWaveformDestructiveEdit)
                .width(72.0),
        ]),
    ])
    .spacing(6.0)
    .fill_width()
    .fill_height();

    ui::closeable_dialog_layer(
        "Destructive Edit",
        content,
        ui::WidgetTone::Warning,
        ui::Vector2::new(500.0, 190.0),
        GuiMessage::CancelPendingWaveformDestructiveEdit,
    )
    .key(identity::WAVEFORM_DESTRUCTIVE_EDIT_MODAL_KEY)
}

pub(in crate::native_app) fn protected_extraction_target_source(
    state: &NativeAppState,
) -> ui::View<GuiMessage> {
    let projection = ProtectedExtractionTargetSourceProjection::from_state(state);
    let content = ui::column([
        ui::text_line(projection.title, 24.0).fill_width(),
        ui::text_line(projection.message, 20.0).fill_width(),
        ui::button_row([
            ui::button("Add Target Source")
                .primary()
                .message(GuiMessage::AddProtectedExtractionTargetSource)
                .width(142.0),
            ui::button("Cancel")
                .message(GuiMessage::CancelProtectedExtractionTargetSource)
                .width(72.0),
        ]),
    ])
    .spacing(6.0)
    .fill_width()
    .fill_height();

    ui::closeable_dialog_layer(
        "Target Source Required",
        content,
        ui::WidgetTone::Warning,
        ui::Vector2::new(560.0, 190.0),
        GuiMessage::CancelProtectedExtractionTargetSource,
    )
    .key(identity::PROTECTED_EXTRACTION_TARGET_SOURCE_MODAL_KEY)
}

pub(in crate::native_app) fn trash_folder_setup() -> ui::View<GuiMessage> {
    let content = ui::column([
        ui::text_line(
            "Choose a trash folder before moving selected samples to trash.",
            22.0,
        )
        .fill_width(),
        ui::text_line(
            "The selected samples will be moved after the folder is saved.",
            20.0,
        )
        .fill_width(),
        ui::button_row([
            ui::button("Choose folder")
                .primary()
                .message(GuiMessage::ChooseTrashFolderForPendingMove)
                .width(116.0),
            ui::button("Cancel")
                .message(GuiMessage::CancelTrashFolderSetup)
                .width(72.0),
        ]),
    ])
    .spacing(6.0)
    .fill_width()
    .fill_height();

    ui::closeable_dialog_layer(
        "Trash Folder Required",
        content,
        ui::WidgetTone::Warning,
        ui::Vector2::new(560.0, 210.0),
        GuiMessage::CancelTrashFolderSetup,
    )
    .key(identity::TRASH_FOLDER_SETUP_MODAL_KEY)
}

pub(in crate::native_app) fn unsupported_files(state: &NativeAppState) -> ui::View<GuiMessage> {
    let dialog = state
        .ui
        .browser_interaction
        .unsupported_files_dialog
        .as_ref()
        .expect("unsupported-files modal requires dialog state");
    let source_label = dialog.source_label.clone();
    let paths = dialog.paths.clone();
    let body = if dialog.loading {
        ui::text_line("Loading unsupported files…", 22.0)
            .fill_width()
            .fill_height()
    } else if let Some(error) = dialog.error.as_deref() {
        ui::text_line(format!("Could not load unsupported files: {error}"), 20.0)
            .fill_width()
            .fill_height()
    } else if paths.is_empty() {
        ui::text_line(
            "No unsupported files are currently recorded for this source.",
            20.0,
        )
        .fill_width()
        .fill_height()
    } else {
        ui::scroll(
            ui::column(
                paths
                    .into_iter()
                    .map(unsupported_file_row)
                    .collect::<Vec<_>>(),
            )
            .spacing(4.0)
            .fill_width(),
        )
        .fill_width()
        .fill_height()
    };
    let content = ui::column([
        ui::text_line(
            format!(
                "{} unsupported file(s) recorded in {source_label}",
                dialog.paths.len()
            ),
            22.0,
        )
        .fill_width(),
        body,
        ui::button_row([ui::button("Close").message(GuiMessage::CloseUnsupportedFiles)]),
    ])
    .spacing(6.0)
    .fill_width()
    .fill_height();

    ui::closeable_dialog_layer(
        "Unsupported Files",
        content,
        ui::WidgetTone::Warning,
        ui::Vector2::new(820.0, 540.0),
        GuiMessage::CloseUnsupportedFiles,
    )
    .key(identity::UNSUPPORTED_FILES_MODAL_KEY)
}

fn unsupported_file_row(path: std::path::PathBuf) -> ui::View<GuiMessage> {
    let display_path = path.display().to_string();
    ui::row([
        ui::text_line(&display_path, 18.0).fill_width(),
        ui::button("Reveal")
            .message(GuiMessage::RevealUnsupportedFile(path.clone()))
            .width(72.0)
            .height(22.0),
        ui::button("Move to Trash")
            .danger()
            .message(GuiMessage::MoveUnsupportedFileToTrash(path))
            .width(UNSUPPORTED_FILE_ACTION_WIDTH)
            .height(22.0),
    ])
    .key(format!("unsupported-file-{}", display_path))
    .spacing(6.0)
    .fill_width()
    .height(28.0)
}

fn transaction_list_summary(projection: &TransactionListProjection) -> ui::View<GuiMessage> {
    ui::text_line(projection.summary.clone(), 20.0)
        .key(identity::TRANSACTION_LIST_SUMMARY_KEY)
        .fill_width()
}

fn shortcut_help_section_view(section: ShortcutHelpSection) -> ui::View<GuiMessage> {
    ui::column([
        ui::text_line(section.title, 20.0)
            .style(ui::WidgetStyle::strong(ui::WidgetTone::Accent))
            .fill_width(),
        ui::column(
            section
                .items
                .into_iter()
                .map(shortcut_help_row)
                .collect::<Vec<_>>(),
        )
        .spacing(2.0)
        .fill_width(),
    ])
    .style(ui::WidgetStyle::subtle(ui::WidgetTone::Neutral))
    .padding(6.0)
    .spacing(4.0)
    .fill_width()
}

fn shortcut_help_row(item: ShortcutHelpItem) -> ui::View<GuiMessage> {
    ui::row([
        ui::passive_badge(item.keys)
            .style(ui::WidgetStyle::subtle(ui::WidgetTone::Accent))
            .width(SHORTCUT_HELP_KEY_WIDTH)
            .height(20.0),
        ui::text_line(item.action, 20.0).fill_width(),
    ])
    .spacing(6.0)
    .fill_width()
    .height(22.0)
}

fn transaction_list_row(row: TransactionListRowProjection) -> ui::View<GuiMessage> {
    ui::row([
        transaction_list_row_action(&row),
        ui::text_line(row.order_label, 22.0).width(58.0),
        ui::text_line(row.label, 22.0).fill_width(),
        ui::text_line(row.action_summary, 22.0).width(240.0),
    ])
    .key(identity::transaction_list_row_key(row.id))
    .style(ui::WidgetStyle::subtle(ui::WidgetTone::Neutral))
    .padding_x(6.0)
    .spacing(6.0)
    .fill_width()
    .height(26.0)
}

fn transaction_list_row_action(row: &TransactionListRowProjection) -> ui::View<GuiMessage> {
    match row.state {
        TransactionListState::Active => ui::passive_badge(row.state.label().to_string())
            .style(transaction_list_state_style(row.state))
            .size(TRANSACTION_LIST_ACTION_WIDTH, 20.0),
        TransactionListState::Undoable => ui::button(row.state.label().to_string())
            .message(GuiMessage::UndoTransactionsThrough(row.id))
            .primary()
            .width(TRANSACTION_LIST_ACTION_WIDTH)
            .height(20.0),
        TransactionListState::Redoable => ui::button(row.state.label().to_string())
            .message(GuiMessage::RedoTransactionsThrough(row.id))
            .width(TRANSACTION_LIST_ACTION_WIDTH)
            .height(20.0),
    }
}

fn transaction_list_state_style(state: TransactionListState) -> ui::WidgetStyle {
    match state {
        TransactionListState::Active => ui::WidgetStyle::strong(ui::WidgetTone::Warning),
        TransactionListState::Undoable => ui::WidgetStyle::strong(ui::WidgetTone::Accent),
        TransactionListState::Redoable => ui::WidgetStyle::subtle(ui::WidgetTone::Neutral),
    }
}
