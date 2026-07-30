use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

use wavecrate::sample_sources::{SampleSource, SourceDatabase, readiness::ReadinessStage};
use wavecrate_library::timestamps::system_time_to_unix_nanos;

use super::watcher_echo::{capture_expected_path_state, watcher_echoes_for_changes};
use super::{
    CommittedFileMutation, ExpectedMutationPathState, FileMutationChange, FileMutationFailure,
    FileMutationOperation, FileMutationOutcome, FileMutationSemantics,
};
use crate::native_app::sample_library::folder_scan_actions::sync_source_database_paths;
use crate::native_app::source_processing::{ExternalScanHandoff, SourceProcessingBudgetHandle};

#[derive(Clone, Debug)]
pub(super) struct SourceMutationRequest {
    pub(super) source: SampleSource,
    pub(super) lifecycle_generation: u64,
    pub(super) operation_id: u64,
    pub(super) operation: FileMutationOperation,
    pub(super) changes: Vec<FileMutationChange>,
    pub(super) affected_relative_paths: Vec<PathBuf>,
    pub(super) watcher_echoes: Vec<super::CommittedWatcherEcho>,
}

impl PartialEq for SourceMutationRequest {
    fn eq(&self, other: &Self) -> bool {
        self.source.id == other.source.id
            && self.source.root == other.source.root
            && self.lifecycle_generation == other.lifecycle_generation
            && self.source.is_protected() == other.source.is_protected()
            && self.source.is_primary() == other.source.is_primary()
            && self.source.primary_import_path() == other.source.primary_import_path()
            && self.operation_id == other.operation_id
            && self.operation == other.operation
            && self.changes == other.changes
            && self.affected_relative_paths == other.affected_relative_paths
            && self.watcher_echoes == other.watcher_echoes
    }
}

impl Eq for SourceMutationRequest {}

pub(super) fn mutation_completion_is_stale_or_duplicate(
    accepted: (u64, u64),
    candidate: (u64, u64),
) -> bool {
    accepted != (0, 0) && candidate <= accepted
}

pub(super) fn merge_file_mutation_failures(
    outcome: FileMutationOutcome,
    mut reported_failures: Vec<FileMutationFailure>,
) -> FileMutationOutcome {
    if reported_failures.is_empty() {
        return outcome;
    }
    match outcome {
        FileMutationOutcome::Committed(committed) => FileMutationOutcome::Failed {
            committed,
            failures: reported_failures,
        },
        FileMutationOutcome::Failed {
            committed,
            mut failures,
        } => {
            failures.append(&mut reported_failures);
            FileMutationOutcome::Failed {
                committed,
                failures,
            }
        }
        FileMutationOutcome::RolledBack(failure) => {
            reported_failures.insert(0, failure);
            FileMutationOutcome::Failed {
                committed: Vec::new(),
                failures: reported_failures,
            }
        }
    }
}

pub(super) fn capture_expected_filesystem_state(changes: &mut [FileMutationChange]) {
    for change in changes {
        if change.after_content_identity.is_none() {
            change.after_content_identity = change
                .after_path
                .as_deref()
                .and_then(cache_content_identity);
        }
        if change.before_path != change.after_path {
            change.expected_before_state = change
                .before_path
                .as_deref()
                .map(|_| ExpectedMutationPathState::Missing);
        }
        change.expected_after_state = change
            .after_path
            .as_deref()
            .map(capture_expected_path_state);
    }
}

pub(in crate::native_app) fn cache_content_identity(path: &Path) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified_ns = system_time_to_unix_nanos(metadata.modified().ok()?);
    Some(format!("cache:{}:{modified_ns}", metadata.len()))
}

pub(super) fn build_source_requests(
    operation_id: u64,
    operation: FileMutationOperation,
    changes: Vec<FileMutationChange>,
    sources: &[SampleSource],
) -> Vec<SourceMutationRequest> {
    let mut grouped = BTreeMap::<String, SourceMutationRequest>::new();
    for change in changes {
        let affected_sources = [change.before_path.as_deref(), change.after_path.as_deref()]
            .into_iter()
            .flatten()
            .filter_map(|path| source_for_path(sources, path))
            .collect::<BTreeSet<_>>();
        for source_id in affected_sources {
            let Some(source) = sources
                .iter()
                .find(|source| source.id.as_str() == source_id)
            else {
                continue;
            };
            let request =
                grouped
                    .entry(source_id.clone())
                    .or_insert_with(|| SourceMutationRequest {
                        source: source.clone(),
                        lifecycle_generation: 0,
                        operation_id,
                        operation,
                        changes: Vec::new(),
                        affected_relative_paths: Vec::new(),
                        watcher_echoes: Vec::new(),
                    });
            for path in [change.before_path.as_deref(), change.after_path.as_deref()]
                .into_iter()
                .flatten()
            {
                if let Ok(relative) = path.strip_prefix(&source.root)
                    && !request
                        .affected_relative_paths
                        .iter()
                        .any(|existing| existing == relative)
                {
                    request.affected_relative_paths.push(relative.to_path_buf());
                }
            }
            let mut source_change = change.clone();
            source_change.retain_projection_for_source(&source.root);
            request.changes.push(source_change);
        }
    }
    grouped
        .into_values()
        .map(|mut request| {
            request.watcher_echoes =
                watcher_echoes_for_changes(&request.source.root, &request.changes);
            request
        })
        .collect()
}

fn source_for_path(sources: &[SampleSource], path: &Path) -> Option<String> {
    sources
        .iter()
        .filter_map(|source| {
            path.strip_prefix(&source.root)
                .ok()
                .map(|relative| (source, relative))
        })
        .max_by_key(|(source, _)| source.root.components().count())
        .map(|(source, _)| source.id.as_str().to_string())
}

#[cfg(test)]
pub(super) fn reconcile_file_mutation_requests(
    requests: Vec<SourceMutationRequest>,
) -> FileMutationOutcome {
    reconcile_file_mutation_requests_with_database_roots(requests, |source| {
        source
            .database_root()
            .map_err(|error| format!("resolve source metadata location: {error}"))
    })
}

pub(super) fn reconcile_file_mutation_requests_with_handoff(
    requests: Vec<SourceMutationRequest>,
    budget: SourceProcessingBudgetHandle,
) -> FileMutationOutcome {
    let mut committed = Vec::new();
    let mut failures = Vec::new();
    for request in requests {
        let source_id = request.source.id.as_str().to_string();
        let Some(permit) =
            budget.acquire_scan_for_generation(&source_id, request.lifecycle_generation)
        else {
            failures.push(FileMutationFailure {
                source_id: Some(source_id),
                operation_id: request.operation_id,
                operation: request.operation,
                error: String::from("source mutation reconciliation canceled"),
            });
            continue;
        };
        match request
            .source
            .database_root()
            .map_err(|error| format!("resolve source metadata location: {error}"))
            .and_then(|database_root| {
                reconcile_source_mutation_with_cancel(
                    request.clone(),
                    database_root,
                    &permit.cancel_token(),
                )
            }) {
            Ok(mut event) => {
                if event.browser_projection_delta.is_some() {
                    let ticket =
                        permit.release_after_projection_handoff(event.committed_delta.clone());
                    event.projection_handoff_ticket = Some(ticket);
                } else {
                    permit.release_after_handoff(ExternalScanHandoff::FullReconciliation {
                        reason: "committed_mutation_projection_unavailable",
                    });
                }
                committed.push(event);
            }
            Err(error) => {
                permit.release_after_handoff(ExternalScanHandoff::FullReconciliation {
                    reason: "committed_mutation_reconciliation_failed",
                });
                failures.push(FileMutationFailure {
                    source_id: Some(source_id),
                    operation_id: request.operation_id,
                    operation: request.operation,
                    error,
                });
            }
        }
    }
    if failures.is_empty() {
        FileMutationOutcome::Committed(committed)
    } else {
        FileMutationOutcome::Failed {
            committed,
            failures,
        }
    }
}

#[cfg(test)]
pub(super) fn reconcile_file_mutation_requests_with_database_roots(
    requests: Vec<SourceMutationRequest>,
    database_root_for: impl Fn(&SampleSource) -> Result<PathBuf, String>,
) -> FileMutationOutcome {
    let cancel = AtomicBool::new(false);
    let mut committed = Vec::new();
    let mut failures = Vec::new();
    for request in requests {
        let source_id = request.source.id.as_str().to_string();
        let result = database_root_for(&request.source).and_then(|database_root| {
            reconcile_source_mutation(request.clone(), database_root, &cancel)
        });
        match result {
            Ok(event) => committed.push(event),
            Err(error) => failures.push(FileMutationFailure {
                source_id: Some(source_id),
                operation_id: request.operation_id,
                operation: request.operation,
                error,
            }),
        }
    }
    if failures.is_empty() {
        FileMutationOutcome::Committed(committed)
    } else {
        FileMutationOutcome::Failed {
            committed,
            failures,
        }
    }
}

#[cfg(test)]
pub(super) fn reconcile_source_mutation(
    request: SourceMutationRequest,
    database_root: PathBuf,
    cancel: &AtomicBool,
) -> Result<CommittedFileMutation, String> {
    reconcile_source_mutation_with_cancel(request, database_root, &cancel)
}

fn reconcile_source_mutation_with_cancel(
    request: SourceMutationRequest,
    database_root: PathBuf,
    cancel: &AtomicBool,
) -> Result<CommittedFileMutation, String> {
    let root = &request.source.root;
    verify_mutation_still_matches_filesystem(&request.changes)?;
    let before_database =
        SourceDatabase::open_for_background_job_with_database_root(root, &database_root)
            .map_err(|error| format!("open source before mutation reconciliation: {error}"))?;
    let (_, before_entries) = before_database
        .manifest_snapshot_with_revision_under_paths(&request.affected_relative_paths)
        .map_err(|error| format!("read affected source manifest before mutation: {error}"))?;
    let before = manifest_by_path(before_entries);
    drop(before_database);

    let sync = sync_source_database_paths(
        request.source.id.as_str().to_string(),
        root.clone(),
        database_root.clone(),
        request.affected_relative_paths.clone(),
        request.affected_relative_paths.len(),
        cancel,
    );
    let success = sync.result?;
    if let Some(error) = success.incomplete_error {
        return Err(format!(
            "source mutation reconciliation incomplete: {error}"
        ));
    }

    let after_database =
        SourceDatabase::open_for_background_job_with_database_root(root, &database_root)
            .map_err(|error| format!("open source after mutation reconciliation: {error}"))?;
    let (_, after_entries) = after_database
        .manifest_snapshot_with_revision_under_paths(&request.affected_relative_paths)
        .map_err(|error| format!("read affected source manifest after mutation: {error}"))?;
    let after = manifest_by_path(after_entries);
    let committed_source_revision = after_database
        .get_revision()
        .map_err(|error| format!("read committed source revision: {error}"))?
        .max(success.committed_delta.revision);

    verify_mutation_still_matches_filesystem(&request.changes)?;
    let changes = request
        .changes
        .into_iter()
        .map(|mut change| {
            let before_entry = change
                .before_path
                .as_deref()
                .and_then(|path| path.strip_prefix(root).ok())
                .and_then(|path| before.get(path));
            let after_entry = change
                .after_path
                .as_deref()
                .and_then(|path| path.strip_prefix(root).ok())
                .and_then(|path| after.get(path));
            if change.before_content_identity.is_none() {
                change.before_content_identity = before_entry.map(content_identity);
            }
            if change
                .after_content_identity
                .as_deref()
                .is_none_or(|identity| identity.starts_with("cache:"))
            {
                change.after_content_identity = after_entry.map(content_identity);
            }
            if change.semantics == FileMutationSemantics::PathOnlyMove {
                if change.before_content_identity.is_none() {
                    change.before_content_identity = change.after_content_identity.clone();
                }
                if change.after_content_identity.is_none() {
                    change.after_content_identity = change.before_content_identity.clone();
                }
            }
            change
        })
        .collect::<Vec<_>>();

    Ok(CommittedFileMutation {
        source_id: request.source.id.as_str().to_string(),
        lifecycle_generation: request.lifecycle_generation,
        operation_id: request.operation_id,
        operation: request.operation,
        committed_source_revision,
        invalidated_stages: invalidated_stages(&changes),
        changes,
        committed_delta: success.committed_delta,
        affected_relative_paths: request.affected_relative_paths,
        watcher_echoes: request.watcher_echoes,
        browser_projection_delta: success.browser_projection_delta,
        projection_handoff_ticket: None,
    })
}

fn verify_mutation_still_matches_filesystem(changes: &[FileMutationChange]) -> Result<(), String> {
    for change in changes {
        for (path, expected) in [
            (
                change.before_path.as_deref(),
                change.expected_before_state.as_ref(),
            ),
            (
                change.after_path.as_deref(),
                change.expected_after_state.as_ref(),
            ),
        ] {
            let (Some(path), Some(expected)) = (path, expected) else {
                continue;
            };
            if !expected_path_state_matches(path, expected) {
                return Err(String::from(
                    "committed mutation was superseded before source reconciliation",
                ));
            }
        }
    }
    Ok(())
}

fn expected_path_state_matches(path: &Path, expected: &ExpectedMutationPathState) -> bool {
    match (expected, capture_expected_path_state(path)) {
        (ExpectedMutationPathState::Missing, ExpectedMutationPathState::Missing) => true,
        (
            ExpectedMutationPathState::ContentHash(expected),
            ExpectedMutationPathState::ContentHash(current),
        ) => expected == &current,
        (
            ExpectedMutationPathState::Metadata {
                len: expected_len,
                modified_ns: expected_modified_ns,
                is_dir: expected_is_dir,
            },
            ExpectedMutationPathState::Metadata {
                len: current_len,
                modified_ns: current_modified_ns,
                is_dir: current_is_dir,
            },
        ) => {
            expected_len == &current_len
                && expected_modified_ns == &current_modified_ns
                && expected_is_dir == &current_is_dir
        }
        _ => false,
    }
}

fn manifest_by_path(
    entries: Vec<wavecrate_library::sample_sources::SourceManifestEntry>,
) -> HashMap<PathBuf, wavecrate_library::sample_sources::SourceManifestEntry> {
    entries
        .into_iter()
        .map(|entry| (entry.relative_path.clone(), entry))
        .collect()
}

fn content_identity(entry: &wavecrate_library::sample_sources::SourceManifestEntry) -> String {
    entry
        .content_hash
        .as_deref()
        .filter(|hash| !hash.trim().is_empty())
        .map(|hash| format!("hash:{hash}"))
        .or_else(|| {
            entry
                .file_identity
                .as_deref()
                .filter(|identity| !identity.trim().is_empty())
                .map(|identity| {
                    format!(
                        "pending:{identity}:{}:{}",
                        entry.file_size, entry.modified_ns
                    )
                })
        })
        .unwrap_or_else(|| {
            format!(
                "pending:{}:{}:{}",
                entry.relative_path.display(),
                entry.file_size,
                entry.modified_ns
            )
        })
}

fn invalidated_stages(changes: &[FileMutationChange]) -> BTreeSet<ReadinessStage> {
    let mut stages = BTreeSet::new();
    for change in changes {
        match change.semantics {
            FileMutationSemantics::Create | FileMutationSemantics::ContentChanged => {
                stages.extend([
                    ReadinessStage::IndexedIdentity,
                    ReadinessStage::AnalysisFeatures,
                    ReadinessStage::EmbeddingAspects,
                    ReadinessStage::SimilarityLayout,
                ]);
            }
            FileMutationSemantics::PathOnlyMove => {}
            FileMutationSemantics::Delete => {
                stages.insert(ReadinessStage::SimilarityLayout);
            }
        }
    }
    stages
}
