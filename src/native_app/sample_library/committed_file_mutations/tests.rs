use std::{fs, path::Path, sync::atomic::AtomicBool};

use wavecrate::sample_sources::scanner::ManifestIdentityDelta;
use wavecrate::sample_sources::{SampleSource, SourceDatabase, SourceId, scanner};

use super::watcher_echo::capture_expected_path_state;
use super::watcher_echo::watcher_echoes_for_changes;
use super::worker::{
    SourceMutationRequest, build_source_requests, capture_expected_filesystem_state,
    merge_file_mutation_failures, mutation_completion_is_stale_or_duplicate,
    reconcile_file_mutation_requests_with_database_roots, reconcile_source_mutation,
};
use super::*;

fn request(
    root: &Path,
    operation: FileMutationOperation,
    mut changes: Vec<FileMutationChange>,
) -> SourceMutationRequest {
    capture_expected_filesystem_state(&mut changes);
    SourceMutationRequest {
        source: SampleSource::new_with_id(SourceId::from_string("source-a"), root.to_path_buf()),
        operation_id: 42,
        operation,
        affected_relative_paths: changes
            .iter()
            .flat_map(|change| {
                [change.before_path.as_deref(), change.after_path.as_deref()]
                    .into_iter()
                    .flatten()
            })
            .filter_map(|path| path.strip_prefix(root).ok().map(Path::to_path_buf))
            .collect(),
        watcher_echoes: watcher_echoes_for_changes(root, &changes),
        changes,
    }
}

#[test]
fn large_file_mutation_fence_preserves_pre_epoch_modified_time() {
    let root = tempfile::tempdir().expect("source root");
    let path = root.path().join("large.wav");
    fs::write(&path, vec![7_u8; 9 * 1024 * 1024]).expect("create large file");
    filetime::set_file_mtime(&path, filetime::FileTime::from_unix_time(-1, 0))
        .expect("set pre-epoch modified time");

    let state = capture_expected_path_state(&path);
    assert!(matches!(
        state,
        ExpectedMutationPathState::Metadata {
            modified_ns: Some(-1_000_000_000),
            ..
        }
    ));
}

fn reconcile_test_request(
    request: SourceMutationRequest,
    cancel: &AtomicBool,
) -> Result<CommittedFileMutation, String> {
    let database_root = request.source.root.clone();
    reconcile_source_mutation(request, database_root, cancel)
}

#[test]
fn create_commits_revision_and_invalidates_file_readiness() {
    let root = tempfile::tempdir().expect("source root");
    let path = root.path().join("created.wav");
    fs::write(&path, b"created").expect("create file");

    let event = reconcile_test_request(
        request(
            root.path(),
            FileMutationOperation::Duplicate,
            vec![FileMutationChange::created(path)],
        ),
        &AtomicBool::new(false),
    )
    .expect("commit create");

    assert!(event.committed_source_revision > 0);
    assert_eq!(event.committed_delta.created.len(), 1);
    assert!(
        event
            .invalidated_stages
            .contains(&ReadinessStage::IndexedIdentity)
    );
    assert!(event.changes[0].before_content_identity.is_none());
    assert!(event.changes[0].after_content_identity.is_some());
    assert!(matches!(
        event.watcher_echoes[0].expected_state,
        CommittedWatcherPathState::ContentHash(_)
    ));
}

#[test]
fn path_only_move_retains_content_identity_and_readiness_artifacts() {
    let root = tempfile::tempdir().expect("source root");
    let old_path = root.path().join("old.wav");
    let new_path = root.path().join("new.wav");
    fs::write(&old_path, b"same content").expect("create old");
    let database =
        SourceDatabase::open_for_test_fixture_source_write(root.path()).expect("source db");
    scanner::hard_rescan(&database).expect("initial scan");
    fs::rename(&old_path, &new_path).expect("move path");

    let event = reconcile_test_request(
        request(
            root.path(),
            FileMutationOperation::Move,
            vec![FileMutationChange::path_only_move(old_path, new_path)],
        ),
        &AtomicBool::new(false),
    )
    .expect("commit move");

    assert!(event.invalidated_stages.is_empty());
    assert_eq!(
        event.changes[0].before_content_identity,
        event.changes[0].after_content_identity
    );
    assert_eq!(event.committed_delta.moved.len(), 1);
}

#[test]
fn destructive_edit_carries_previous_and_current_content_identity() {
    let root = tempfile::tempdir().expect("source root");
    let path = root.path().join("edited.wav");
    fs::write(&path, b"before").expect("create file");
    let database =
        SourceDatabase::open_for_test_fixture_source_write(root.path()).expect("source db");
    scanner::hard_rescan(&database).expect("initial scan");
    fs::write(&path, b"after and different").expect("edit file");

    let event = reconcile_test_request(
        request(
            root.path(),
            FileMutationOperation::Edit,
            vec![FileMutationChange::content_changed(path)],
        ),
        &AtomicBool::new(false),
    )
    .expect("commit edit");

    assert_ne!(
        event.changes[0].before_content_identity,
        event.changes[0].after_content_identity
    );
    assert_eq!(event.committed_delta.changed.len(), 1);
    assert!(
        event
            .invalidated_stages
            .contains(&ReadinessStage::AnalysisFeatures)
    );
}

#[test]
fn delete_retires_manifest_identity_and_only_invalidates_membership() {
    let root = tempfile::tempdir().expect("source root");
    let path = root.path().join("deleted.wav");
    fs::write(&path, b"deleted").expect("create file");
    let database =
        SourceDatabase::open_for_test_fixture_source_write(root.path()).expect("source db");
    scanner::hard_rescan(&database).expect("initial scan");
    fs::remove_file(&path).expect("delete file");

    let event = reconcile_test_request(
        request(
            root.path(),
            FileMutationOperation::Trash,
            vec![FileMutationChange::deleted(path)],
        ),
        &AtomicBool::new(false),
    )
    .expect("commit delete");

    assert_eq!(event.committed_delta.deleted.len(), 1);
    assert!(event.changes[0].before_content_identity.is_some());
    assert!(event.changes[0].after_content_identity.is_none());
    assert_eq!(
        event.invalidated_stages,
        BTreeSet::from([ReadinessStage::SimilarityLayout])
    );
    assert_eq!(
        event.watcher_echoes[0].expected_state,
        CommittedWatcherPathState::Missing
    );
}

#[test]
fn large_create_commits_without_synchronous_deep_hashing() {
    let root = tempfile::tempdir().expect("source root");
    let path = root.path().join("large.wav");
    fs::write(&path, vec![7_u8; 9 * 1024 * 1024]).expect("create large file");

    let event = reconcile_test_request(
        request(
            root.path(),
            FileMutationOperation::ImportDrop,
            vec![FileMutationChange::created(path)],
        ),
        &AtomicBool::new(false),
    )
    .expect("commit large create");

    assert_eq!(event.committed_delta.created.len(), 1);
    assert!(
        event.committed_delta.created[0]
            .content_generation
            .starts_with("pending:")
    );
    assert!(
        event.watcher_echoes.is_empty(),
        "large files use conservative watcher reconciliation instead of synchronous hashing"
    );
}

#[test]
fn cross_source_requests_keep_one_operation_id_and_distinct_sources() {
    let first = tempfile::tempdir().expect("first source");
    let second = tempfile::tempdir().expect("second source");
    let before = first.path().join("sample.wav");
    let after = second.path().join("sample.wav");
    let sources = vec![
        SampleSource::new(first.path().to_path_buf()),
        SampleSource::new(second.path().to_path_buf()),
    ];

    let requests = build_source_requests(
        88,
        FileMutationOperation::Move,
        vec![FileMutationChange::path_only_move(before, after)],
        &sources,
    );

    assert_eq!(requests.len(), 2);
    assert!(requests.iter().all(|request| request.operation_id == 88));
    assert_ne!(requests[0].source.id, requests[1].source.id);
}

#[test]
fn cross_source_database_root_failure_keeps_valid_commit_and_explicit_failure() {
    let first = tempfile::tempdir().expect("first source");
    let second = tempfile::tempdir().expect("second source");
    let before = first.path().join("sample.wav");
    let after = second.path().join("sample.wav");
    fs::write(&before, b"same content").expect("create source file");
    let first_database =
        SourceDatabase::open_for_test_fixture_source_write(first.path()).expect("first source db");
    scanner::hard_rescan(&first_database).expect("initial first source scan");
    fs::rename(&before, &after).expect("move across sources");

    let first_source = SampleSource::new_with_id(
        SourceId::from_string("source-a"),
        first.path().to_path_buf(),
    );
    let second_source = SampleSource::new_with_id(
        SourceId::from_string("source-b"),
        second.path().to_path_buf(),
    );
    let requests = build_source_requests(
        88,
        FileMutationOperation::Move,
        vec![FileMutationChange::path_only_move(before, after)],
        &[first_source.clone(), second_source.clone()],
    );

    let outcome = reconcile_file_mutation_requests_with_database_roots(requests, |source| {
        if source.id == second_source.id {
            Err(String::from("metadata root unavailable"))
        } else {
            Ok(source.root.clone())
        }
    });

    let FileMutationOutcome::Failed {
        committed,
        failures,
    } = outcome
    else {
        panic!("cross-source partial failure must be explicit");
    };
    assert_eq!(committed.len(), 1);
    assert_eq!(committed[0].source_id, first_source.id.as_str());
    assert_eq!(failures.len(), 1);
    assert_eq!(
        failures[0].source_id.as_deref(),
        Some(second_source.id.as_str())
    );
    assert!(failures[0].error.contains("metadata root unavailable"));
}

#[test]
fn failed_reconciliation_does_not_apply_browser_projection() {
    let root = tempfile::tempdir().expect("source root");
    let selected = root.path().join("selected.wav");
    let created = root.path().join("created.wav");
    fs::write(&selected, b"selected").expect("selected file");
    fs::write(&created, b"created").expect("created file");
    let source = SampleSource::new(root.path().to_path_buf());
    let browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            source.clone()
        ]);
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(browser)
        .build();
    state
        .library
        .folder_browser
        .select_file(selected.to_string_lossy().to_string());
    let mut changes = vec![
        FileMutationChange::created(created.clone()).with_projection(
            FileMutationProjection::SelectAndFollow {
                path: created.clone(),
            },
        ),
    ];
    capture_expected_filesystem_state(&mut changes);
    let requests = build_source_requests(91, FileMutationOperation::Duplicate, changes, &[source]);
    let outcome = reconcile_file_mutation_requests_with_database_roots(requests, |_| {
        Err(String::from("metadata root unavailable"))
    });

    state
        .finish_committed_file_mutation(outcome, &mut radiant::prelude::UiUpdateContext::default());

    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(selected.to_string_lossy().as_ref())
    );
}

#[test]
fn failed_and_rolled_back_outcomes_are_explicit() {
    let failure = FileMutationFailure {
        source_id: Some(String::from("source-a")),
        operation_id: 9,
        operation: FileMutationOperation::Move,
        error: String::from("rolled back"),
    };
    assert!(matches!(
        FileMutationOutcome::RolledBack(failure.clone()),
        FileMutationOutcome::RolledBack(_)
    ));
    assert!(matches!(
        FileMutationOutcome::Failed {
            committed: Vec::new(),
            failures: vec![failure],
        },
        FileMutationOutcome::Failed { .. }
    ));
}

#[test]
fn content_only_delta_is_still_a_committed_authoritative_event() {
    let delta = CommittedSourceDelta {
        revision: 7,
        changed: vec![ManifestIdentityDelta {
            identity: String::from("file:test"),
            relative_path: PathBuf::from("sample.wav"),
            content_generation: String::from("hash:new"),
            source_metadata_changed: false,
        }],
        ..CommittedSourceDelta::default()
    };
    assert!(!delta.is_empty());
}

#[test]
fn stale_and_duplicate_completions_are_fenced_by_revision_then_operation() {
    assert!(!mutation_completion_is_stale_or_duplicate((0, 0), (0, 1)));
    assert!(mutation_completion_is_stale_or_duplicate((7, 11), (7, 10)));
    assert!(mutation_completion_is_stale_or_duplicate((7, 11), (7, 11)));
    assert!(!mutation_completion_is_stale_or_duplicate((7, 11), (8, 2)));
}

#[test]
fn partial_failure_keeps_commits_under_one_operation_outcome() {
    let failure = FileMutationFailure {
        source_id: Some(String::from("source-a")),
        operation_id: 9,
        operation: FileMutationOperation::Normalize,
        error: String::from("one file failed"),
    };
    let merged = merge_file_mutation_failures(
        FileMutationOutcome::Committed(Vec::new()),
        vec![failure.clone()],
    );
    assert_eq!(
        merged,
        FileMutationOutcome::Failed {
            committed: Vec::new(),
            failures: vec![failure],
        }
    );
}

#[test]
fn rapid_repeated_edit_fences_the_superseded_completion() {
    let root = tempfile::tempdir().expect("source root");
    let path = root.path().join("edited.wav");
    fs::write(&path, b"first committed edit").expect("first edit");
    let changes = vec![FileMutationChange::content_changed(path.clone())];
    let request = request(root.path(), FileMutationOperation::Edit, changes);

    fs::write(&path, b"second committed edit with different size").expect("second edit");
    let error = reconcile_test_request(request, &AtomicBool::new(false))
        .expect_err("superseded edit must not publish");

    assert!(error.contains("superseded"));
}

#[test]
fn intervening_equal_size_rewrite_with_preserved_mtime_is_not_acknowledged_as_owned() {
    let root = tempfile::tempdir().expect("source root");
    let path = root.path().join("edited.wav");
    fs::write(&path, b"owned-content").expect("owned edit");
    let committed_modified = fs::metadata(&path)
        .expect("owned metadata")
        .modified()
        .expect("owned modified time");
    let request = request(
        root.path(),
        FileMutationOperation::Edit,
        vec![FileMutationChange::content_changed(path.clone())],
    );

    fs::write(&path, b"other-content").expect("intervening edit");
    fs::File::options()
        .write(true)
        .open(&path)
        .expect("open intervening edit")
        .set_times(std::fs::FileTimes::new().set_modified(committed_modified))
        .expect("preserve modified time");

    let error = reconcile_test_request(request, &AtomicBool::new(false))
        .expect_err("intervening content must not publish as the owned edit");
    assert!(error.contains("superseded"));
}
