use std::path::{Path, PathBuf};
use std::time::Instant;

use radiant::prelude as ui;
use wavecrate::sample_sources::{
    DuplicateDoubleRequest, DuplicateSameRequest, HarvestDerivationOperation, SampleSource,
    execute_duplicate_context_sample_double, execute_duplicate_context_sample_same,
};

use crate::native_app::app::{GuiMessage, NativeAppState, emit_gui_action};
use crate::native_app::sample_library::committed_file_mutations::{
    FileMutationOperation, FileMutationProjection, PreparedCommittedFileMutationChange,
};
use crate::native_app::sample_library::context_menu_target as context_menu;
use crate::native_app::sample_library::context_menu_target::{
    BrowserContextMenu, BrowserContextTargetKind,
};
use crate::native_app::sample_library::file_actions::sample_path_label;
use crate::native_app::sample_library::folder_browser::BrowserListingRevealReason;

impl NativeAppState {
    pub(in crate::native_app) fn duplicate_context_sample_same(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let started_at = Instant::now();
        let Some(menu) = self.ui.browser_interaction.context_menu.take() else {
            return;
        };
        let source_path = menu.path.clone();
        match self.duplicate_context_sample_same_request(&menu) {
            Ok(request) => {
                self.ui.status.sample = format!("Duplicating {}", sample_path_label(&source_path));
                context
                    .business()
                    .blocking_io("gui-sample-duplicate-same")
                    .run(
                        move |_| execute_duplicate_context_sample_same(request),
                        move |result| GuiMessage::ContextSampleSameFinished {
                            source_path,
                            started_at,
                            result,
                        },
                    );
            }
            Err(error) => {
                self.ui.status.sample = error.clone();
                emit_gui_action(
                    "browser.context_menu.sample.duplicate_same",
                    Some("browser"),
                    Some(context_menu::target_label(&menu.path).as_str()),
                    "error",
                    started_at,
                    Some(&error),
                );
            }
        }
    }

    fn duplicate_context_sample_same_request(
        &self,
        menu: &BrowserContextMenu,
    ) -> Result<DuplicateSameRequest, String> {
        if menu.kind != BrowserContextTargetKind::Sample {
            return Err(String::from("Choose a sample to duplicate"));
        }
        if menu.sample_missing {
            return Err(String::from("Sample file is missing"));
        }
        let Some((source_root, source_database_root)) =
            self.context_sample_source_roots(menu.source_id.as_deref(), &menu.path)
        else {
            return Err(String::from("Sample source is unavailable"));
        };
        Ok(DuplicateSameRequest {
            source_path: menu.path.clone(),
            source_root,
            source_database_root,
        })
    }

    fn context_sample_source_roots(
        &self,
        source_id: Option<&str>,
        path: &Path,
    ) -> Option<(PathBuf, PathBuf)> {
        source_id
            .and_then(|source_id| self.library.folder_browser.source_roots(source_id))
            .or_else(|| self.library.folder_browser.source_roots_for_path(path))
    }

    pub(in crate::native_app) fn duplicate_context_sample_double(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let started_at = Instant::now();
        let Some(menu) = self.ui.browser_interaction.context_menu.take() else {
            return;
        };
        let source_path = menu.path.clone();
        match self.duplicate_context_sample_double_request(&menu) {
            Ok(request) => {
                self.ui.status.sample = format!("Duplicating {}", sample_path_label(&source_path));
                context
                    .business()
                    .background("gui-sample-duplicate-double")
                    .run(
                        move |_| execute_duplicate_context_sample_double(request),
                        move |result| GuiMessage::ContextSampleDoubleFinished {
                            source_path,
                            started_at,
                            result,
                        },
                    );
            }
            Err(error) => {
                self.ui.status.sample = error.clone();
                emit_gui_action(
                    "browser.context_menu.sample.duplicate_double",
                    Some("browser"),
                    Some(context_menu::target_label(&menu.path).as_str()),
                    "error",
                    started_at,
                    Some(&error),
                );
            }
        }
    }

    fn duplicate_context_sample_double_request(
        &self,
        menu: &BrowserContextMenu,
    ) -> Result<DuplicateDoubleRequest, String> {
        if menu.kind != BrowserContextTargetKind::Sample {
            return Err(String::from("Choose a sample to duplicate"));
        }
        if menu.sample_missing {
            return Err(String::from("Sample file is missing"));
        }
        let Some((origin_source, _)) = self
            .library
            .folder_browser
            .sample_source_for_file_path(&menu.path)
        else {
            return Err(String::from("Sample source is unavailable"));
        };
        let (target_folder, target_source) =
            self.double_duplicate_target(&menu.path, &origin_source)?;
        if let Some(error) = self
            .library
            .folder_browser
            .folder_target_lock_error(&target_folder, "Duplicate")
        {
            return Err(error);
        }
        let target_database_root = target_source
            .database_root()
            .map_err(|err| format!("Resolve target metadata location failed: {err}"))?;
        Ok(DuplicateDoubleRequest {
            source_path: menu.path.clone(),
            target_folder,
            target_source_root: target_source.root,
            target_database_root,
        })
    }

    fn double_duplicate_target(
        &self,
        source_path: &Path,
        origin_source: &SampleSource,
    ) -> Result<(PathBuf, SampleSource), String> {
        if origin_source.is_protected() {
            let Some(primary_source) = self.library.folder_browser.primary_sample_source() else {
                return Err(String::from(
                    "Set a Primary source before duplicating from a protected source",
                ));
            };
            let target_folder = self
                .harvest_destination_for_protected_origin(source_path)?
                .unwrap_or_else(|| primary_source.primary_import_path());
            return Ok((target_folder, primary_source));
        }
        let target_folder = source_path
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| String::from("Sample file has no destination folder"))?;
        Ok((target_folder, origin_source.clone()))
    }

    pub(in crate::native_app) fn finish_context_sample_same(
        &mut self,
        source_path: PathBuf,
        started_at: Instant,
        result: Result<wavecrate::sample_sources::ContextSampleSameResult, String>,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        match result {
            Ok(completion) => {
                self.ui.status.sample =
                    format!("Duplicated {}", sample_path_label(&completion.destination));
                self.queue_prepared_committed_file_mutation(
                    FileMutationOperation::Duplicate,
                    vec![
                        PreparedCommittedFileMutationChange::created(
                            completion.destination.clone(),
                            completion.destination_evidence,
                        )
                        .with_projection(
                            FileMutationProjection::SelectAndFollow {
                                path: completion.destination.clone(),
                            },
                        ),
                    ],
                    context,
                );
                emit_gui_action(
                    "browser.context_menu.sample.duplicate_same",
                    Some("browser"),
                    Some(context_menu::target_label(&completion.destination).as_str()),
                    "success",
                    started_at,
                    None,
                );
            }
            Err(error) => {
                self.record_failed_file_mutation(
                    FileMutationOperation::Duplicate,
                    None,
                    error.clone(),
                    context,
                );
                self.ui.status.sample = error.clone();
                emit_gui_action(
                    "browser.context_menu.sample.duplicate_same",
                    Some("browser"),
                    Some(context_menu::target_label(&source_path).as_str()),
                    "error",
                    started_at,
                    Some(&error),
                );
            }
        }
    }

    pub(in crate::native_app) fn finish_context_sample_double(
        &mut self,
        source_path: PathBuf,
        started_at: Instant,
        result: Result<wavecrate::sample_sources::ContextSampleDoubleResult, String>,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        match result {
            Ok(completion) => {
                self.record_harvest_whole_file_derivation(
                    &source_path,
                    &completion.destination,
                    HarvestDerivationOperation::DuplicateDoubleCopy,
                );
                self.ui.status.sample =
                    format!("Duplicated {}", sample_path_label(&completion.destination));
                self.queue_prepared_committed_file_mutation(
                    FileMutationOperation::Duplicate,
                    vec![
                        PreparedCommittedFileMutationChange::created(
                            completion.destination.clone(),
                            completion.destination_evidence,
                        )
                        .with_projection(
                            FileMutationProjection::FocusAndLoad {
                                path: completion.destination.clone(),
                                reason: BrowserListingRevealReason::LoadedFileFocus,
                            },
                        ),
                    ],
                    context,
                );
                emit_gui_action(
                    "browser.context_menu.sample.duplicate_double",
                    Some("browser"),
                    Some(context_menu::target_label(&completion.destination).as_str()),
                    "success",
                    started_at,
                    None,
                );
            }
            Err(error) => {
                self.record_failed_file_mutation(
                    FileMutationOperation::Duplicate,
                    None,
                    error.clone(),
                    context,
                );
                self.ui.status.sample = error.clone();
                emit_gui_action(
                    "browser.context_menu.sample.duplicate_double",
                    Some("browser"),
                    Some(context_menu::target_label(&source_path).as_str()),
                    "error",
                    started_at,
                    Some(&error),
                );
            }
        }
    }
}
