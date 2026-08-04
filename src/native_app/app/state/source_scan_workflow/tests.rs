use std::fs;

use super::*;
use crate::native_app::sample_library::folder_browser::scan::{
    FolderScanProgress, FolderScanRequest, scan_source_with_progress,
};
use crate::native_app::sample_library::source_watcher::{WatcherBackend, WatcherContinuityProof};
use wavecrate::sample_sources::SourceId;
use wavecrate_library::sample_sources::reconciliation::{
    BackendStreamIdentity, CaptureBoundary, RawEventKind, RawObservation, RawObservationEnvelope,
    RawObservationLimits, RawObservationProvenance, RawObservedPath, RawPathHint, RawPathRole,
    ReconciliationScopeKind, RootIdentity, WatcherGeneration, normalize_observation,
};

fn temp_dir_with_wav() -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("source root");
    fs::write(root.path().join("sample.wav"), [0_u8; 8]).expect("write sample");
    root
}

fn replay_proof(end_event_id: u64) -> WatcherContinuityProof {
    WatcherContinuityProof {
        root_identity: String::from("root-a"),
        backend: WatcherBackend::Fsevents,
        backend_device: 10,
        watcher_generation: 4,
        replay_coverage_start_event_id: end_event_id.saturating_sub(1),
        replay_coverage_end_event_id: end_event_id,
        acknowledged_end_event_id: end_event_id,
    }
}

fn typed_scopes() -> Vec<ReconciliationScope> {
    let provenance = RawObservationProvenance::new(
        SourceId::from_string("source-a"),
        Some(RootIdentity::from_bytes(vec![1])),
        Some(BackendStreamIdentity::from_bytes(vec![2])),
        WatcherGeneration::new(4),
        CaptureBoundary::try_new(9, Some(8), Some(9)).expect("capture boundary"),
    );
    let envelope = RawObservationEnvelope::try_new(
        provenance,
        vec![
            RawObservation::new(
                RawEventKind::Modify,
                vec![RawObservedPath::new(
                    PathBuf::from("file.wav"),
                    RawPathRole::Subject,
                )],
            ),
            RawObservation::new(
                RawEventKind::Create,
                vec![
                    RawObservedPath::new(PathBuf::from("folder"), RawPathRole::Subject)
                        .with_hint(RawPathHint::Directory),
                ],
            ),
        ],
        RawObservationLimits::new(8, usize::MAX, usize::MAX).expect("observation limits"),
    )
    .expect("observation envelope");
    normalize_observation(envelope).scopes().to_vec()
}

fn source_audit_scope() -> Vec<ReconciliationScope> {
    let provenance = RawObservationProvenance::new(
        SourceId::from_string("source-a"),
        Some(RootIdentity::from_bytes(vec![1])),
        Some(BackendStreamIdentity::from_bytes(vec![2])),
        WatcherGeneration::new(4),
        CaptureBoundary::try_new(9, Some(8), Some(9)).expect("capture boundary"),
    );
    let envelope = RawObservationEnvelope::try_new(
        provenance,
        vec![RawObservation::new(RawEventKind::RootChanged, Vec::new())],
        RawObservationLimits::new(8, usize::MAX, usize::MAX).expect("observation limits"),
    )
    .expect("observation envelope");
    normalize_observation(envelope).scopes().to_vec()
}

#[test]
fn stale_progress_is_ignored() {
    let root = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let request = workflow
        .begin_add_source_path(&mut browser, root.path().to_path_buf(), 7)
        .expect("scan request");
    workflow.start_scan(&request);

    let stale = FolderScanProgress::new(
        request.task_id + 1,
        request.source_id.clone(),
        request.label.clone(),
        FolderScanLifecycle::Scanning,
        1,
        1,
        String::new(),
    );

    assert!(!workflow.apply_progress(&browser, stale));
    assert_eq!(
        workflow.progress().expect("queued progress").lifecycle,
        FolderScanLifecycle::Queued
    );
    assert_eq!(
        workflow.progress().expect("queued progress").detail,
        "Queued — preparing source scan"
    );
}

#[test]
fn stale_finish_keeps_active_scan_owner() {
    let root = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let request = workflow
        .begin_add_source_path(&mut browser, root.path().to_path_buf(), 11)
        .expect("scan request");
    workflow.start_scan(&request);
    let stale_result = scan_source_with_progress(
        FolderScanRequest {
            task_id: request.task_id + 1,
            source_id: request.source_id.clone(),
            label: request.label.clone(),
            root: request.root.clone(),
            database_root: request.database_root.clone(),
            rating_decay_weeks: request.rating_decay_weeks,
        },
        |_| {},
        |_| {},
    );

    assert!(matches!(
        workflow.finish_scan(&mut browser, stale_result),
        SourceScanFinish::Stale { .. }
    ));
    assert!(workflow.active());
}

#[test]
fn apply_and_persist_owners_advance_in_order_and_terminal_completion_retires_progress() {
    let root = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let request = workflow
        .begin_add_source_path(&mut browser, root.path().to_path_buf(), 12)
        .expect("scan request");
    let source_id = request.source_id.clone();
    workflow.start_scan(&request);
    assert!(workflow.transition_current_scan(
        request.task_id,
        &source_id,
        Some(3),
        FolderScanLifecycle::ApplyingResults,
        "Applying"
    ));
    let applying = workflow.progress().cloned().expect("applying progress");
    let result = scan_source_with_progress(request.clone(), |_| {}, |_| {});
    assert!(matches!(
        workflow.finish_scan_with_lifecycle(&mut browser, result, Some(3), true),
        SourceScanFinish::Applied { .. }
    ));
    assert!(
        !workflow.active(),
        "projection owner retires before persistence"
    );
    assert!(workflow.resume_progress_after_projection(applying));
    assert!(workflow.transition_current_scan(
        request.task_id,
        &source_id,
        Some(3),
        FolderScanLifecycle::PersistingResults,
        "Saving"
    ));
    assert!(
        workflow
            .finish_current_scan_terminal(
                request.task_id,
                &source_id,
                Some(3),
                FolderScanLifecycle::Complete,
            )
            .is_some()
    );
    assert!(!workflow.active());
}

#[test]
fn stale_maintenance_generation_cannot_retire_replacement_progress() {
    let root = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let request = workflow
        .begin_add_source_path(&mut browser, root.path().to_path_buf(), 13)
        .expect("scan request");
    let source_id = request.source_id.clone();
    workflow.start_scan(&request);
    assert!(workflow.transition_current_scan(
        request.task_id,
        &source_id,
        Some(5),
        FolderScanLifecycle::PersistingResults,
        "Saving"
    ));

    assert!(
        workflow
            .finish_current_scan_terminal(
                request.task_id,
                &source_id,
                Some(4),
                FolderScanLifecycle::Complete,
            )
            .is_none()
    );
    assert_eq!(
        workflow
            .progress()
            .expect("replacement progress")
            .lifecycle_generation,
        Some(5)
    );
}

#[test]
fn pending_refresh_waits_for_active_scan() {
    let root = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let request = workflow
        .begin_add_source_path(&mut browser, root.path().to_path_buf(), 21)
        .expect("scan request");
    let source_id = request.source_id.clone();
    workflow.start_scan(&request);

    let plan = workflow.plan_filesystem_change(&mut browser, source_id.clone(), &[], true, true);

    assert!(matches!(
        plan,
        SourceFilesystemChangePlan::DeferredAlreadyRunning { .. }
    ));
    assert_eq!(workflow.next_pending_refresh_if_idle(), None);
    let result = scan_source_with_progress(request, |_| {}, |_| {});
    assert!(matches!(
        workflow.finish_scan(&mut browser, result),
        SourceScanFinish::Applied { .. }
    ));
    assert_eq!(workflow.next_pending_refresh_if_idle(), Some(source_id));
}

#[test]
fn accepted_scan_discards_manifest_refresh_at_covered_revision() {
    let root = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let request = workflow
        .begin_add_source_path(&mut browser, root.path().to_path_buf(), 211)
        .expect("scan request");
    let source_id = request.source_id.clone();
    workflow.start_scan(&request);
    let result = scan_source_with_progress(request, |_| {}, |_| {});
    let accepted_revision = result
        .metadata_hydration
        .revision()
        .expect("authoritative scan revision");

    workflow.queue_required_refresh_with_context(
        source_id,
        SourceRefreshCause::ManifestAudit {
            committed_revision: accepted_revision,
        },
        Some(7),
    );

    assert!(matches!(
        workflow.finish_scan(&mut browser, result),
        SourceScanFinish::Applied { .. }
    ));
    assert_eq!(workflow.next_pending_refresh_if_idle(), None);
}

#[test]
fn current_projection_suppresses_revisioned_refresh_before_queue() {
    let root = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let request = workflow
        .begin_add_source_path(&mut browser, root.path().to_path_buf(), 214)
        .expect("scan request");
    let source_id = request.source_id.clone();
    workflow.start_scan(&request);
    let result = scan_source_with_progress(request, |_| {}, |_| {});
    let accepted_revision = result
        .metadata_hydration
        .revision()
        .expect("authoritative scan revision");
    workflow.finish_scan(&mut browser, result);

    assert!(matches!(
        workflow.begin_filesystem_refresh_with_context(
            &mut browser,
            source_id,
            215,
            SourceRefreshCause::ManifestAudit {
                committed_revision: accepted_revision,
            },
            Some(7),
        ),
        SourceRefreshRequest::Covered {
            accepted_revision: revision,
            ..
        } if revision == accepted_revision
    ));
    assert!(!workflow.active());
}

#[test]
fn two_source_revisioned_handoffs_converge_without_alternating_scans() {
    let first_root = temp_dir_with_wav();
    let second_root = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();

    let first = workflow
        .begin_add_source_path(&mut browser, first_root.path().to_path_buf(), 212)
        .expect("first source scan");
    let first_id = first.source_id.clone();
    workflow.start_scan(&first);
    let first_result = scan_source_with_progress(first, |_| {}, |_| {});
    let first_revision = first_result
        .metadata_hydration
        .revision()
        .expect("first revision");
    workflow.queue_required_refresh_with_context(
        first_id,
        SourceRefreshCause::ManifestAudit {
            committed_revision: first_revision,
        },
        Some(11),
    );
    workflow.finish_scan(&mut browser, first_result);

    let second = workflow
        .begin_add_source_path_preserving_selection(
            &mut browser,
            second_root.path().to_path_buf(),
            213,
        )
        .expect("second source scan");
    let second_id = second.source_id.clone();
    workflow.start_scan(&second);
    let second_result = scan_source_with_progress(second, |_| {}, |_| {});
    let second_revision = second_result
        .metadata_hydration
        .revision()
        .expect("second revision");
    workflow.queue_required_refresh_with_context(
        second_id,
        SourceRefreshCause::ProjectionRevisionGap {
            committed_revision: second_revision,
        },
        Some(12),
    );
    workflow.finish_scan(&mut browser, second_result);

    assert_eq!(
        workflow.next_pending_refresh_if_idle(),
        None,
        "covered startup audit and projection handoffs must reach terminal idle"
    );
}

#[test]
fn cancelled_scan_releases_ownership_and_requeues_the_source() {
    let root = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let request = workflow
        .begin_add_source_path(&mut browser, root.path().to_path_buf(), 22)
        .expect("scan request");
    let source_id = request.source_id.clone();
    workflow.start_scan(&request);
    let mut result = scan_source_with_progress(request, |_| {}, |_| {});
    result.cancelled = true;

    assert!(matches!(
        workflow.finish_scan(&mut browser, result),
        SourceScanFinish::Cancelled { .. }
    ));
    assert!(!workflow.active());
    assert_eq!(workflow.retry_counts.get(&source_id), Some(&1));
    assert_eq!(workflow.next_pending_refresh_if_idle(), Some(source_id));
}

#[test]
fn repeated_scan_retries_increment_without_creating_parallel_owners() {
    let root = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let first = workflow
        .begin_add_source_path(&mut browser, root.path().to_path_buf(), 223)
        .expect("first scan request");
    let source_id = first.source_id.clone();
    workflow.start_scan(&first);
    let mut first_result = scan_source_with_progress(first, |_| {}, |_| {});
    first_result.cancelled = true;
    workflow.finish_scan(&mut browser, first_result);
    assert_eq!(
        workflow.next_pending_refresh_if_idle(),
        Some(source_id.clone())
    );

    let second = match workflow.begin_filesystem_refresh_with_context(
        &mut browser,
        source_id.clone(),
        224,
        SourceRefreshCause::ScanCancelled,
        Some(4),
    ) {
        SourceRefreshRequest::Queued(request) => request,
        _ => panic!("retry must queue exactly one replacement scan"),
    };
    workflow.start_scan(&second);
    assert_eq!(workflow.progress().expect("retry progress").retry_count, 1);
    let mut second_result = scan_source_with_progress(second, |_| {}, |_| {});
    second_result.cancelled = true;
    workflow.finish_scan(&mut browser, second_result);

    assert!(!workflow.active());
    assert_eq!(workflow.retry_counts.get(&source_id), Some(&2));
    assert_eq!(workflow.next_pending_refresh_if_idle(), Some(source_id));
}

#[test]
fn cancelled_scan_from_retired_generation_releases_ownership_without_requeueing() {
    let root = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let request = workflow
        .begin_add_source_path(&mut browser, root.path().to_path_buf(), 221)
        .expect("scan request");
    workflow.start_scan(&request);
    let mut result = scan_source_with_progress(request, |_| {}, |_| {});
    result.cancelled = true;
    workflow.queue_required_refresh_with_context(
        result.source_id.clone(),
        SourceRefreshCause::ManifestAudit {
            committed_revision: 12,
        },
        Some(8),
    );

    assert!(matches!(
        workflow.finish_scan_with_lifecycle(&mut browser, result, Some(7), false),
        SourceScanFinish::Stale { .. }
    ));
    assert!(!workflow.active());
    let pending = workflow
        .next_pending_refresh_context_if_idle()
        .expect("replacement generation refresh remains queued");
    assert_eq!(pending.lifecycle_generation, Some(8));
    assert_eq!(
        pending.cause,
        SourceRefreshCause::ManifestAudit {
            committed_revision: 12
        }
    );
}

#[test]
fn user_cancel_releases_visible_owner_once_and_drops_late_completion() {
    let root = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let request = workflow
        .begin_add_source_path(&mut browser, root.path().to_path_buf(), 222)
        .expect("scan request");
    let source_id = request.source_id.clone();
    let result = scan_source_with_progress(request.clone(), |_| {}, |_| {});
    workflow.start_scan(&request);
    workflow.queue_required_refresh_with_context(
        source_id.clone(),
        SourceRefreshCause::WatcherOverflow,
        Some(3),
    );

    assert_eq!(
        workflow.cancel_active_scan_by_user(&mut browser),
        Some((source_id, request.label))
    );
    assert!(!workflow.active());
    assert!(workflow.next_pending_refresh_context_if_idle().is_none());
    assert_eq!(workflow.cancel_active_scan_by_user(&mut browser), None);
    assert!(matches!(
        workflow.finish_scan(&mut browser, result),
        SourceScanFinish::Stale { .. }
    ));
    assert!(!workflow.active());
}

#[test]
fn worker_failure_retires_owner_without_scheduling_an_automatic_retry() {
    let root = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let request = workflow
        .begin_add_source_path(&mut browser, root.path().to_path_buf(), 72)
        .expect("scan request");
    let source_id = request.source_id.clone();
    workflow.start_scan(&request);
    assert!(workflow.transition_current_scan(
        request.task_id,
        &source_id,
        Some(8),
        FolderScanLifecycle::Scanning,
        "Scanning"
    ));

    let failed = workflow
        .fail_active_scan(&mut browser, request.task_id, &source_id, Some(8))
        .expect("failed terminal progress");

    assert_eq!(failed.lifecycle, FolderScanLifecycle::Failed);
    assert!(!workflow.active());
    assert_eq!(workflow.next_pending_refresh_if_idle(), None);
    assert!(!browser.scan_is_active(&source_id, request.task_id));
}

#[test]
fn user_cancel_retires_post_projection_persistence_owner_and_fences_late_completion() {
    let root = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let request = workflow
        .begin_add_source_path(&mut browser, root.path().to_path_buf(), 73)
        .expect("scan request");
    let source_id = request.source_id.clone();
    workflow.start_scan(&request);
    assert!(workflow.transition_current_scan(
        request.task_id,
        &source_id,
        Some(9),
        FolderScanLifecycle::ApplyingResults,
        "Applying"
    ));
    let applying = workflow.progress().cloned().expect("applying progress");
    let result = scan_source_with_progress(request.clone(), |_| {}, |_| {});
    assert!(matches!(
        workflow.finish_scan_with_lifecycle(&mut browser, result, Some(9), true),
        SourceScanFinish::Applied { .. }
    ));
    assert!(workflow.resume_progress_after_projection(applying));
    assert!(workflow.transition_current_scan(
        request.task_id,
        &source_id,
        Some(9),
        FolderScanLifecycle::PersistingResults,
        "Saving"
    ));

    assert_eq!(
        workflow.cancel_active_scan_by_user(&mut browser),
        Some((source_id.clone(), request.label.clone()))
    );
    assert!(!workflow.active());
    assert!(
        workflow
            .finish_current_scan_terminal(
                request.task_id,
                &source_id,
                Some(9),
                FolderScanLifecycle::Complete,
            )
            .is_none(),
        "late maintenance completion must not revive a retired owner"
    );
}

#[test]
fn refresh_coalescing_does_not_transfer_cause_across_lifecycle_generations() {
    let mut workflow = SourceScanWorkflow::new();
    let source_id = String::from("source");
    workflow.queue_required_refresh_with_context(
        source_id.clone(),
        SourceRefreshCause::WatcherOverflow,
        Some(1),
    );
    workflow.queue_required_refresh_with_context(
        source_id,
        SourceRefreshCause::ManifestAudit {
            committed_revision: 9,
        },
        Some(2),
    );

    let pending = workflow
        .next_pending_refresh_context_if_idle()
        .expect("current-generation refresh");
    assert_eq!(pending.lifecycle_generation, Some(2));
    assert_eq!(
        pending.cause,
        SourceRefreshCause::ManifestAudit {
            committed_revision: 9
        }
    );
}

#[test]
fn removed_active_source_releases_global_scan_lane_and_purges_refreshes() {
    let root = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let request = workflow
        .begin_add_source_path(&mut browser, root.path().to_path_buf(), 23)
        .expect("scan request");
    let source_id = request.source_id.clone();
    workflow.start_scan(&request);
    workflow.queue_required_refresh(source_id.clone());

    assert!(workflow.retire_source(&source_id));
    assert!(!workflow.active());
    assert!(!workflow.pending_refresh_contains_for_tests(&source_id));

    browser.remove_source(&source_id).expect("remove source");
    let result = scan_source_with_progress(request, |_| {}, |_| {});
    assert!(matches!(
        workflow.finish_scan(&mut browser, result),
        SourceScanFinish::Stale { .. }
    ));
    assert!(!workflow.active());
    assert_eq!(workflow.next_pending_refresh_if_idle(), None);
}

#[test]
fn cached_source_selection_does_not_queue_refresh_while_another_scan_is_active() {
    let first_root = temp_dir_with_wav();
    let second_root = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();

    let first = workflow
        .begin_add_source_path(&mut browser, first_root.path().to_path_buf(), 31)
        .expect("first source scan");
    let first_id = first.source_id.clone();
    workflow.start_scan(&first);
    let first_result = scan_source_with_progress(first, |_| {}, |_| {});
    assert!(matches!(
        workflow.finish_scan(&mut browser, first_result),
        SourceScanFinish::Applied { .. }
    ));

    let second = workflow
        .begin_add_source_path_preserving_selection(
            &mut browser,
            second_root.path().to_path_buf(),
            32,
        )
        .expect("second source scan");
    let second_id = second.source_id.clone();
    workflow.start_scan(&second);
    let second_result = scan_source_with_progress(second, |_| {}, |_| {});
    assert!(matches!(
        workflow.finish_scan(&mut browser, second_result),
        SourceScanFinish::Applied { .. }
    ));

    let active = workflow
        .begin_source_scan(&mut browser, first_id.clone(), 33)
        .expect("active source rescan");
    workflow.start_scan(&active);

    assert!(matches!(
        workflow.begin_select_source(&mut browser, second_id.clone(), 34),
        SourceSelectionRequest::Settled
    ));
    assert_eq!(browser.selected_source_id(), second_id);
    assert!(browser.selected_source_loaded());
    assert!(!workflow.pending_refresh_contains_for_tests(&second_id));
    let second_visible = browser
        .selected_audio_files()
        .into_iter()
        .map(|file| file.id.clone())
        .collect::<Vec<_>>();

    assert!(matches!(
        workflow.begin_select_source(&mut browser, second_id.clone(), 35),
        SourceSelectionRequest::Settled
    ));
    assert_eq!(
        browser
            .selected_audio_files()
            .into_iter()
            .map(|file| file.id.clone())
            .collect::<Vec<_>>(),
        second_visible,
        "reselecting the visible source must preserve its live tree"
    );
    assert!(matches!(
        workflow.begin_select_source(&mut browser, first_id.clone(), 36),
        SourceSelectionRequest::Settled
    ));
    assert_eq!(browser.selected_source_id(), first_id);
    assert!(browser.selected_source_loaded());
    assert!(!workflow.pending_refresh_contains_for_tests(&first_id));

    let active_result = scan_source_with_progress(active, |_| {}, |_| {});
    assert!(matches!(
        workflow.finish_scan(&mut browser, active_result),
        SourceScanFinish::Applied { .. }
    ));
    assert_eq!(workflow.next_pending_refresh_if_idle(), None);
}

#[test]
fn cached_source_selection_does_not_rescan_when_idle() {
    let root = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let request = workflow
        .begin_add_source_path(&mut browser, root.path().to_path_buf(), 37)
        .expect("initial source scan");
    let source_id = request.source_id.clone();
    workflow.start_scan(&request);
    let result = scan_source_with_progress(request, |_| {}, |_| {});
    workflow.finish_scan(&mut browser, result);

    assert!(matches!(
        workflow.begin_select_source(&mut browser, source_id.clone(), 38),
        SourceSelectionRequest::Settled
    ));
    assert_eq!(browser.selected_source_id(), source_id);
    assert!(browser.selected_source_loaded());
    assert!(!workflow.active());
    assert_eq!(workflow.next_pending_refresh_if_idle(), None);
}

#[test]
fn active_source_reselection_preserves_visible_tree() {
    let root = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let initial = workflow
        .begin_add_source_path(&mut browser, root.path().to_path_buf(), 40)
        .expect("initial scan");
    let source_id = initial.source_id.clone();
    workflow.start_scan(&initial);
    let result = scan_source_with_progress(initial, |_| {}, |_| {});
    assert!(matches!(
        workflow.finish_scan(&mut browser, result),
        SourceScanFinish::Applied { .. }
    ));
    let rescan = workflow
        .begin_source_scan(&mut browser, source_id.clone(), 41)
        .expect("rescan");
    workflow.start_scan(&rescan);
    let visible_before = browser
        .selected_audio_files()
        .into_iter()
        .map(|file| file.id.clone())
        .collect::<Vec<_>>();

    assert!(matches!(
        workflow.begin_select_source(&mut browser, source_id, 42),
        SourceSelectionRequest::Settled
    ));
    assert_eq!(
        browser
            .selected_audio_files()
            .into_iter()
            .map(|file| file.id.clone())
            .collect::<Vec<_>>(),
        visible_before
    );
}

#[test]
fn selecting_background_rescan_owner_keeps_completed_scan_tree_loaded() {
    let first = temp_dir_with_wav();
    let second = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();

    let first_request = workflow
        .begin_add_source_path(&mut browser, first.path().to_path_buf(), 43)
        .expect("first source scan");
    workflow.start_scan(&first_request);
    let first_result = scan_source_with_progress(first_request, |_| {}, |_| {});
    workflow.finish_scan(&mut browser, first_result);

    let second_request = workflow
        .begin_add_source_path_preserving_selection(&mut browser, second.path().to_path_buf(), 44)
        .expect("background source scan");
    let second_id = second_request.source_id.clone();
    workflow.start_scan(&second_request);
    let second_result = scan_source_with_progress(second_request, |_| {}, |_| {});
    workflow.finish_scan(&mut browser, second_result);

    let rescan = workflow
        .begin_source_scan(&mut browser, second_id.clone(), 45)
        .expect("background source rescan");
    workflow.start_scan(&rescan);

    assert!(matches!(
        workflow.begin_select_source(&mut browser, second_id.clone(), 46),
        SourceSelectionRequest::Settled
    ));
    assert_eq!(browser.selected_source_id(), second_id);
    assert!(browser.selected_source_loaded());
    assert!(
        !browser.selected_audio_files().is_empty(),
        "selecting the scan owner should keep its completed tree visible and loaded"
    );
}

#[test]
fn missing_selected_source_reselection_replaces_stale_tree_during_another_scan() {
    let selected_root = temp_dir_with_wav();
    let scanning_root = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let selected = workflow
        .begin_add_source_path(&mut browser, selected_root.path().to_path_buf(), 43)
        .expect("selected source scan");
    let selected_id = selected.source_id.clone();
    workflow.start_scan(&selected);
    let selected_result = scan_source_with_progress(selected, |_| {}, |_| {});
    assert!(matches!(
        workflow.finish_scan(&mut browser, selected_result),
        SourceScanFinish::Applied { .. }
    ));
    assert!(browser.selected_source_loaded());
    assert!(!browser.selected_audio_files().is_empty());

    let scanning = workflow
        .begin_add_source_path_preserving_selection(
            &mut browser,
            scanning_root.path().to_path_buf(),
            44,
        )
        .expect("background source scan");
    workflow.start_scan(&scanning);
    drop(selected_root);

    assert!(matches!(
        workflow.begin_select_source(&mut browser, selected_id.clone(), 45),
        SourceSelectionRequest::Settled
    ));
    assert_eq!(browser.selected_source_id(), selected_id);
    assert!(browser.source_is_missing(&selected_id));
    assert!(!browser.selected_source_loaded());
    assert!(browser.selected_audio_files().is_empty());
    assert_eq!(workflow.progress().unwrap().source_id, scanning.source_id);
}

#[test]
fn uncached_selection_reports_deferred_while_another_scan_is_active() {
    let first = temp_dir_with_wav();
    let second = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let active = workflow
        .begin_add_source_path(&mut browser, first.path().to_path_buf(), 50)
        .expect("active scan");
    workflow.start_scan(&active);
    let pending = browser
        .begin_add_source_path_preserving_selection(second.path().to_path_buf(), 51)
        .expect("pending source");

    assert!(matches!(
        workflow.begin_select_source(&mut browser, pending.source_id.clone(), 52),
        SourceSelectionRequest::Deferred
    ));
    assert!(workflow.pending_refresh_contains_for_tests(&pending.source_id));
}

#[test]
fn missing_selection_is_not_queued_for_permanent_refresh() {
    let first = temp_dir_with_wav();
    let second = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let active = workflow
        .begin_add_source_path(&mut browser, first.path().to_path_buf(), 60)
        .expect("active scan");
    workflow.start_scan(&active);
    let pending = browser
        .begin_add_source_path_preserving_selection(second.path().to_path_buf(), 61)
        .expect("pending source");
    let source_id = pending.source_id.clone();
    drop(second);

    assert!(matches!(
        workflow.begin_select_source(&mut browser, source_id.clone(), 62),
        SourceSelectionRequest::Settled
    ));
    assert!(!workflow.pending_refresh_contains_for_tests(&source_id));
}

#[test]
fn duplicate_add_is_blocked_while_its_scan_is_active() {
    let root = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let active = workflow
        .begin_add_source_path(&mut browser, root.path().to_path_buf(), 70)
        .expect("active scan");
    workflow.start_scan(&active);

    assert!(
        workflow
            .begin_add_source_path(&mut browser, root.path().to_path_buf(), 71)
            .is_none()
    );
    assert_eq!(workflow.progress().unwrap().task_id, active.task_id);
    assert!(browser.scan_is_active(&active.source_id, active.task_id));
}

#[test]
fn latest_deferred_selection_is_refreshed_first() {
    let first = temp_dir_with_wav();
    let second = temp_dir_with_wav();
    let third = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let active = workflow
        .begin_add_source_path(&mut browser, first.path().to_path_buf(), 80)
        .expect("active scan");
    workflow.start_scan(&active);
    let second_request = browser
        .begin_add_source_path_preserving_selection(second.path().to_path_buf(), 81)
        .expect("second source");
    let third_request = browser
        .begin_add_source_path_preserving_selection(third.path().to_path_buf(), 82)
        .expect("third source");

    assert!(matches!(
        workflow.begin_select_source(&mut browser, second_request.source_id.clone(), 83),
        SourceSelectionRequest::Deferred
    ));
    assert!(matches!(
        workflow.begin_select_source(&mut browser, third_request.source_id.clone(), 84),
        SourceSelectionRequest::Deferred
    ));
    let result = scan_source_with_progress(active, |_| {}, |_| {});
    assert!(matches!(
        workflow.finish_scan(&mut browser, result),
        SourceScanFinish::Applied { .. }
    ));

    assert_eq!(
        workflow.next_pending_refresh_if_idle(),
        Some(third_request.source_id)
    );
    assert_eq!(
        workflow.next_pending_refresh_if_idle(),
        Some(second_request.source_id)
    );
}

#[test]
fn deferred_selection_is_refreshed_before_newer_watcher_work() {
    let mut workflow = SourceScanWorkflow::new();
    let selected_source = String::from("selected-source");
    let watcher_source = String::from("watcher-source");

    workflow.queue_selected_required_refresh(selected_source.clone());
    workflow.queue_required_refresh(watcher_source.clone());

    assert_eq!(
        workflow.next_pending_refresh_if_idle(),
        Some(selected_source)
    );
    assert_eq!(
        workflow.next_pending_refresh_if_idle(),
        Some(watcher_source)
    );
}

#[test]
fn deferred_selection_missing_before_execution_is_dropped() {
    let first = temp_dir_with_wav();
    let second = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let active = workflow
        .begin_add_source_path(&mut browser, first.path().to_path_buf(), 90)
        .expect("active scan");
    workflow.start_scan(&active);
    let pending = browser
        .begin_add_source_path_preserving_selection(second.path().to_path_buf(), 91)
        .expect("pending source");
    let source_id = pending.source_id.clone();
    assert!(matches!(
        workflow.begin_select_source(&mut browser, source_id.clone(), 92),
        SourceSelectionRequest::Deferred
    ));
    let result = scan_source_with_progress(active, |_| {}, |_| {});
    assert!(matches!(
        workflow.finish_scan(&mut browser, result),
        SourceScanFinish::Applied { .. }
    ));
    assert_eq!(
        workflow.next_pending_refresh_if_idle(),
        Some(source_id.clone())
    );
    drop(second);

    assert!(matches!(
        workflow.begin_filesystem_refresh(&mut browser, source_id.clone(), 93),
        SourceRefreshRequest::IgnoredMissing { .. }
    ));
    assert!(!workflow.pending_refresh_contains_for_tests(&source_id));
    assert_eq!(workflow.next_pending_refresh_if_idle(), None);
}

#[test]
fn new_source_add_is_registered_and_deferred_during_active_scan() {
    let first = temp_dir_with_wav();
    let second = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let active = workflow
        .begin_add_source_path(&mut browser, first.path().to_path_buf(), 100)
        .expect("active scan");
    workflow.start_scan(&active);

    assert!(
        workflow
            .begin_add_source_path(&mut browser, second.path().to_path_buf(), 101)
            .is_none()
    );
    let deferred_id = browser
        .source_id_for_root_path(second.path())
        .expect("deferred source registered");
    assert!(workflow.pending_refresh_contains_for_tests(&deferred_id));
    let watcher_source = String::from("newer-watcher-source");
    workflow.queue_required_refresh(watcher_source.clone());

    let result = scan_source_with_progress(active, |_| {}, |_| {});
    assert!(matches!(
        workflow.finish_scan(&mut browser, result),
        SourceScanFinish::Applied { .. }
    ));
    assert_eq!(
        workflow.next_pending_refresh_if_idle(),
        Some(deferred_id.clone())
    );
    assert_eq!(
        workflow.next_pending_refresh_if_idle(),
        Some(watcher_source)
    );
    assert!(matches!(
        workflow.begin_filesystem_refresh(&mut browser, deferred_id, 102),
        SourceRefreshRequest::Queued(_)
    ));
}

#[test]
fn removed_pending_source_is_dropped_instead_of_requeued() {
    let first = temp_dir_with_wav();
    let second = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let active = workflow
        .begin_add_source_path(&mut browser, first.path().to_path_buf(), 110)
        .expect("active scan");
    workflow.start_scan(&active);
    workflow.begin_add_source_path_preserving_selection(
        &mut browser,
        second.path().to_path_buf(),
        111,
    );
    let source_id = browser
        .source_id_for_root_path(second.path())
        .expect("deferred source");
    browser
        .remove_source(&source_id)
        .expect("remove deferred source");

    let result = scan_source_with_progress(active, |_| {}, |_| {});
    workflow.finish_scan(&mut browser, result);
    assert_eq!(
        workflow.next_pending_refresh_if_idle(),
        Some(source_id.clone())
    );
    assert!(matches!(
        workflow.begin_filesystem_refresh(&mut browser, source_id.clone(), 112),
        SourceRefreshRequest::IgnoredMissing { .. }
    ));
    assert!(!workflow.pending_refresh_contains_for_tests(&source_id));
}

#[test]
fn active_source_click_preserves_required_filesystem_refresh() {
    let root = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let active = workflow
        .begin_add_source_path(&mut browser, root.path().to_path_buf(), 120)
        .expect("active scan");
    let source_id = active.source_id.clone();
    workflow.start_scan(&active);
    assert!(matches!(
        workflow.plan_filesystem_change(&mut browser, source_id.clone(), &[], true, true),
        SourceFilesystemChangePlan::DeferredAlreadyRunning { .. }
    ));

    assert!(matches!(
        workflow.begin_select_source(&mut browser, source_id.clone(), 121),
        SourceSelectionRequest::Settled
    ));
    assert!(workflow.pending_refresh_contains_for_tests(&source_id));
}

#[test]
fn filesystem_change_uses_watcher_root_observation_without_probing_disk() {
    let root = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let request = workflow
        .begin_add_source_path(&mut browser, root.path().to_path_buf(), 124)
        .expect("source scan");
    let source_id = request.source_id.clone();

    let plan = workflow.plan_filesystem_change(&mut browser, source_id.clone(), &[], true, false);

    assert!(matches!(
        plan,
        SourceFilesystemChangePlan::IgnoredSourceMissing { .. }
    ));
    assert!(browser.source_is_missing(&source_id));
    assert!(
        root.path().is_dir(),
        "the test root remains present on disk"
    );
}

#[test]
fn targeted_watcher_hint_does_not_patch_browser_before_commit() {
    let root = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let request = workflow
        .begin_add_source_path(&mut browser, root.path().to_path_buf(), 125)
        .expect("source scan");
    let source_id = request.source_id.clone();
    workflow.start_scan(&request);
    let result = scan_source_with_progress(request, |_| {}, |_| {});
    workflow.finish_scan(&mut browser, result);
    fs::remove_file(root.path().join("sample.wav")).expect("remove sample");

    assert!(matches!(
        workflow.plan_filesystem_change_for_generation(
            &mut browser,
            source_id,
            &[PathBuf::from("sample.wav")],
            false,
            true,
            Some(7),
            Some(9),
            Some(replay_proof(9)),
        ),
        SourceFilesystemChangePlan::SyncPaths {
            changed_count: 1,
            ..
        }
    ));
    assert_eq!(
        browser.selected_audio_files().len(),
        1,
        "the watcher path is only a hint until the source database commit completes"
    );
}

#[test]
fn typed_scope_plan_preserves_exact_and_subtree_kinds_and_ignores_legacy_paths() {
    let root = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let request = workflow
        .begin_add_source_path(&mut browser, root.path().to_path_buf(), 126)
        .expect("source scan");
    let source_id = request.source_id.clone();
    workflow.start_scan(&request);
    workflow.finish_scan(
        &mut browser,
        scan_source_with_progress(request, |_| {}, |_| {}),
    );

    let plan = workflow.plan_filesystem_change_with_scopes_for_generation(
        &mut browser,
        source_id,
        Some(&typed_scopes()),
        &[PathBuf::from("legacy-fallback.wav")],
        false,
        true,
        Some(String::from("root-a")),
        Some(7),
        Some(9),
        Some(replay_proof(9)),
    );

    let SourceFilesystemChangePlan::SyncScopes {
        scopes,
        changed_count,
        source_root_identity,
        lifecycle_generation,
        ..
    } = plan
    else {
        panic!("typed replay should remain in the typed sync plan");
    };
    assert_eq!(changed_count, 2);
    assert_eq!(scopes[0].kind(), ReconciliationScopeKind::ExactEntry);
    assert_eq!(scopes[1].kind(), ReconciliationScopeKind::Subtree);
    assert_eq!(source_root_identity.as_deref(), Some("root-a"));
    assert_eq!(lifecycle_generation, Some(7));
}

#[test]
fn typed_source_audit_and_scope_loss_route_before_targeted_dispatch() {
    for (scopes, reason) in [
        (source_audit_scope(), "reconciliation_source_audit_scope"),
        (Vec::new(), "reconciliation_scope_lost"),
    ] {
        let root = temp_dir_with_wav();
        let mut browser = FolderBrowserState::load_default();
        let mut workflow = SourceScanWorkflow::new();
        let request = workflow
            .begin_add_source_path(&mut browser, root.path().to_path_buf(), 127)
            .expect("source scan");
        let source_id = request.source_id.clone();
        workflow.start_scan(&request);
        workflow.finish_scan(
            &mut browser,
            scan_source_with_progress(request, |_| {}, |_| {}),
        );

        assert!(matches!(
            workflow.plan_filesystem_change_with_scopes_for_generation(
                &mut browser,
                source_id,
                Some(&scopes),
                &[PathBuf::from("must-not-be-used.wav")],
                false,
                true,
                Some(String::from("root-a")),
                Some(7),
                Some(9),
                Some(replay_proof(9)),
            ),
            SourceFilesystemChangePlan::SourceAuditRequired { reason: actual, .. }
                if actual == reason
        ));
    }
}

#[test]
fn unproven_idle_watcher_evidence_queues_authoritative_refresh() {
    let mut malformed = replay_proof(60);
    malformed.backend_device = 0;
    let cases: Vec<(&str, Option<u64>, Option<WatcherContinuityProof>)> = vec![
        ("proofless", None, None),
        ("cursor-only", Some(60), None),
        ("proof-only", None, Some(replay_proof(60))),
        ("malformed", Some(60), Some(malformed)),
        ("mismatched", Some(61), Some(replay_proof(60))),
    ];

    for (label, event_id, proof) in cases {
        let root = temp_dir_with_wav();
        let mut browser = FolderBrowserState::load_default();
        let mut workflow = SourceScanWorkflow::new();
        let request = workflow
            .begin_add_source_path(&mut browser, root.path().to_path_buf(), 133)
            .expect("source scan");
        let source_id = request.source_id.clone();
        workflow.start_scan(&request);
        let result = scan_source_with_progress(request, |_| {}, |_| {});
        workflow.finish_scan(&mut browser, result);

        assert!(
            matches!(
                workflow.plan_filesystem_change_for_generation(
                    &mut browser,
                    source_id.clone(),
                    &[PathBuf::from("sample.wav")],
                    false,
                    true,
                    Some(7),
                    event_id,
                    proof,
                ),
                SourceFilesystemChangePlan::QueueRefresh {
                    cause: SourceRefreshCause::WatcherAuthorityUnproven,
                    ..
                }
            ),
            "{label} watcher evidence must not target-sync"
        );
    }
}

#[test]
fn proofless_watcher_paths_during_full_scan_require_audit() {
    let root = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let request = workflow
        .begin_add_source_path(&mut browser, root.path().to_path_buf(), 134)
        .expect("active scan");
    let source_id = request.source_id.clone();
    workflow.start_scan(&request);

    assert!(matches!(
        workflow.plan_filesystem_change_for_generation(
            &mut browser,
            source_id.clone(),
            &[PathBuf::from("proofless.wav")],
            false,
            true,
            Some(12),
            None,
            None,
        ),
        SourceFilesystemChangePlan::DeferredAlreadyRunning { .. }
    ));

    let result = scan_source_with_progress(request, |_| {}, |_| {});
    workflow.finish_scan(&mut browser, result);
    let pending = workflow
        .next_pending_targeted_sync()
        .expect("deferred audit");
    assert!(pending.audit_required);
    assert!(pending.proofless_evidence_seen);
    assert_eq!(pending.journal_checkpoint_event_id, None);
    assert_eq!(pending.watcher_continuity_proof, None);
}

#[test]
fn proofless_watcher_paths_during_targeted_sync_require_audit() {
    let root = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let request = workflow
        .begin_add_source_path(&mut browser, root.path().to_path_buf(), 135)
        .expect("active scan");
    let source_id = request.source_id.clone();
    workflow.start_scan(&request);
    let result = scan_source_with_progress(request, |_| {}, |_| {});
    workflow.finish_scan(&mut browser, result);
    assert!(workflow.mark_targeted_sync_started(&source_id, 14));

    assert!(matches!(
        workflow.plan_filesystem_change_for_generation(
            &mut browser,
            source_id.clone(),
            &[PathBuf::from("proofless.wav")],
            false,
            true,
            Some(14),
            None,
            None,
        ),
        SourceFilesystemChangePlan::DeferredAlreadyRunning { .. }
    ));

    workflow.mark_targeted_sync_finished(&source_id, 14);
    let pending = workflow
        .next_pending_targeted_sync()
        .expect("deferred audit");
    assert!(pending.audit_required);
    assert!(pending.proofless_evidence_seen);
    assert_eq!(pending.journal_checkpoint_event_id, None);
    assert_eq!(pending.watcher_continuity_proof, None);
}

#[test]
fn watcher_paths_arriving_during_full_scan_coalesce_for_one_followup_sync() {
    let root = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let request = workflow
        .begin_add_source_path(&mut browser, root.path().to_path_buf(), 126)
        .expect("source scan");
    let source_id = request.source_id.clone();
    workflow.start_scan(&request);

    let proof = replay_proof(11);
    for paths in [
        vec![PathBuf::from("sample.wav")],
        vec![
            PathBuf::from("sample.wav"),
            PathBuf::from("nested/extra.wav"),
        ],
    ]
    .into_iter()
    {
        assert!(matches!(
            workflow.plan_filesystem_change_for_generation(
                &mut browser,
                source_id.clone(),
                &paths,
                false,
                true,
                Some(7),
                Some(11),
                Some(proof.clone()),
            ),
            SourceFilesystemChangePlan::DeferredAlreadyRunning { .. }
        ));
    }
    assert!(workflow.next_pending_targeted_sync().is_none());

    let result = scan_source_with_progress(request, |_| {}, |_| {});
    assert!(matches!(
        workflow.finish_scan(&mut browser, result),
        SourceScanFinish::Applied { .. }
    ));
    let pending = workflow
        .next_pending_targeted_sync()
        .expect("one targeted followup");
    assert_eq!(pending.source_id, source_id);
    assert_eq!(pending.lifecycle_generation, Some(7));
    assert_eq!(pending.journal_checkpoint_event_id, Some(11));
    assert_eq!(pending.watcher_continuity_proof, Some(proof));
    assert!(!pending.proofless_evidence_seen);
    assert!(!pending.audit_required);
    assert_eq!(
        pending.paths,
        vec![
            PathBuf::from("nested/extra.wav"),
            PathBuf::from("sample.wav")
        ]
    );
    assert!(workflow.next_pending_targeted_sync().is_none());
}

#[test]
fn proof_then_proofless_watcher_paths_during_full_scan_require_audit() {
    let root = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let request = workflow
        .begin_add_source_path(&mut browser, root.path().to_path_buf(), 129)
        .expect("active scan");
    let source_id = request.source_id.clone();
    workflow.start_scan(&request);

    for (path, journal_checkpoint_event_id, watcher_continuity_proof) in [
        ("proof.wav", Some(31), Some(replay_proof(31))),
        ("proofless.wav", None, None),
    ] {
        assert!(matches!(
            workflow.plan_filesystem_change_for_generation(
                &mut browser,
                source_id.clone(),
                &[PathBuf::from(path)],
                false,
                true,
                Some(12),
                journal_checkpoint_event_id,
                watcher_continuity_proof,
            ),
            SourceFilesystemChangePlan::DeferredAlreadyRunning { .. }
        ));
    }

    let result = scan_source_with_progress(request, |_| {}, |_| {});
    workflow.finish_scan(&mut browser, result);
    let pending = workflow
        .next_pending_targeted_sync()
        .expect("deferred targeted sync");
    assert!(pending.audit_required);
    assert!(pending.proofless_evidence_seen);
    assert_eq!(pending.journal_checkpoint_event_id, None);
    assert_eq!(pending.watcher_continuity_proof, None);
}

#[test]
fn proofless_then_proof_watcher_paths_during_full_scan_require_audit() {
    let root = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let request = workflow
        .begin_add_source_path(&mut browser, root.path().to_path_buf(), 130)
        .expect("active scan");
    let source_id = request.source_id.clone();
    workflow.start_scan(&request);

    for (path, journal_checkpoint_event_id, watcher_continuity_proof) in [
        ("proofless.wav", None, None),
        ("proof.wav", Some(32), Some(replay_proof(32))),
    ] {
        assert!(matches!(
            workflow.plan_filesystem_change_for_generation(
                &mut browser,
                source_id.clone(),
                &[PathBuf::from(path)],
                false,
                true,
                Some(12),
                journal_checkpoint_event_id,
                watcher_continuity_proof,
            ),
            SourceFilesystemChangePlan::DeferredAlreadyRunning { .. }
        ));
    }

    let result = scan_source_with_progress(request, |_| {}, |_| {});
    workflow.finish_scan(&mut browser, result);
    let pending = workflow
        .next_pending_targeted_sync()
        .expect("deferred targeted sync");
    assert!(pending.audit_required);
    assert!(pending.proofless_evidence_seen);
    assert_eq!(pending.journal_checkpoint_event_id, None);
    assert_eq!(pending.watcher_continuity_proof, None);
}

#[test]
fn replay_watcher_paths_arriving_during_targeted_sync_retain_boundary() {
    let root = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let request = workflow
        .begin_add_source_path(&mut browser, root.path().to_path_buf(), 127)
        .expect("source scan");
    let source_id = request.source_id.clone();
    workflow.start_scan(&request);
    let result = scan_source_with_progress(request, |_| {}, |_| {});
    workflow.finish_scan(&mut browser, result);
    assert!(workflow.mark_targeted_sync_started(&source_id, 9));

    let proof = replay_proof(21);
    for path in ["sample.wav", "nested/extra.wav"] {
        assert!(matches!(
            workflow.plan_filesystem_change_for_generation(
                &mut browser,
                source_id.clone(),
                &[PathBuf::from(path)],
                false,
                true,
                Some(9),
                Some(21),
                Some(proof.clone()),
            ),
            SourceFilesystemChangePlan::DeferredAlreadyRunning { .. }
        ));
    }

    workflow.mark_targeted_sync_finished(&source_id, 9);
    let pending = workflow
        .next_pending_targeted_sync()
        .expect("deferred replay sync");
    assert_eq!(pending.journal_checkpoint_event_id, Some(21));
    assert_eq!(pending.watcher_continuity_proof, Some(proof));
    assert!(!pending.proofless_evidence_seen);
    assert!(!pending.audit_required);
}

#[test]
fn proof_then_proofless_watcher_paths_during_targeted_sync_require_audit() {
    let root = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let request = workflow
        .begin_add_source_path(&mut browser, root.path().to_path_buf(), 131)
        .expect("source scan");
    let source_id = request.source_id.clone();
    workflow.start_scan(&request);
    let result = scan_source_with_progress(request, |_| {}, |_| {});
    workflow.finish_scan(&mut browser, result);
    assert!(workflow.mark_targeted_sync_started(&source_id, 14));

    for (path, journal_checkpoint_event_id, watcher_continuity_proof) in [
        ("proof.wav", Some(41), Some(replay_proof(41))),
        ("proofless.wav", None, None),
    ] {
        assert!(matches!(
            workflow.plan_filesystem_change_for_generation(
                &mut browser,
                source_id.clone(),
                &[PathBuf::from(path)],
                false,
                true,
                Some(14),
                journal_checkpoint_event_id,
                watcher_continuity_proof,
            ),
            SourceFilesystemChangePlan::DeferredAlreadyRunning { .. }
        ));
    }

    workflow.mark_targeted_sync_finished(&source_id, 14);
    let pending = workflow
        .next_pending_targeted_sync()
        .expect("deferred replay sync");
    assert!(pending.audit_required);
    assert!(pending.proofless_evidence_seen);
    assert_eq!(pending.journal_checkpoint_event_id, None);
    assert_eq!(pending.watcher_continuity_proof, None);
}

#[test]
fn proofless_then_proof_watcher_paths_during_targeted_sync_require_audit() {
    let root = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let request = workflow
        .begin_add_source_path(&mut browser, root.path().to_path_buf(), 132)
        .expect("source scan");
    let source_id = request.source_id.clone();
    workflow.start_scan(&request);
    let result = scan_source_with_progress(request, |_| {}, |_| {});
    workflow.finish_scan(&mut browser, result);
    assert!(workflow.mark_targeted_sync_started(&source_id, 15));

    for (path, journal_checkpoint_event_id, watcher_continuity_proof) in [
        ("proofless.wav", None, None),
        ("proof.wav", Some(42), Some(replay_proof(42))),
    ] {
        assert!(matches!(
            workflow.plan_filesystem_change_for_generation(
                &mut browser,
                source_id.clone(),
                &[PathBuf::from(path)],
                false,
                true,
                Some(15),
                journal_checkpoint_event_id,
                watcher_continuity_proof,
            ),
            SourceFilesystemChangePlan::DeferredAlreadyRunning { .. }
        ));
    }

    workflow.mark_targeted_sync_finished(&source_id, 15);
    let pending = workflow
        .next_pending_targeted_sync()
        .expect("deferred replay sync");
    assert!(pending.audit_required);
    assert!(pending.proofless_evidence_seen);
    assert_eq!(pending.journal_checkpoint_event_id, None);
    assert_eq!(pending.watcher_continuity_proof, None);
}

#[test]
fn incompatible_replay_boundaries_arriving_during_targeted_sync_require_audit() {
    let root = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let request = workflow
        .begin_add_source_path(&mut browser, root.path().to_path_buf(), 128)
        .expect("source scan");
    let source_id = request.source_id.clone();
    workflow.start_scan(&request);
    let result = scan_source_with_progress(request, |_| {}, |_| {});
    workflow.finish_scan(&mut browser, result);
    assert!(workflow.mark_targeted_sync_started(&source_id, 10));

    for (event_id, path) in [(31, "first.wav"), (32, "second.wav")] {
        assert!(matches!(
            workflow.plan_filesystem_change_for_generation(
                &mut browser,
                source_id.clone(),
                &[PathBuf::from(path)],
                false,
                true,
                Some(10),
                Some(event_id),
                Some(replay_proof(event_id)),
            ),
            SourceFilesystemChangePlan::DeferredAlreadyRunning { .. }
        ));
    }

    workflow.mark_targeted_sync_finished(&source_id, 10);
    let pending = workflow
        .next_pending_targeted_sync()
        .expect("deferred replay sync");
    assert!(pending.audit_required);
    assert_eq!(pending.journal_checkpoint_event_id, None);
    assert_eq!(pending.watcher_continuity_proof, None);
}

#[test]
fn pending_full_scan_subsumes_watcher_paths_before_execution() {
    let root = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let request = workflow
        .begin_add_source_path(&mut browser, root.path().to_path_buf(), 127)
        .expect("source scan");
    let source_id = request.source_id.clone();
    workflow.start_scan(&request);
    let result = scan_source_with_progress(request, |_| {}, |_| {});
    workflow.finish_scan(&mut browser, result);
    workflow.queue_required_refresh_with_context(
        source_id.clone(),
        SourceRefreshCause::WatcherOverflow,
        Some(9),
    );

    assert!(matches!(
        workflow.plan_filesystem_change_for_generation(
            &mut browser,
            source_id.clone(),
            &[PathBuf::from("sample.wav")],
            false,
            true,
            Some(9),
            None,
            None,
        ),
        SourceFilesystemChangePlan::DeferredAlreadyRunning { .. }
    ));
    assert!(workflow.next_pending_targeted_sync().is_none());
    assert_eq!(workflow.next_pending_refresh_if_idle(), Some(source_id));
}

#[test]
fn full_fallback_waits_for_active_targeted_sync_and_supersedes_queued_paths() {
    let root = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let request = workflow
        .begin_add_source_path(&mut browser, root.path().to_path_buf(), 128)
        .expect("source scan");
    let source_id = request.source_id.clone();
    workflow.start_scan(&request);
    let result = scan_source_with_progress(request, |_| {}, |_| {});
    workflow.finish_scan(&mut browser, result);
    assert!(workflow.mark_targeted_sync_started(&source_id, 11));
    assert!(matches!(
        workflow.plan_filesystem_change_for_generation(
            &mut browser,
            source_id.clone(),
            &[PathBuf::from("later.wav")],
            false,
            true,
            Some(11),
            None,
            None,
        ),
        SourceFilesystemChangePlan::DeferredAlreadyRunning { .. }
    ));

    assert!(matches!(
        workflow.begin_filesystem_refresh_with_context(
            &mut browser,
            source_id.clone(),
            129,
            SourceRefreshCause::FilesystemSyncIncomplete,
            Some(11),
        ),
        SourceRefreshRequest::Deferred { .. }
    ));
    workflow.mark_targeted_sync_finished(&source_id, 11);

    assert!(workflow.next_pending_targeted_sync().is_none());
    assert_eq!(workflow.next_pending_refresh_if_idle(), Some(source_id));
}

#[test]
fn source_retirement_purges_targeted_sync_causal_state() {
    let root = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let request = workflow
        .begin_add_source_path(&mut browser, root.path().to_path_buf(), 130)
        .expect("source scan");
    let source_id = request.source_id.clone();
    workflow.start_scan(&request);
    let result = scan_source_with_progress(request, |_| {}, |_| {});
    workflow.finish_scan(&mut browser, result);
    assert!(workflow.mark_targeted_sync_started(&source_id, 13));
    assert!(matches!(
        workflow.plan_filesystem_change_for_generation(
            &mut browser,
            source_id.clone(),
            &[PathBuf::from("late.wav")],
            false,
            true,
            Some(13),
            None,
            None,
        ),
        SourceFilesystemChangePlan::DeferredAlreadyRunning { .. }
    ));

    workflow.retire_source(&source_id);
    workflow.mark_targeted_sync_finished(&source_id, 13);

    assert!(workflow.next_pending_targeted_sync().is_none());
    assert!(workflow.mark_targeted_sync_started(&source_id, 14));
}

#[test]
fn switching_away_parks_live_discoveries_from_an_active_scan() {
    let first = temp_dir_with_wav();
    let second = temp_dir_with_wav();
    let mut browser = FolderBrowserState::load_default();
    let mut workflow = SourceScanWorkflow::new();
    let first_request = workflow
        .begin_add_source_path(&mut browser, first.path().to_path_buf(), 130)
        .expect("first source");
    let first_id = first_request.source_id.clone();
    workflow.start_scan(&first_request);
    let first_result = scan_source_with_progress(first_request, |_| {}, |_| {});
    workflow.finish_scan(&mut browser, first_result);
    let second_request = workflow
        .begin_add_source_path_preserving_selection(&mut browser, second.path().to_path_buf(), 131)
        .expect("second source");
    let second_id = second_request.source_id.clone();
    workflow.start_scan(&second_request);
    let second_result = scan_source_with_progress(second_request, |_| {}, |_| {});
    workflow.finish_scan(&mut browser, second_result);

    let active = workflow
        .begin_source_scan(&mut browser, first_id.clone(), 132)
        .expect("first source rescan");
    workflow.start_scan(&active);
    let mut batches = Vec::new();
    let _result = scan_source_with_progress(active, |_| {}, |batch| batches.push(batch));
    for event in batches {
        workflow.apply_discovery_batch(
            &mut browser,
            FolderScanDiscoveryBatch {
                task_id: event.task_id,
                source_id: event.source_id.clone(),
                committed_revision: event.committed_revision,
                lifecycle_generation: None,
                sequence: 0,
                events: vec![event],
            },
        );
    }
    let visible_before = browser.selected_audio_files().len();
    assert!(visible_before > 0);

    workflow.begin_select_source(&mut browser, second_id, 133);
    workflow.begin_select_source(&mut browser, first_id, 134);

    assert_eq!(browser.selected_audio_files().len(), visible_before);
}
