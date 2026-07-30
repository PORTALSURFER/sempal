use std::{path::PathBuf, time::Instant};

use super::movement::{
    TrashMoveOutcome, TrashMoveResult, move_path_to_configured_trash,
    move_paths_to_configured_trash,
};
use crate::native_app::app::{
    GuiMessage, NativeAppState, PendingFolderDelete, PendingTrashFolderSetup, TrashMoveTarget,
    emit_gui_action, sample_path_label,
};
use crate::native_app::sample_library::committed_file_mutations::{
    FileMutationOperation, FileMutationProjection, PreparedCommittedFileMutationChange,
};
use crate::native_app::sample_library::context_menu_target::BrowserContextTargetKind;
use crate::native_app::sample_library::sample_list::{
    SAMPLE_BROWSER_LIST_ID, SAMPLE_BROWSER_ROW_HEIGHT, SAMPLE_BROWSER_SELECTION_CONTEXT_ROWS,
};

impl NativeAppState {
    pub(in crate::native_app) fn move_context_target_to_trash(
        &mut self,
        context: &mut radiant::prelude::UiUpdateContext<GuiMessage>,
    ) {
        let started_at = Instant::now();
        let Some(menu) = self.ui.browser_interaction.context_menu.take() else {
            return;
        };
        match menu.kind {
            BrowserContextTargetKind::Folder => {
                self.move_folder_path_to_trash(
                    menu.path,
                    "browser.context_menu.folder.trash",
                    started_at,
                    context,
                );
            }
            BrowserContextTargetKind::Sample => {
                let paths = self.context_sample_trash_paths(menu.path);
                self.move_file_paths_to_trash(
                    paths,
                    "browser.context_menu.sample.trash",
                    started_at,
                    context,
                );
            }
            BrowserContextTargetKind::Source
            | BrowserContextTargetKind::Collection
            | BrowserContextTargetKind::MetadataTag => {
                self.ui.status.sample = String::from("Context target cannot be moved to trash");
                emit_gui_action(
                    "browser.context_menu.trash",
                    Some("browser"),
                    None,
                    "blocked",
                    started_at,
                    Some("unsupported target"),
                );
            }
        }
    }

    pub(in crate::native_app) fn request_delete_context_folder(&mut self) {
        let started_at = Instant::now();
        let Some(menu) = self.ui.browser_interaction.context_menu.take() else {
            return;
        };
        if menu.kind != BrowserContextTargetKind::Folder {
            self.ui.status.sample = String::from("Choose a folder to delete");
            emit_gui_action(
                "browser.context_menu.folder.delete",
                Some("folder_browser"),
                None,
                "blocked",
                started_at,
                Some("unsupported target"),
            );
            return;
        }
        if let Some(error) = self
            .library
            .folder_browser
            .folder_change_lock_error(&menu.path, "Folder delete")
        {
            self.ui.status.sample = error.clone();
            emit_gui_action(
                "browser.context_menu.folder.delete",
                Some("folder_browser"),
                Some(sample_path_label(&menu.path).as_str()),
                "blocked",
                started_at,
                Some(&error),
            );
            return;
        }
        let name = sample_path_label(&menu.path);
        self.ui.browser_interaction.pending_folder_delete = Some(PendingFolderDelete {
            path: menu.path,
            name: name.clone(),
        });
        self.ui.status.sample = format!("Confirm delete folder {name}");
        emit_gui_action(
            "browser.context_menu.folder.delete",
            Some("folder_browser"),
            Some(name.as_str()),
            "confirming",
            started_at,
            None,
        );
    }

    pub(in crate::native_app) fn confirm_context_folder_delete(
        &mut self,
        context: &mut radiant::prelude::UiUpdateContext<GuiMessage>,
    ) {
        let started_at = Instant::now();
        let Some(target) = self.ui.browser_interaction.pending_folder_delete.take() else {
            return;
        };
        self.move_folder_path_to_trash(
            target.path,
            "browser.context_menu.folder.delete",
            started_at,
            context,
        );
    }

    pub(in crate::native_app) fn cancel_context_folder_delete(&mut self) {
        self.ui.browser_interaction.pending_folder_delete = None;
        self.ui.status.sample = String::from("Folder delete canceled");
    }

    pub(in crate::native_app) fn move_selected_folder_to_trash(
        &mut self,
        path: PathBuf,
        started_at: Instant,
        context: &mut radiant::prelude::UiUpdateContext<GuiMessage>,
    ) {
        self.move_folder_path_to_trash(path, "folder_browser.delete_selected", started_at, context);
    }

    pub(in crate::native_app) fn move_selected_files_to_trash(
        &mut self,
        paths: Vec<PathBuf>,
        started_at: Instant,
        context: &mut radiant::prelude::UiUpdateContext<GuiMessage>,
    ) {
        self.move_file_paths_to_trash(paths, "browser.delete_selected_files", started_at, context);
    }

    pub(in crate::native_app) fn move_negative_threshold_files_to_trash(
        &mut self,
        paths: Vec<PathBuf>,
        started_at: Instant,
        context: &mut radiant::prelude::UiUpdateContext<GuiMessage>,
    ) {
        self.move_file_paths_to_trash(
            paths,
            "browser.rating.auto_trash_threshold",
            started_at,
            context,
        );
    }

    fn move_folder_path_to_trash(
        &mut self,
        path: PathBuf,
        action: &'static str,
        started_at: Instant,
        context: &mut radiant::prelude::UiUpdateContext<GuiMessage>,
    ) {
        if let Some(error) = self
            .library
            .folder_browser
            .folder_change_lock_error(&path, "Folder trash")
        {
            self.ui.status.sample = error.clone();
            emit_gui_action(
                action,
                Some("folder_browser"),
                Some(sample_path_label(&path).as_str()),
                "blocked",
                started_at,
                Some(&error),
            );
            return;
        }
        let trash_folder = self.ui.settings.persisted.trash_folder.clone();
        self.ui.status.sample = format!("Moving {} to trash", sample_path_label(&path));
        context.business().blocking_io("gui-trash-move").run(
            {
                let path = path.clone();
                move |_| {
                    vec![move_path_to_configured_trash(
                        &path,
                        trash_folder.as_deref(),
                    )]
                }
            },
            move |outcomes| GuiMessage::TrashMoveFinished {
                target: TrashMoveTarget::Folder(path),
                action,
                started_at,
                outcomes,
            },
        );
    }

    pub(in crate::native_app) fn finish_trash_move(
        &mut self,
        target: TrashMoveTarget,
        action: &'static str,
        started_at: Instant,
        outcomes: Vec<TrashMoveOutcome>,
        context: &mut radiant::prelude::UiUpdateContext<GuiMessage>,
    ) {
        let committed_paths = outcomes
            .iter()
            .filter(|outcome| {
                matches!(
                    outcome.result,
                    TrashMoveResult::Moved { .. } | TrashMoveResult::Missing
                )
            })
            .map(|outcome| (outcome.source.clone(), outcome.evidence.clone()))
            .collect::<Vec<_>>();
        let failed_mutations = outcomes
            .iter()
            .filter_map(|outcome| match &outcome.result {
                TrashMoveResult::Failed { error } => Some(error.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let projection = match target {
            TrashMoveTarget::Folder(path) => {
                match outcomes.first().map(|outcome| &outcome.result) {
                    Some(TrashMoveResult::Moved { destination }) => self.finish_folder_trash_move(
                        path.clone(),
                        destination.clone(),
                        action,
                        started_at,
                    ),
                    Some(TrashMoveResult::Missing) => {
                        self.finish_folder_trash_move_missing(path.clone(), action, started_at);
                    }
                    Some(TrashMoveResult::Failed { error }) => self.finish_trash_move_error(
                        Some(path.clone()),
                        action,
                        started_at,
                        error.clone(),
                    ),
                    None => self.finish_trash_move_error(
                        Some(path.clone()),
                        action,
                        started_at,
                        String::from("Trash move produced no outcome"),
                    ),
                }
                committed_paths
                    .iter()
                    .any(|(committed, _)| committed == &path)
                    .then_some(FileMutationProjection::TrashFolder { path })
            }
            TrashMoveTarget::Files(_) => self.finish_file_trash_move(&outcomes, action, started_at),
        };
        let mut committed_changes = committed_paths
            .into_iter()
            .map(|(path, evidence)| PreparedCommittedFileMutationChange::deleted(path, evidence))
            .collect::<Vec<_>>();
        if let (Some(change), Some(projection)) = (committed_changes.first_mut(), projection) {
            *change = change.clone().with_projection(projection);
        }
        self.queue_prepared_partially_committed_file_mutation(
            FileMutationOperation::Trash,
            committed_changes,
            failed_mutations
                .into_iter()
                .map(|error| (None, error))
                .collect(),
            context,
        );
    }

    fn finish_folder_trash_move(
        &mut self,
        path: PathBuf,
        destination: PathBuf,
        action: &'static str,
        started_at: Instant,
    ) {
        self.ui.status.sample = format!("Moved {} to trash", sample_path_label(&destination));
        emit_gui_action(
            action,
            Some("folder_browser"),
            Some(sample_path_label(&path).as_str()),
            "success",
            started_at,
            None,
        );
    }

    fn finish_file_trash_move(
        &mut self,
        outcomes: &[TrashMoveOutcome],
        action: &'static str,
        started_at: Instant,
    ) -> Option<FileMutationProjection> {
        let reconciled_paths = outcomes
            .iter()
            .filter(|outcome| {
                matches!(
                    outcome.result,
                    TrashMoveResult::Moved { .. } | TrashMoveResult::Missing
                )
            })
            .map(|outcome| outcome.source.clone())
            .collect::<Vec<_>>();
        let moved_count = outcomes
            .iter()
            .filter(|outcome| matches!(outcome.result, TrashMoveResult::Moved { .. }))
            .count();
        let missing_count = outcomes
            .iter()
            .filter(|outcome| matches!(outcome.result, TrashMoveResult::Missing))
            .count();
        let failed_paths = outcomes
            .iter()
            .filter(|outcome| matches!(outcome.result, TrashMoveResult::Failed { .. }))
            .map(|outcome| outcome.source.clone())
            .collect::<Vec<_>>();
        let failures = outcomes
            .iter()
            .filter_map(|outcome| match &outcome.result {
                TrashMoveResult::Failed { error } => Some(error.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let previous_selected = self
            .library
            .folder_browser
            .selected_file_id()
            .map(str::to_owned);
        let loaded_removed = reconciled_paths
            .iter()
            .any(|path| self.waveform.current.path() == path.as_path());
        let (status, outcome) =
            trash_batch_completion(moved_count, missing_count, &failures, action);
        self.ui.status.sample = status.clone();
        let noun = if moved_count == 1 { "file" } else { "files" };
        emit_gui_action(
            action,
            Some("browser"),
            Some(&format!("{moved_count} {noun}")),
            outcome,
            started_at,
            failures.first().copied(),
        );
        reconciled_paths
            .first()
            .cloned()
            .map(|target_path| FileMutationProjection::TrashFiles {
                target_path,
                reconciled_paths,
                failed_paths,
                previous_selected,
                loaded_removed,
                status,
            })
    }

    pub(in crate::native_app) fn load_selected_sample_after_trash_if_needed(
        &mut self,
        selected_after_trash: Option<String>,
        focus_changed: bool,
        loaded_removed: bool,
        context: &mut radiant::prelude::UiUpdateContext<GuiMessage>,
    ) {
        let Some(selected) = selected_after_trash else {
            return;
        };
        if !focus_changed && !loaded_removed {
            return;
        }
        if focus_changed {
            self.cancel_metadata_tag_entry();
            self.metadata.selected_tag = None;
        }
        if let Some(index) = self
            .library
            .folder_browser
            .selected_audio_file_index_matching_tags(&self.metadata.tags_by_file)
        {
            context.scroll_fixed_row_into_view(
                SAMPLE_BROWSER_LIST_ID,
                index,
                SAMPLE_BROWSER_ROW_HEIGHT,
                SAMPLE_BROWSER_SELECTION_CONTEXT_ROWS,
                SAMPLE_BROWSER_SELECTION_CONTEXT_ROWS,
                1,
            );
        }
        self.load_navigation_sample(selected, context);
    }

    fn finish_folder_trash_move_missing(
        &mut self,
        path: PathBuf,
        action: &'static str,
        started_at: Instant,
    ) {
        let label = sample_path_label(&path);
        self.ui.status.sample =
            format!("Folder {label} no longer exists; removed it from the browser");
        emit_gui_action(
            action,
            Some("folder_browser"),
            Some(label.as_str()),
            "reconciled",
            started_at,
            Some("folder missing"),
        );
    }

    fn finish_trash_move_error(
        &mut self,
        path: Option<PathBuf>,
        action: &'static str,
        started_at: Instant,
        error: String,
    ) {
        self.ui.status.sample = error.clone();
        emit_gui_action(
            action,
            Some("browser"),
            path.as_ref().map(sample_path_label).as_deref(),
            "error",
            started_at,
            Some(&error),
        );
    }

    pub(super) fn move_file_paths_to_trash(
        &mut self,
        paths: Vec<PathBuf>,
        action: &'static str,
        started_at: Instant,
        context: &mut radiant::prelude::UiUpdateContext<GuiMessage>,
    ) {
        if let Some((blocked_path, error)) = paths.iter().find_map(|path| {
            self.library
                .folder_browser
                .file_change_lock_error(path, "File trash")
                .map(|error| (path, error))
        }) {
            self.flash_protected_source_block_if_error(&error, blocked_path);
            self.ui.status.sample = self.protected_source_status_or_error(&error, blocked_path);
            emit_gui_action(
                action,
                Some("browser"),
                None,
                "blocked",
                started_at,
                Some(&error),
            );
            return;
        }
        if self.ui.settings.persisted.trash_folder.is_none() {
            self.ui.browser_interaction.pending_trash_folder_setup =
                Some(PendingTrashFolderSetup {
                    paths,
                    action,
                    started_at,
                });
            self.ui.status.sample =
                String::from("Choose a trash folder before moving selected files to trash");
            emit_gui_action(
                action,
                Some("browser"),
                None,
                "needs_trash_folder",
                started_at,
                Some("trash folder is not configured"),
            );
            return;
        }
        let trash_folder = self.ui.settings.persisted.trash_folder.clone();
        self.ui.status.sample = trash_move_started_status(paths.len(), action);
        context.business().blocking_io("gui-trash-move").run(
            {
                let paths = paths.clone();
                move |_| move_paths_to_configured_trash(&paths, trash_folder.as_deref())
            },
            move |outcomes| GuiMessage::TrashMoveFinished {
                target: TrashMoveTarget::Files(paths),
                action,
                started_at,
                outcomes,
            },
        );
    }

    fn context_sample_trash_paths(&self, path: PathBuf) -> Vec<PathBuf> {
        let selected_paths = self.library.folder_browser.selected_file_paths();
        if selected_paths.iter().any(|selected| selected == &path) {
            selected_paths
        } else {
            vec![path]
        }
    }
}

fn trash_batch_completion(
    moved_count: usize,
    missing_count: usize,
    failures: &[&str],
    action: &str,
) -> (String, &'static str) {
    let noun = if moved_count == 1 { "file" } else { "files" };
    let failure_summary = bounded_failure_summary(failures);
    if moved_count == 0 && !failures.is_empty() {
        return (
            format!(
                "Failed to move {} files to trash: {failure_summary}",
                failures.len()
            ),
            "error",
        );
    }
    if !failures.is_empty() {
        return (
            format!(
                "Moved {moved_count} {noun} to trash; {} failed: {failure_summary}",
                failures.len()
            ),
            "partial",
        );
    }
    if moved_count == 0 && missing_count > 0 {
        let noun = if missing_count == 1 { "file" } else { "files" };
        return (
            format!("Removed {missing_count} missing {noun} from the browser"),
            "reconciled",
        );
    }
    (
        trash_move_finished_status(moved_count, noun, action),
        "success",
    )
}

fn bounded_failure_summary(failures: &[&str]) -> String {
    const MAX_VISIBLE_FAILURES: usize = 3;
    let mut summary = failures
        .iter()
        .take(MAX_VISIBLE_FAILURES)
        .copied()
        .collect::<Vec<_>>()
        .join("; ");
    let remaining = failures.len().saturating_sub(MAX_VISIBLE_FAILURES);
    if remaining > 0 {
        summary.push_str(&format!("; and {remaining} more"));
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_failed_batch_reports_error_and_caps_details() {
        let failures = ["one", "two", "three", "four", "five"];

        let (status, outcome) = trash_batch_completion(0, 0, &failures, "trash");

        assert_eq!(outcome, "error");
        assert_eq!(
            status,
            "Failed to move 5 files to trash: one; two; three; and 2 more"
        );
    }

    #[test]
    fn missing_only_batch_reports_reconciliation() {
        let (status, outcome) = trash_batch_completion(0, 2, &[], "trash");

        assert_eq!(outcome, "reconciled");
        assert_eq!(status, "Removed 2 missing files from the browser");
    }
}

fn trash_move_started_status(count: usize, action: &str) -> String {
    if action == "browser.rating.auto_trash_threshold" {
        return match count {
            1 => String::from("Moving sample to trash after fourth negative rating"),
            count => format!("Moving {count} samples to trash after fourth negative rating"),
        };
    }
    match count {
        1 => String::from("Moving file to trash"),
        count => format!("Moving {count} files to trash"),
    }
}

fn trash_move_finished_status(count: usize, noun: &str, action: &str) -> String {
    if action == "browser.rating.auto_trash_threshold" {
        return format!("Moved {count} {noun} to trash after fourth negative rating");
    }
    format!("Moved {count} {noun} to trash")
}
