use super::commands::FolderBrowserMessage;
use super::{FolderBrowserState, path_id};
use crate::native_app::sample_library::folder_browser::commands::{
    FileMoveConflictResolution, FileMoveConflictResolutionRequest, FolderDropResult,
    FolderMoveDropInput, RenameCommitResult, RenameInputResult, execute_file_move_conflict_request,
    execute_folder_move_request, execute_rename_commit_request,
};
use crate::native_app::sample_library::folder_browser::scan::{
    FolderScanDiscoveryBatch, FolderTreeRefreshRequest, refresh_folder_tree_only,
    scan_source_with_progress, scan_source_with_progress_cancellable,
};
use crate::native_app::sample_library::folder_browser::test_support::{
    FolderDragPreview, MIN_FILE_COLUMN_WIDTH,
};
use radiant::{
    layout::Point,
    runtime::ExternalDragPayload,
    widgets::{DragHandleMessage, PointerModifiers, TextInputMessage},
};
use std::{fs, path::PathBuf};
use wavecrate::sample_sources::{Rating, SampleCollection, SourceDatabase};

mod collection_activation;
mod collection_escape;
mod collection_rename;
mod cross_source_focus;
mod delete;
mod drop_target_chrome;
mod external_drag;
mod file_columns;
mod file_drag_hover;
mod file_filters;
mod file_keyboard_selection;
mod file_mouse_selection;
mod file_move_drag_drop;
mod file_rename;
mod file_scroll_follow;
mod file_selection_queries;
mod filesystem_refresh;
mod folder_drag_drop;
mod folder_editing;
mod folder_locks;
mod folder_tree;
mod panel_layout;
mod random_navigation;
mod source_management;
mod source_scanning;
#[cfg(unix)]
mod source_scanning_symlinks;

fn temp_source_root(name: &str) -> PathBuf {
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

fn submit_rename(browser: &mut FolderBrowserState, value: impl Into<String>) -> RenameCommitResult {
    match browser
        .apply_rename_input(TextInputMessage::Submitted {
            value: value.into(),
        })
        .expect("rename result")
    {
        RenameInputResult::Status(result) => result,
        RenameInputResult::Commit(request) => {
            let completion = execute_rename_commit_request(request);
            browser.apply_rename_commit_completion(completion)
        }
    }
}

fn submit_folder_drop(
    browser: &mut FolderBrowserState,
    target_folder_id: &str,
) -> Result<FolderDropResult, String> {
    match browser.drop_drag_on_folder(target_folder_id)? {
        FolderMoveDropInput::Status(result) => Ok(result),
        FolderMoveDropInput::Request(request) => {
            let completion = execute_folder_move_request(request);
            let tags_by_file = std::collections::HashMap::new();
            completion.result.and_then(|success| {
                browser.apply_folder_move_completion(&completion.request, success, &tags_by_file)
            })
        }
    }
}

fn submit_file_move_conflict(
    browser: &mut FolderBrowserState,
    request: impl Into<FileMoveConflictResolutionRequest>,
) -> Result<FolderDropResult, String> {
    let Some(batch) = browser.take_file_move_conflict_batch() else {
        return Ok(FolderDropResult::default());
    };
    let completion = execute_file_move_conflict_request(batch, request.into());
    let tags_by_file = std::collections::HashMap::new();
    browser.apply_file_move_conflict_completion(completion, &tags_by_file)
}

#[test]
fn root_facade_exports_only_stable_entrypoints() {
    let source = include_str!("../../folder_browser.rs");
    let root_exports = source
        .lines()
        .filter(|line| line.starts_with("pub(in crate::native_app) use "))
        .collect::<Vec<_>>();

    assert_eq!(
        root_exports,
        vec!["pub(in crate::native_app) use state::FolderBrowserState;"]
    );
    for module in [
        "commands",
        "model",
        "projection",
        "scan",
        "test_support",
        "view_contract",
    ] {
        assert!(
            source.contains(&format!("pub(in crate::native_app) mod {module}")),
            "folder-browser API group `{module}` should stay explicit"
        );
    }
}
