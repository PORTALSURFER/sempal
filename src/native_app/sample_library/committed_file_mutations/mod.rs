//! Authoritative completion contract for Wavecrate-owned filesystem mutations.
//!
//! File-operation workers own the filesystem and operation-specific rollback. Once a worker has
//! reached its durable filesystem boundary, this module reconciles every affected source database,
//! publishes one revisioned outcome, refreshes the browser projection from that committed state,
//! acknowledges the matching watcher echo, and only then wakes durable readiness reconciliation.

use std::{collections::BTreeSet, path::PathBuf, time::Instant};

use radiant::prelude as ui;
use wavecrate::sample_sources::{readiness::ReadinessStage, scanner::CommittedSourceDelta};
use wavecrate::selection::SelectionRange;

use crate::native_app::app::{ExtractedFilePlaybackType, GuiMessage, NativeAppState};
use crate::native_app::sample_library::folder_browser::BrowserListingRevealReason;
use crate::native_app::sample_library::folder_browser::commands::{
    FileMoveConflictCompletion, FolderMoveRequest, FolderMoveSuccess, RenameCommitCompletion,
};
use crate::native_app::sample_library::source_prep::{
    CacheWarmIntent, MetadataRefreshIntent, ReadinessIntent, SourceFeedbackIntent,
    SourcePrepIntents, SourcePriorityIntent,
};

#[cfg(test)]
mod tests;
mod watcher_echo;
mod worker;

pub(in crate::native_app) use watcher_echo::{
    CommittedWatcherEcho, CommittedWatcherPathState, observed_watcher_path_state,
};
use worker::{
    build_source_requests, capture_expected_filesystem_state, merge_file_mutation_failures,
    mutation_completion_is_stale_or_duplicate, reconcile_file_mutation_requests,
};

pub(in crate::native_app) const COMMITTED_MUTATION_PREP_INTENTS: SourcePrepIntents =
    SourcePrepIntents {
        readiness: ReadinessIntent::InvalidateAndRequestConvergence,
        priority: SourcePriorityIntent::PromoteIfSelected,
        metadata_refresh: MetadataRefreshIntent::Force,
        refresh_waveform_cache_projection_if_selected: true,
        cache_warm: CacheWarmIntent::Preserve,
        feedback: SourceFeedbackIntent::Preserve,
    };
pub(in crate::native_app) const COMMITTED_PLAYMARK_PREP_INTENTS: SourcePrepIntents =
    SourcePrepIntents {
        readiness: ReadinessIntent::InvalidateAndRequestConvergence,
        priority: SourcePriorityIntent::PromoteIfSelected,
        metadata_refresh: MetadataRefreshIntent::IfNotLoaded,
        refresh_waveform_cache_projection_if_selected: true,
        cache_warm: CacheWarmIntent::Preserve,
        feedback: SourceFeedbackIntent::Preserve,
    };
pub(in crate::native_app) const COMMITTED_MUTATION_PREP_REASON: &str = "filesystem_changed";

#[cfg(test)]
pub(in crate::native_app) fn reconcile_file_mutation_for_liveness_test(
    source: wavecrate::sample_sources::SampleSource,
    operation_id: u64,
    operation: FileMutationOperation,
    mut changes: Vec<FileMutationChange>,
) -> Result<CommittedFileMutation, String> {
    capture_expected_filesystem_state(&mut changes);
    let requests = build_source_requests(operation_id, operation, changes, &[source]);
    match reconcile_file_mutation_requests(requests) {
        FileMutationOutcome::Committed(mut committed) if committed.len() == 1 => {
            Ok(committed.remove(0))
        }
        FileMutationOutcome::Committed(committed) => Err(format!(
            "liveness mutation expected one committed source, got {}",
            committed.len()
        )),
        FileMutationOutcome::Failed {
            committed,
            failures,
        } => Err(format!(
            "liveness mutation partially failed: {} committed, failures={failures:?}",
            committed.len()
        )),
        FileMutationOutcome::RolledBack(failure) => {
            Err(format!("liveness mutation rolled back: {failure:?}"))
        }
    }
}

/// User-visible mutation family that owns one operation ID across all affected sources.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::native_app) enum FileMutationOperation {
    Duplicate,
    Extract,
    ImportDrop,
    Edit,
    Normalize,
    Undo,
    Redo,
    Rename,
    Move,
    Trash,
}

impl FileMutationOperation {
    fn as_str(self) -> &'static str {
        match self {
            Self::Duplicate => "duplicate",
            Self::Extract => "extract",
            Self::ImportDrop => "import_drop",
            Self::Edit => "edit",
            Self::Normalize => "normalize",
            Self::Undo => "undo",
            Self::Redo => "redo",
            Self::Rename => "rename",
            Self::Move => "move",
            Self::Trash => "trash",
        }
    }
}

/// Readiness-relevant meaning of one committed path transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::native_app) enum FileMutationSemantics {
    Create,
    ContentChanged,
    PathOnlyMove,
    Delete,
}

/// Extraction follow-up inputs captured before asynchronous reconciliation changes selection or
/// playback state. The follow-up is admitted only after the created source row is authoritative.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::native_app) struct FileMutationPostCommit {
    pub(in crate::native_app) source_path: PathBuf,
    pub(in crate::native_app) selection: SelectionRange,
    pub(in crate::native_app) playback_type: ExtractedFilePlaybackType,
    pub(in crate::native_app) focus_derivative: bool,
    pub(in crate::native_app) started_at: Instant,
    pub(in crate::native_app) presentation: FileMutationPostCommitPresentation,
}

impl Eq for FileMutationPostCommit {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::native_app) enum FileMutationPostCommitPresentation {
    Extracted,
    Drag,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ExpectedMutationPathState {
    Missing,
    ContentHash([u8; 32]),
    Metadata {
        len: u64,
        modified_ns: Option<i64>,
        is_dir: bool,
    },
    Unverifiable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) enum FileMutationProjection {
    SelectAndFollow {
        path: PathBuf,
    },
    SelectAndLoad {
        path: PathBuf,
    },
    FocusAndLoad {
        path: PathBuf,
        reason: BrowserListingRevealReason,
    },
    LoadSelectedIfChanged {
        target_path: PathBuf,
        previous_selected: Option<String>,
    },
    RenameCompletion {
        target_path: PathBuf,
        completion: RenameCommitCompletion,
    },
    MoveCompletion {
        target_path: PathBuf,
        cut_paste: bool,
        request: FolderMoveRequest,
        success: FolderMoveSuccess,
        previous_selected: Option<String>,
        started_at: Instant,
    },
    MoveConflictCompletion {
        target_path: PathBuf,
        completion: FileMoveConflictCompletion,
        previous_selected: Option<String>,
        started_at: Instant,
    },
    MoveTransaction {
        target_path: PathBuf,
        source_root: PathBuf,
        source_database_root: PathBuf,
        moves: Vec<(PathBuf, PathBuf)>,
    },
    TrashFolder {
        path: PathBuf,
    },
    TrashFiles {
        target_path: PathBuf,
        reconciled_paths: Vec<PathBuf>,
        failed_paths: Vec<PathBuf>,
        previous_selected: Option<String>,
        loaded_removed: bool,
        status: String,
    },
}

impl FileMutationProjection {
    fn target_path(&self) -> Option<&std::path::Path> {
        match self {
            Self::SelectAndFollow { path }
            | Self::SelectAndLoad { path }
            | Self::FocusAndLoad { path, .. } => Some(path),
            Self::LoadSelectedIfChanged { target_path, .. }
            | Self::RenameCompletion { target_path, .. }
            | Self::MoveCompletion { target_path, .. }
            | Self::MoveConflictCompletion { target_path, .. }
            | Self::MoveTransaction { target_path, .. }
            | Self::TrashFiles { target_path, .. } => Some(target_path),
            Self::TrashFolder { path } => Some(path),
        }
    }

    fn replaces_default_refresh(&self) -> bool {
        matches!(
            self,
            Self::RenameCompletion { .. }
                | Self::MoveCompletion { .. }
                | Self::MoveConflictCompletion { .. }
                | Self::MoveTransaction { .. }
                | Self::TrashFolder { .. }
                | Self::TrashFiles { .. }
        )
    }
}

/// One logical file or folder transition. Paths are absolute so cross-source moves retain both
/// endpoints in every source-scoped outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct FileMutationChange {
    pub(in crate::native_app) before_path: Option<PathBuf>,
    pub(in crate::native_app) after_path: Option<PathBuf>,
    pub(in crate::native_app) before_content_identity: Option<String>,
    pub(in crate::native_app) after_content_identity: Option<String>,
    pub(in crate::native_app) semantics: FileMutationSemantics,
    expected_before_state: Option<ExpectedMutationPathState>,
    expected_after_state: Option<ExpectedMutationPathState>,
    projection: Option<FileMutationProjection>,
    post_commit: Option<FileMutationPostCommit>,
}

impl FileMutationChange {
    pub(in crate::native_app) fn created(path: PathBuf) -> Self {
        Self {
            before_path: None,
            after_path: Some(path),
            before_content_identity: None,
            after_content_identity: None,
            semantics: FileMutationSemantics::Create,
            expected_before_state: None,
            expected_after_state: None,
            projection: None,
            post_commit: None,
        }
    }

    pub(in crate::native_app) fn content_changed(path: PathBuf) -> Self {
        Self {
            before_path: Some(path.clone()),
            after_path: Some(path),
            before_content_identity: None,
            after_content_identity: None,
            semantics: FileMutationSemantics::ContentChanged,
            expected_before_state: None,
            expected_after_state: None,
            projection: None,
            post_commit: None,
        }
    }

    pub(in crate::native_app) fn path_only_move(before: PathBuf, after: PathBuf) -> Self {
        Self {
            before_path: Some(before),
            after_path: Some(after),
            before_content_identity: None,
            after_content_identity: None,
            semantics: FileMutationSemantics::PathOnlyMove,
            expected_before_state: None,
            expected_after_state: None,
            projection: None,
            post_commit: None,
        }
    }

    pub(in crate::native_app) fn deleted(path: PathBuf) -> Self {
        Self {
            before_path: Some(path),
            after_path: None,
            before_content_identity: None,
            after_content_identity: None,
            semantics: FileMutationSemantics::Delete,
            expected_before_state: None,
            expected_after_state: None,
            projection: None,
            post_commit: None,
        }
    }

    pub(in crate::native_app) fn with_before_content_identity(
        mut self,
        identity: Option<String>,
    ) -> Self {
        self.before_content_identity = identity;
        self
    }

    pub(in crate::native_app) fn with_projection(
        mut self,
        projection: FileMutationProjection,
    ) -> Self {
        self.projection = Some(projection);
        self
    }

    pub(in crate::native_app) fn with_post_commit(
        mut self,
        post_commit: FileMutationPostCommit,
    ) -> Self {
        self.post_commit = Some(post_commit);
        self
    }

    fn retain_projection_for_source(&mut self, source_root: &std::path::Path) {
        if self
            .projection
            .as_ref()
            .and_then(FileMutationProjection::target_path)
            .is_some_and(|path| !path.starts_with(source_root))
        {
            self.projection = None;
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct CommittedFileMutation {
    pub(in crate::native_app) source_id: String,
    pub(in crate::native_app) lifecycle_generation: u64,
    pub(in crate::native_app) operation_id: u64,
    pub(in crate::native_app) operation: FileMutationOperation,
    pub(in crate::native_app) committed_source_revision: u64,
    pub(in crate::native_app) changes: Vec<FileMutationChange>,
    pub(in crate::native_app) invalidated_stages: BTreeSet<ReadinessStage>,
    pub(in crate::native_app) committed_delta: CommittedSourceDelta,
    pub(in crate::native_app) affected_relative_paths: Vec<PathBuf>,
    pub(in crate::native_app) watcher_echoes: Vec<CommittedWatcherEcho>,
    pub(in crate::native_app) browser_projection_delta:
        Option<crate::native_app::app::BrowserProjectionDelta>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct FileMutationFailure {
    pub(in crate::native_app) source_id: Option<String>,
    pub(in crate::native_app) operation_id: u64,
    pub(in crate::native_app) operation: FileMutationOperation,
    pub(in crate::native_app) error: String,
}

/// Explicit terminal outcome. A cross-source operation can commit one source and fail another;
/// readiness is woken only for entries in `Committed`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) enum FileMutationOutcome {
    Committed(Vec<CommittedFileMutation>),
    Failed {
        committed: Vec<CommittedFileMutation>,
        failures: Vec<FileMutationFailure>,
    },
    RolledBack(FileMutationFailure),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct FileMutationWork {
    requests: Vec<worker::SourceMutationRequest>,
    failures: Vec<FileMutationFailure>,
}

impl NativeAppState {
    /// Reconcile one successful Wavecrate-owned filesystem operation off the UI thread.
    pub(in crate::native_app) fn queue_committed_file_mutation(
        &mut self,
        operation: FileMutationOperation,
        changes: Vec<FileMutationChange>,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) -> Option<u64> {
        self.queue_file_mutation_outcome(operation, changes, Vec::new(), context)
    }

    pub(in crate::native_app) fn queue_partially_committed_file_mutation(
        &mut self,
        operation: FileMutationOperation,
        changes: Vec<FileMutationChange>,
        failures: Vec<(Option<String>, String)>,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) -> Option<u64> {
        self.queue_file_mutation_outcome(operation, changes, failures, context)
    }

    fn queue_file_mutation_outcome(
        &mut self,
        operation: FileMutationOperation,
        mut changes: Vec<FileMutationChange>,
        reported_failures: Vec<(Option<String>, String)>,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) -> Option<u64> {
        if changes.is_empty() && reported_failures.is_empty() {
            return None;
        }
        let operation_id = self.background.next_task_id();
        let had_changes = !changes.is_empty();
        capture_expected_filesystem_state(&mut changes);
        let failures = reported_failures
            .into_iter()
            .map(|(source_id, error)| FileMutationFailure {
                source_id,
                operation_id,
                operation,
                error,
            })
            .collect::<Vec<_>>();
        let sources = self.library.folder_browser.configured_sample_sources();
        let mut requests = build_source_requests(operation_id, operation, changes, &sources);
        let lifecycle_generations = self.background.source_processing.lifecycle_generations();
        for request in &mut requests {
            request.lifecycle_generation = lifecycle_generations
                .get(request.source.id.as_str())
                .copied()
                .unwrap_or_default();
        }
        if requests.is_empty() {
            let mut failures = failures;
            if had_changes {
                failures.push(FileMutationFailure {
                    source_id: None,
                    operation_id,
                    operation,
                    error: String::from("No configured source owns the committed mutation paths"),
                });
            }
            self.finish_committed_file_mutation(
                FileMutationOutcome::Failed {
                    committed: Vec::new(),
                    failures,
                },
                context,
            );
            return Some(operation_id);
        }
        context.emit(GuiMessage::CommittedFileMutationRequested(
            FileMutationWork { requests, failures },
        ));
        Some(operation_id)
    }

    pub(in crate::native_app) fn start_committed_file_mutation(
        &mut self,
        work: FileMutationWork,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        context
            .business()
            .background("gui-committed-file-mutation")
            .run(
                move |_| {
                    merge_file_mutation_failures(
                        reconcile_file_mutation_requests(work.requests),
                        work.failures,
                    )
                },
                GuiMessage::CommittedFileMutationFinished,
            );
    }

    pub(in crate::native_app) fn record_failed_file_mutation(
        &mut self,
        operation: FileMutationOperation,
        source_id: Option<String>,
        error: impl Into<String>,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let operation_id = self.background.next_task_id();
        self.finish_committed_file_mutation(
            FileMutationOutcome::Failed {
                committed: Vec::new(),
                failures: vec![FileMutationFailure {
                    source_id,
                    operation_id,
                    operation,
                    error: error.into(),
                }],
            },
            context,
        );
    }

    pub(in crate::native_app) fn record_rolled_back_file_mutation(
        &mut self,
        operation: FileMutationOperation,
        source_id: Option<String>,
        error: impl Into<String>,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let operation_id = self.background.next_task_id();
        self.finish_committed_file_mutation(
            FileMutationOutcome::RolledBack(FileMutationFailure {
                source_id,
                operation_id,
                operation,
                error: error.into(),
            }),
            context,
        );
    }

    pub(in crate::native_app) fn finish_committed_file_mutation(
        &mut self,
        outcome: FileMutationOutcome,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let (committed, failures) = match outcome {
            FileMutationOutcome::Committed(committed) => (committed, Vec::new()),
            FileMutationOutcome::Failed {
                committed,
                failures,
            } => (committed, failures),
            FileMutationOutcome::RolledBack(failure) => {
                tracing::warn!(
                    operation_id = failure.operation_id,
                    operation = failure.operation.as_str(),
                    source_id = failure.source_id.as_deref().unwrap_or("unknown"),
                    error = %failure.error,
                    "Wavecrate-owned file mutation rolled back"
                );
                return;
            }
        };

        for event in committed {
            let current_lifecycle_generation = self
                .background
                .source_lifecycle_generations
                .get(&event.source_id)
                .copied();
            let test_fixture_lifecycle = cfg!(test)
                && current_lifecycle_generation.is_none()
                && event.lifecycle_generation == 0;
            if current_lifecycle_generation != Some(event.lifecycle_generation)
                && !test_fixture_lifecycle
            {
                tracing::debug!(
                    source_id = %event.source_id,
                    operation_id = event.operation_id,
                    event_lifecycle_generation = event.lifecycle_generation,
                    current_lifecycle_generation = ?current_lifecycle_generation,
                    "Ignoring committed file-mutation completion from a stale source lifecycle"
                );
                continue;
            }
            let last_commit = self
                .transactions
                .latest_committed_mutation
                .entry(event.source_id.clone())
                .or_default();
            let current_commit = (event.committed_source_revision, event.operation_id);
            let accepted_commit = *last_commit;
            if mutation_completion_is_stale_or_duplicate(accepted_commit, current_commit) {
                tracing::debug!(
                    source_id = %event.source_id,
                    operation_id = event.operation_id,
                    revision = event.committed_source_revision,
                    accepted_revision = accepted_commit.0,
                    accepted_operation_id = accepted_commit.1,
                    "Ignoring stale committed file-mutation completion"
                );
                continue;
            }
            let extraction_post_commit = event
                .changes
                .iter()
                .find_map(|change| change.post_commit.clone());
            let extraction_path = event
                .changes
                .iter()
                .find_map(|change| change.after_path.clone());
            let browser_projection_applied =
                event
                    .browser_projection_delta
                    .clone()
                    .is_some_and(|projection| {
                        self.library
                            .folder_browser
                            .apply_committed_projection_delta(&event.source_id, projection)
                    });
            if event.operation == FileMutationOperation::Extract && !browser_projection_applied {
                // This is the conservative recovery path: the exact committed projection could
                // not be applied at the expected revision, so do not acknowledge the watcher or
                // publish readiness. Affected-path refresh keeps the durable metadata completion
                // visible while the queued source refresh repairs the missing projection.
                self.library
                    .folder_browser
                    .refresh_filesystem_paths(&event.source_id, &event.affected_relative_paths);
                if let (Some(post_commit), Some(path)) =
                    (extraction_post_commit.as_ref(), extraction_path.as_deref())
                {
                    let metadata_error = self.finish_committed_playmark_extraction_metadata(
                        path,
                        post_commit,
                        context,
                    );
                    self.reapply_desired_rating_overlay();
                    self.finish_committed_playmark_extraction_visuals(
                        path,
                        post_commit,
                        metadata_error.as_deref(),
                    );
                }
                self.queue_full_source_reconciliation_after_committed_mutation(
                    event.source_id.clone(),
                    event.committed_source_revision,
                    event.lifecycle_generation,
                    context,
                );
                continue;
            }
            self.transactions
                .latest_committed_mutation
                .insert(event.source_id.clone(), accepted_commit.max(current_commit));
            for change in &event.changes {
                let before = change.before_path.as_deref().and_then(|path| {
                    self.library
                        .folder_browser
                        .source_database_relative_file_path(path)
                        .map(|(_, _, relative)| relative)
                });
                let after = change.after_path.as_deref().and_then(|path| {
                    self.library
                        .folder_browser
                        .source_database_relative_file_path(path)
                        .map(|(_, _, relative)| relative)
                });
                match (
                    change.before_path.as_deref(),
                    before,
                    change.after_path.as_deref(),
                    after,
                ) {
                    (Some(_), Some(before), Some(after_path), Some(after))
                        if change.semantics == FileMutationSemantics::PathOnlyMove =>
                    {
                        let after_source = self
                            .library
                            .folder_browser
                            .source_id_for_file_path(after_path);
                        if after_source.as_deref() == Some(event.source_id.as_str()) {
                            self.background.rating_persist.rekey_prefix(
                                &event.source_id,
                                &before,
                                &after,
                                false,
                            );
                        } else if let Some(after_source) = after_source {
                            if let Some((root, database_root, _)) = self
                                .library
                                .folder_browser
                                .source_database_relative_file_path(after_path)
                            {
                                self.background.rating_persist.rekey_cross_source(
                                    &event.source_id,
                                    &before,
                                    &after_source,
                                    &after,
                                    &root,
                                    &database_root,
                                );
                            }
                        } else {
                            self.background
                                .rating_persist
                                .invalidate_prefix(&event.source_id, &before);
                        }
                    }
                    (Some(_), Some(before), Some(_), Some(after)) if before == after => {
                        self.background.rating_persist.rekey_exact(
                            &event.source_id,
                            &before,
                            &after,
                        );
                    }
                    (Some(_), Some(before), None, None) => {
                        self.background
                            .rating_persist
                            .invalidate_prefix(&event.source_id, &before);
                    }
                    _ => {}
                }
            }
            let extraction_metadata_error =
                match (&extraction_post_commit, extraction_path.as_deref()) {
                    (Some(post_commit), Some(path))
                        if event.operation == FileMutationOperation::Extract =>
                    {
                        self.finish_committed_playmark_extraction_metadata(
                            path,
                            post_commit,
                            context,
                        )
                    }
                    _ => None,
                };
            self.reapply_desired_rating_overlay();

            let projections = event
                .changes
                .iter()
                .filter_map(|change| change.projection.as_ref())
                .collect::<Vec<_>>();
            if !browser_projection_applied
                && !projections
                    .iter()
                    .any(|projection| projection.replaces_default_refresh())
            {
                self.library
                    .folder_browser
                    .refresh_filesystem_paths(&event.source_id, &event.affected_relative_paths);
            }
            for projection in projections {
                self.apply_committed_file_mutation_projection(projection, context);
            }
            if let (Some(post_commit), Some(path)) =
                (extraction_post_commit.as_ref(), extraction_path.as_deref())
            {
                self.finish_committed_playmark_extraction_visuals(
                    path,
                    post_commit,
                    extraction_metadata_error.as_deref(),
                );
            }
            if let Some(watcher) = self.library.source_watcher.as_ref() {
                watcher.acknowledge_committed_paths(
                    event.source_id.clone(),
                    event.watcher_echoes,
                    event.operation_id,
                );
            }
            tracing::info!(
                source_id = %event.source_id,
                operation_id = event.operation_id,
                operation = event.operation.as_str(),
                revision = event.committed_source_revision,
                changes = event.changes.len(),
                invalidated_stages = ?event.invalidated_stages,
                "Committed Wavecrate-owned file mutation"
            );
            self.background.source_processing.request_source_delta(
                &event.source_id,
                event.lifecycle_generation,
                &event.committed_delta,
                "committed_file_mutation_delta",
            );
            // This call refreshes metadata projections and wakes the source-owned readiness
            // reconciler. It deliberately happens after the source DB and browser projection.
            self.queue_source_prep(
                event.source_id,
                if extraction_post_commit.is_some() {
                    COMMITTED_PLAYMARK_PREP_INTENTS
                } else {
                    COMMITTED_MUTATION_PREP_INTENTS
                },
                COMMITTED_MUTATION_PREP_REASON,
                context,
            );
        }

        for failure in failures {
            tracing::warn!(
                operation_id = failure.operation_id,
                operation = failure.operation.as_str(),
                source_id = failure.source_id.as_deref().unwrap_or("unknown"),
                error = %failure.error,
                "Wavecrate-owned file mutation failed before authoritative publication"
            );
        }
    }

    fn apply_committed_file_mutation_projection(
        &mut self,
        projection: &FileMutationProjection,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        match projection {
            FileMutationProjection::SelectAndFollow { path } => {
                self.library
                    .folder_browser
                    .select_file(path.to_string_lossy().to_string());
                self.library
                    .folder_browser
                    .follow_selected_file_view_matching_tags(12, 6, 2, &self.metadata.tags_by_file);
            }
            FileMutationProjection::SelectAndLoad { path } => {
                let path = path.to_string_lossy().to_string();
                self.library.folder_browser.select_file(path.clone());
                self.load_navigation_sample(path, context);
            }
            FileMutationProjection::FocusAndLoad { path, reason } => {
                self.library
                    .folder_browser
                    .focus_file_across_sources_matching_tags_for_reason(
                        path,
                        &self.metadata.tags_by_file,
                        *reason,
                    );
                self.load_navigation_sample(path.to_string_lossy().to_string(), context);
            }
            FileMutationProjection::LoadSelectedIfChanged {
                previous_selected, ..
            } => {
                let Some(selected) = self
                    .library
                    .folder_browser
                    .selected_file_id()
                    .map(str::to_owned)
                else {
                    return;
                };
                if previous_selected.as_deref() == Some(selected.as_str()) {
                    return;
                }
                self.cancel_metadata_tag_entry();
                self.metadata.selected_tag = None;
                self.load_navigation_sample(selected, context);
            }
            FileMutationProjection::RenameCompletion { completion, .. } => {
                self.apply_committed_folder_browser_rename(completion.clone(), context);
            }
            FileMutationProjection::MoveCompletion {
                cut_paste,
                request,
                success,
                previous_selected,
                started_at,
                ..
            } => {
                self.apply_committed_folder_move(
                    *cut_paste,
                    request.clone(),
                    success.clone(),
                    previous_selected.clone(),
                    *started_at,
                    context,
                );
            }
            FileMutationProjection::MoveConflictCompletion {
                completion,
                previous_selected,
                started_at,
                ..
            } => {
                self.apply_committed_file_move_conflict(
                    completion.clone(),
                    previous_selected.clone(),
                    *started_at,
                    context,
                );
            }
            FileMutationProjection::MoveTransaction {
                source_root,
                source_database_root,
                moves,
                ..
            } => {
                self.apply_committed_folder_move_transaction(
                    source_root,
                    source_database_root,
                    moves,
                );
            }
            FileMutationProjection::TrashFolder { path } => {
                self.library
                    .folder_browser
                    .discard_trashed_folder_path(path);
                self.clear_loaded_sample_if_path_within(path);
            }
            FileMutationProjection::TrashFiles {
                reconciled_paths,
                failed_paths,
                previous_selected,
                loaded_removed,
                status,
                ..
            } => {
                let discarded = self
                    .library
                    .folder_browser
                    .discard_trashed_file_paths_matching_tags_preserving_selection(
                        reconciled_paths,
                        &self.metadata.tags_by_file,
                        failed_paths,
                    );
                let selected_after_trash = discarded
                    .then(|| {
                        self.library
                            .folder_browser
                            .selected_file_id()
                            .map(str::to_owned)
                    })
                    .flatten();
                let focus_changed =
                    discarded && previous_selected.as_deref() != selected_after_trash.as_deref();
                for path in reconciled_paths {
                    self.clear_loaded_sample_if_exact(path);
                }
                self.load_selected_sample_after_trash_if_needed(
                    selected_after_trash,
                    focus_changed,
                    *loaded_removed,
                    context,
                );
                self.ui.status.sample = status.clone();
            }
        }
    }
}
