use radiant::prelude as ui;
use radiant::widgets::PointerModifiers;
use std::time::{Duration, Instant};

use crate::native_app::app::{GuiMessage, NativeAppState, emit_gui_action, logging};
use crate::native_app::sample_library::committed_file_mutations::{
    FileMutationOperation, FileMutationProjection, PreparedCommittedFileMutationChange,
};
use crate::native_app::sample_library::context_menu_target::BrowserContextTargetKind;
use crate::native_app::sample_library::folder_browser::commands::{
    FolderBrowserMessage, RenameCommitCompletion, RenameCommitRequest, RenameCommitResult,
    RenameCommitSuccess, RenameInputResult, RenamePathRemap, execute_rename_commit_request,
};

impl NativeAppState {
    pub(in crate::native_app) fn begin_folder_browser_rename(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let started_at = Instant::now();
        let target = self.library.folder_browser.selected_rename_target();
        if logging::debug_logging_enabled() {
            tracing::debug!(
                target: logging::ACTION_EVENT_TARGET,
                event = "action_detail",
                action = "folder_browser.rename.begin",
                pane = "folder_browser",
                target_kind = target.kind,
                target_label = target.label,
                is_source_root = target.is_source_root,
                "Folder browser rename requested"
            );
        }
        match self.library.folder_browser.begin_rename_selected() {
            Ok(Some(input_id)) => {
                self.ui.status.sample = rename_begin_status(target.kind);
                context.after(
                    Duration::from_millis(1),
                    GuiMessage::FocusRenameInput(input_id),
                );
                emit_gui_action(
                    "folder_browser.rename.begin",
                    Some("folder_browser"),
                    Some(target.kind),
                    "success",
                    started_at,
                    None,
                );
            }
            Ok(None) => {
                self.ui.status.sample = String::from("Select a folder to rename");
                emit_gui_action(
                    "folder_browser.rename.begin",
                    Some("folder_browser"),
                    None,
                    "short_circuit",
                    started_at,
                    Some("nothing_selected"),
                );
            }
            Err(error) => {
                self.ui.status.sample = error;
                emit_gui_action(
                    "folder_browser.rename.begin",
                    Some("folder_browser"),
                    None,
                    "error",
                    started_at,
                    Some("rename_begin_failed"),
                );
            }
        }
    }

    pub(in crate::native_app) fn rename_context_folder(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let Some(menu) = self.ui.browser_interaction.context_menu.take() else {
            self.ui.status.sample = String::from("Select a folder to rename");
            return;
        };
        if menu.kind != BrowserContextTargetKind::Folder {
            self.ui.status.sample = String::from("Select a folder to rename");
            return;
        }

        let folder_id = menu.path.to_string_lossy().to_string();
        if self
            .library
            .folder_browser
            .folder_path(&folder_id)
            .is_none()
        {
            self.ui.status.sample = String::from("Folder is unavailable");
            return;
        }

        self.library
            .folder_browser
            .apply_message(FolderBrowserMessage::ActivateFolder(
                folder_id,
                PointerModifiers::default(),
            ));
        self.begin_folder_browser_rename(context);
    }

    pub(in crate::native_app) fn begin_folder_browser_subfolder_creation(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let started_at = Instant::now();
        match self.library.folder_browser.begin_create_subfolder() {
            Ok(Some(input_id)) => {
                self.ui.status.sample = String::from("Creating new folder");
                context.after(
                    Duration::from_millis(1),
                    GuiMessage::FocusRenameInput(input_id),
                );
                emit_gui_action(
                    "folder_browser.folder.create_begin",
                    Some("folder_browser"),
                    Some("folder"),
                    "success",
                    started_at,
                    None,
                );
            }
            Ok(None) => {
                self.ui.status.sample = String::from("Select a folder to add a subfolder");
                emit_gui_action(
                    "folder_browser.folder.create_begin",
                    Some("folder_browser"),
                    None,
                    "short_circuit",
                    started_at,
                    Some("nothing_selected"),
                );
            }
            Err(error) => {
                self.ui.status.sample = error;
                emit_gui_action(
                    "folder_browser.folder.create_begin",
                    Some("folder_browser"),
                    None,
                    "error",
                    started_at,
                    Some("create_begin_failed"),
                );
            }
        }
    }

    pub(in crate::native_app) fn apply_folder_browser_rename_input(
        &mut self,
        message: radiant::widgets::TextInputMessage,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let started_at = Instant::now();
        let input_action = rename_input_action(&message);
        let collection_names_before = input_action
            .is_some()
            .then(|| self.library.folder_browser.custom_collection_names());
        if let Some(result) = self.library.folder_browser.apply_rename_input(message) {
            self.apply_folder_browser_rename_result(result, context);
        }
        if let Some(before) = collection_names_before {
            let after = self.library.folder_browser.custom_collection_names();
            if after != before {
                self.persist_user_configuration(
                    "folder_browser.collection_names.persist",
                    started_at,
                );
            }
        }
        if let Some(action) = input_action {
            emit_gui_action(
                action,
                Some("folder_browser"),
                None,
                "applied",
                started_at,
                None,
            );
        }
    }

    pub(in crate::native_app) fn finish_folder_browser_rename(
        &mut self,
        completion: RenameCommitCompletion,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        if let Err(error) = completion.result.as_ref() {
            let error = error.clone();
            let result = self
                .library
                .folder_browser
                .apply_rename_commit_completion(completion);
            self.apply_folder_browser_rename_status(result, context);
            self.record_failed_file_mutation(FileMutationOperation::Rename, None, error, context);
            return;
        }
        let Some((old_path, new_path)) = rename_completion_paths(&completion) else {
            let result = self
                .library
                .folder_browser
                .apply_rename_commit_completion(completion);
            self.apply_folder_browser_rename_status(result, context);
            return;
        };
        let metadata_error = rename_completion_metadata_error(&completion);
        let evidence = completion
            .post_filesystem_evidence
            .clone()
            .unwrap_or(wavecrate::sample_sources::SourceFileEvidence::Unverifiable);
        self.ui.status.sample = String::from("Finishing rename");
        self.queue_prepared_partially_committed_file_mutation(
            FileMutationOperation::Rename,
            vec![
                PreparedCommittedFileMutationChange::path_only_move(
                    old_path,
                    new_path.clone(),
                    evidence,
                )
                .with_projection(FileMutationProjection::RenameCompletion {
                    target_path: new_path,
                    completion,
                }),
            ],
            metadata_error
                .into_iter()
                .map(|error| (None, error))
                .collect(),
            context,
        );
    }

    pub(in crate::native_app) fn apply_committed_folder_browser_rename(
        &mut self,
        completion: RenameCommitCompletion,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let result = self
            .library
            .folder_browser
            .apply_rename_commit_completion(completion);
        if let Some(remap) = result.path_remap.as_ref() {
            self.apply_browser_rename_path_remap(remap);
            self.queue_active_similarity_score_resolution(context);
        }
        self.ui.status.sample = result.status;
    }

    fn apply_folder_browser_rename_result(
        &mut self,
        result: RenameInputResult,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        match result {
            RenameInputResult::Status(result) => {
                self.apply_folder_browser_rename_status(result, context);
            }
            RenameInputResult::Commit(request) => {
                self.ui.status.sample = String::from("Applying rename");
                context
                    .business()
                    .background("gui-folder-browser-rename")
                    .run(
                        move |_| execute_rename_commit_request(request),
                        GuiMessage::FolderBrowserRenameFinished,
                    );
            }
        }
    }

    fn apply_folder_browser_rename_status(
        &mut self,
        result: RenameCommitResult,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        if let Some(remap) = result.path_remap {
            self.apply_browser_rename_path_remap(&remap);
            self.queue_active_similarity_score_resolution(context);
        }
        self.ui.status.sample = result.status;
    }

    fn apply_browser_rename_path_remap(&mut self, remap: &RenamePathRemap) {
        self.waveform
            .current
            .rewrite_path_prefix(&remap.old_path, &remap.new_path);
        self.remap_renamed_waveform_cache_path(&remap.old_path, &remap.new_path);
    }
}

fn rename_completion_paths(
    completion: &RenameCommitCompletion,
) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    match (&completion.request, &completion.result) {
        (
            RenameCommitRequest::FolderRename {
                old_path, new_path, ..
            },
            Ok(RenameCommitSuccess::FolderRenamed),
        )
        | (
            RenameCommitRequest::FileRename {
                old_path, new_path, ..
            },
            Ok(RenameCommitSuccess::FileRenamed { .. }),
        ) => Some((old_path.clone(), new_path.clone())),
        _ => None,
    }
}

fn rename_completion_metadata_error(completion: &RenameCommitCompletion) -> Option<String> {
    match &completion.result {
        Ok(RenameCommitSuccess::FileRenamed {
            metadata_remap_result: Err(error),
        }) => Some(error.clone()),
        _ => None,
    }
}

fn rename_begin_status(target_kind: &str) -> String {
    match target_kind {
        "file" => String::from("Renaming selected file"),
        "collection" => String::from("Renaming selected collection"),
        "folder" => String::from("Renaming selected folder"),
        _ => String::from("Renaming selection"),
    }
}

fn rename_input_action(message: &radiant::widgets::TextInputMessage) -> Option<&'static str> {
    message
        .is_submitted()
        .then_some("folder_browser.rename.submit")
}
