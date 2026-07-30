#[test]
fn bounded_manifest_delta_preserves_unaffected_in_flight_generation() {
    let (_directory, source) = unhashed_source("bounded-delta-generation");
    let mut supervisor = SourceProcessingSupervisor::dormant();
    supervisor
        .replace_sources(vec![source.clone()])
        .expect("configure source");
    let retained_generation = {
        let mut control = supervisor.shared.control();
        control.dirty_sources.clear();
        Arc::clone(&control.source_work_cancels[source.id.as_str()])
    };
    supervisor.request_source_delta(
        source.id.as_str(),
        supervisor.lifecycle_generations()[source.id.as_str()],
        &CommittedSourceDelta {
            revision: 1,
            changed: vec![wavecrate::sample_sources::scanner::ManifestIdentityDelta {
                identity: String::from("changed"),
                relative_path: PathBuf::from("changed.wav"),
                content_generation: String::from("changed-generation"),
                source_metadata_changed: false,
            }],
            ..CommittedSourceDelta::default()
        },
        "test_bounded_delta",
    );

    supervisor.wake_source(source.id.as_str(), "filesystem_changed");

    assert!(!retained_generation.load(Ordering::Acquire));
    let control = supervisor.shared.control();
    assert!(control.dirty_sources.contains(source.id.as_str()));
    assert!(
        control
            .pending_readiness_deltas
            .contains_key(source.id.as_str())
    );
    assert!(Arc::ptr_eq(
        &retained_generation,
        &control.source_work_cancels[source.id.as_str()]
    ));
    drop(control);
    assert_eq!(supervisor.shutdown()["joined"], true);
}

#[test]
fn non_mutating_source_requests_preserve_in_flight_generation() {
    let (_directory, source) = unhashed_source("non-mutating-source-request");
    let mut supervisor = SourceProcessingSupervisor::dormant();
    supervisor
        .replace_sources(vec![source.clone()])
        .expect("configure source");
    let retained_generation = {
        let mut control = supervisor.shared.control();
        control.dirty_sources.clear();
        Arc::clone(&control.source_work_cancels[source.id.as_str()])
    };

    supervisor.request_source_processing(source.id.as_str(), "source_scan_finished");

    assert!(!retained_generation.load(Ordering::Acquire));
    let control = supervisor.shared.control();
    assert!(control.dirty_sources.contains(source.id.as_str()));
    assert!(Arc::ptr_eq(
        &retained_generation,
        &control.source_work_cancels[source.id.as_str()]
    ));
    drop(control);
    assert_eq!(supervisor.shutdown()["joined"], true);
}

#[test]
fn unchanged_foreground_scan_release_does_not_request_generic_discovery() {
    let (_directory, source) = unhashed_source("unchanged-foreground-release");
    let mut supervisor = SourceProcessingSupervisor::dormant();
    supervisor
        .replace_sources(vec![source.clone()])
        .expect("configure source");
    {
        let mut control = supervisor.shared.control();
        control.dirty_sources.clear();
        control
            .awaiting_foreground_refresh_sources
            .remove(source.id.as_str());
    }

    supervisor.finish_foreground_source_refresh(source.id.as_str(), "unchanged_foreground_scan");

    let control = supervisor.shared.control();
    assert!(
        !control.dirty_sources.contains(source.id.as_str()),
        "an unchanged foreground scan must remain a bounded no-op"
    );
    assert!(
        !control
            .pending_readiness_deltas
            .contains_key(source.id.as_str())
    );
    drop(control);
    assert_eq!(supervisor.shutdown()["joined"], true);
}

#[test]
fn explicit_reanalysis_cancels_current_work_without_implicit_priority() {
    let (_directory, source) = unhashed_source("explicit-reanalysis-request");
    let mut supervisor = SourceProcessingSupervisor::dormant();
    supervisor
        .replace_sources(vec![source.clone()])
        .expect("configure source");
    let retained_generation = {
        let mut control = supervisor.shared.control();
        control.dirty_sources.clear();
        Arc::clone(&control.source_work_cancels[source.id.as_str()])
    };
    let scan = supervisor
        .budget_handle()
        .acquire_scan(source.id.as_str())
        .expect("admit source scan");
    let scan_cancel = scan.cancel_token();

    supervisor.request_source_reanalysis(source.id.as_str(), "user_process_source");

    assert!(retained_generation.load(Ordering::Acquire));
    assert!(scan_cancel.load(Ordering::Acquire));
    let control = supervisor.shared.control();
    assert!(control.dirty_sources.contains(source.id.as_str()));
    assert!(
        control
            .force_reanalysis_sources
            .contains(source.id.as_str())
    );
    assert_eq!(control.priority.selected_source, None);
    assert!(
        !control.source_work_cancels[source.id.as_str()].load(Ordering::Acquire),
        "the replacement generation must be available for the reanalysis run"
    );
    drop(control);
    drop(scan);
    assert_eq!(supervisor.shutdown()["joined"], true);
}

#[test]
fn external_scan_release_invalidates_retained_source_generation() {
    let (_directory, source) = unhashed_source("external-commit-generation");
    let mut supervisor = SourceProcessingSupervisor::dormant();
    supervisor
        .replace_sources(vec![source.clone()])
        .expect("configure source");
    let retained_generation = {
        let mut control = supervisor.shared.control();
        control.dirty_sources.clear();
        Arc::clone(&control.source_work_cancels[source.id.as_str()])
    };
    let permit = supervisor
        .budget_handle()
        .acquire_scan(source.id.as_str())
        .expect("admit external source work");

    drop(permit);

    assert!(retained_generation.load(Ordering::Acquire));
    let control = supervisor.shared.control();
    assert!(control.dirty_sources.contains(source.id.as_str()));
    assert!(!control.source_work_cancels[source.id.as_str()].load(Ordering::Acquire));
    assert!(!Arc::ptr_eq(
        &retained_generation,
        &control.source_work_cancels[source.id.as_str()]
    ));
    drop(control);
    assert_eq!(supervisor.shutdown()["joined"], true);
}

#[test]
fn targeted_scan_handoff_is_registered_before_delayed_gui_delivery() {
    let (_directory, source) = unhashed_source("targeted-scan-handoff");
    let mut supervisor = SourceProcessingSupervisor::dormant();
    supervisor
        .replace_sources(vec![source.clone()])
        .expect("configure source");
    {
        let mut control = supervisor.shared.control();
        control.dirty_sources.clear();
    }
    let permit = supervisor
        .budget_handle()
        .acquire_scan(source.id.as_str())
        .expect("admit targeted sync");
    let delta = CommittedSourceDelta {
        revision: 11,
        changed: vec![wavecrate::sample_sources::scanner::ManifestIdentityDelta {
            identity: String::from("targeted.wav"),
            relative_path: PathBuf::from("targeted.wav"),
            content_generation: String::from("generation-11"),
            source_metadata_changed: false,
        }],
        ..CommittedSourceDelta::default()
    };

    permit.release_after_handoff(ExternalScanHandoff::CommittedDelta(delta.clone()));

    let control = supervisor.shared.control();
    let pending = control
        .pending_readiness_deltas
        .get(source.id.as_str())
        .expect("targeted commit is owned before GUI delivery");
    assert!(pending.scope_ids.contains("targeted.wav"));
    assert!(control.dirty_sources.contains(source.id.as_str()));
    drop(control);

    // The delayed GUI completion may coalesce the same revision without widening the target set.
    supervisor.request_source_delta(
        source.id.as_str(),
        supervisor.lifecycle_generations()[source.id.as_str()],
        &delta,
        "delayed_targeted_gui_completion",
    );
    let control = supervisor.shared.control();
    assert_eq!(
        control
            .pending_readiness_deltas
            .get(source.id.as_str())
            .expect("coalesced delta remains pending")
            .scope_ids
            .len(),
        1
    );
    drop(control);
    assert_eq!(supervisor.shutdown()["joined"], true);
}

#[test]
fn projection_handoff_rejection_keeps_delta_and_readiness_fenced_until_gui_resolution() {
    let (_directory, source) = unhashed_source("projection-handoff-rejection");
    let mut supervisor = SourceProcessingSupervisor::dormant();
    supervisor
        .replace_sources(vec![source.clone()])
        .expect("configure source");
    {
        let mut control = supervisor.shared.control();
        control.dirty_sources.clear();
    }
    let generation = supervisor.lifecycle_generations()[source.id.as_str()];
    let permit = supervisor
        .budget_handle()
        .acquire_scan_for_generation(source.id.as_str(), generation)
        .expect("admit targeted sync");
    let ticket = permit.release_after_projection_handoff(CommittedSourceDelta {
        revision: 11,
        changed: vec![wavecrate::sample_sources::scanner::ManifestIdentityDelta {
            identity: String::from("projection.wav"),
            relative_path: PathBuf::from("projection.wav"),
            content_generation: String::from("generation-11"),
            source_metadata_changed: true,
        }],
        ..CommittedSourceDelta::default()
    });

    let control = supervisor.shared.control();
    assert!(
        control
            .pending_projection_fences
            .contains_key(source.id.as_str())
    );
    assert!(
        !control
            .pending_readiness_deltas
            .contains_key(source.id.as_str())
    );
    drop(control);

    ticket.reject("projection_handoff_test_rejection");
    let control = supervisor.shared.control();
    assert!(
        !control
            .pending_projection_fences
            .contains_key(source.id.as_str())
    );
    assert!(
        !control
            .pending_readiness_deltas
            .contains_key(source.id.as_str())
    );
    assert!(control.dirty_sources.contains(source.id.as_str()));
    drop(control);
    assert_eq!(supervisor.shutdown()["joined"], true);
}

#[test]
fn accepted_empty_projection_handoff_is_a_checkpoint_only_noop() {
    let (_directory, source) = unhashed_source("projection-handoff-empty");
    let mut supervisor = SourceProcessingSupervisor::dormant();
    supervisor
        .replace_sources(vec![source.clone()])
        .expect("configure source");
    {
        let mut control = supervisor.shared.control();
        control.dirty_sources.clear();
    }
    let generation = supervisor.lifecycle_generations()[source.id.as_str()];
    let permit = supervisor
        .budget_handle()
        .acquire_scan_for_generation(source.id.as_str(), generation)
        .expect("admit targeted sync");
    let ticket = permit.release_after_projection_handoff(CommittedSourceDelta {
        revision: 11,
        ..CommittedSourceDelta::default()
    });

    assert!(ticket.accept());
    let control = supervisor.shared.control();
    assert!(
        !control
            .pending_projection_fences
            .contains_key(source.id.as_str())
    );
    assert!(
        !control
            .pending_readiness_deltas
            .contains_key(source.id.as_str())
    );
    assert!(!control.dirty_sources.contains(source.id.as_str()));
    drop(control);
    assert_eq!(supervisor.shutdown()["joined"], true);
}

#[test]
fn ignored_projection_handoff_delta_requests_full_reconciliation() {
    let (_directory, source) = unhashed_source("projection-handoff-ignored");
    let mut supervisor = SourceProcessingSupervisor::dormant();
    supervisor
        .replace_sources(vec![source.clone()])
        .expect("configure source");
    let generation = supervisor.lifecycle_generations()[source.id.as_str()];
    {
        let mut control = supervisor.shared.control();
        control.accept_reconciled_manifest_revision(source.id.as_str(), generation, 11);
        control.dirty_sources.clear();
    }
    let permit = supervisor
        .budget_handle()
        .acquire_scan_for_generation(source.id.as_str(), generation)
        .expect("admit targeted sync");
    let ticket = permit.release_after_projection_handoff(readiness_delta(11, "ignored"));

    assert!(
        !ticket.accept(),
        "an ignored non-empty delta needs recovery"
    );
    let control = supervisor.shared.control();
    assert!(
        !control
            .pending_projection_fences
            .contains_key(source.id.as_str())
    );
    assert!(
        !control
            .pending_readiness_deltas
            .contains_key(source.id.as_str())
    );
    assert!(control.dirty_sources.contains(source.id.as_str()));
    drop(control);
    assert_eq!(supervisor.shutdown()["joined"], true);
}

#[test]
fn duplicate_old_projection_ticket_cannot_clear_newer_same_source_fence() {
    let (_directory, source) = unhashed_source("projection-handoff-duplicate");
    let mut supervisor = SourceProcessingSupervisor::dormant();
    supervisor
        .replace_sources(vec![source.clone()])
        .expect("configure source");
    let generation = supervisor.lifecycle_generations()[source.id.as_str()];
    {
        let mut control = supervisor.shared.control();
        control.dirty_sources.clear();
    }

    let first_permit = supervisor
        .budget_handle()
        .acquire_scan_for_generation(source.id.as_str(), generation)
        .expect("admit first targeted sync");
    let first_ticket = first_permit.release_after_projection_handoff(readiness_delta(1, "first"));
    assert!(first_ticket.accept());

    let second_permit = supervisor
        .budget_handle()
        .acquire_scan_for_generation(source.id.as_str(), generation)
        .expect("admit second targeted sync");
    let second_ticket =
        second_permit.release_after_projection_handoff(readiness_delta(2, "second"));

    assert!(
        !first_ticket.accept(),
        "duplicate old ticket must remain stale"
    );
    let control = supervisor.shared.control();
    assert_eq!(
        control
            .pending_projection_fences
            .get(source.id.as_str())
            .map(|fence| (fence.lifecycle_generation, fence.revision)),
        Some((generation, 2)),
        "a stale duplicate must not clear the newer same-source fence"
    );
    drop(control);

    second_ticket.reject("projection_handoff_duplicate_test_cleanup");
    assert_eq!(supervisor.shutdown()["joined"], true);
}

#[test]
fn foreground_scan_handoff_uses_full_fallback_before_capacity_release() {
    let (_directory, source) = unhashed_source("foreground-scan-fallback");
    let mut supervisor = SourceProcessingSupervisor::dormant();
    supervisor
        .replace_sources(vec![source.clone()])
        .expect("configure source");
    {
        let mut control = supervisor.shared.control();
        control.dirty_sources.clear();
    }
    let permit = supervisor
        .budget_handle()
        .acquire_scan(source.id.as_str())
        .expect("admit foreground scan");

    permit.release_after_handoff(ExternalScanHandoff::FullReconciliation {
        reason: "foreground_scan_test_fallback",
    });

    let control = supervisor.shared.control();
    assert!(control.dirty_sources.contains(source.id.as_str()));
    assert!(
        !control
            .pending_readiness_deltas
            .contains_key(source.id.as_str())
    );
    drop(control);
    assert_eq!(supervisor.shutdown()["joined"], true);
}

#[test]
fn foreground_scan_handoff_keeps_one_file_delta_bounded_before_gui_delivery() {
    let (_directory, source) = unhashed_source("foreground-scan-delta");
    let mut supervisor = SourceProcessingSupervisor::dormant();
    supervisor
        .replace_sources(vec![source.clone()])
        .expect("configure source");
    {
        let mut control = supervisor.shared.control();
        control.dirty_sources.clear();
    }
    let permit = supervisor
        .budget_handle()
        .acquire_scan(source.id.as_str())
        .expect("admit foreground scan");
    let delta = CommittedSourceDelta {
        revision: 3,
        created: vec![wavecrate::sample_sources::scanner::ManifestIdentityDelta {
            identity: String::from("foreground.wav"),
            relative_path: PathBuf::from("foreground.wav"),
            content_generation: String::from("generation-3"),
            source_metadata_changed: false,
        }],
        ..CommittedSourceDelta::default()
    };

    permit.release_after_handoff(ExternalScanHandoff::CommittedDelta(delta));

    let control = supervisor.shared.control();
    assert_eq!(
        control
            .pending_readiness_deltas
            .get(source.id.as_str())
            .expect("foreground commit is owned before GUI delivery")
            .scope_ids,
        [String::from("foreground.wav")].into_iter().collect()
    );
    drop(control);
    assert_eq!(supervisor.shutdown()["joined"], true);
}

#[test]
fn coalesced_scan_delta_revision_gap_promotes_to_full_reconciliation() {
    let (_directory, source) = unhashed_source("scan-revision-gap");
    let mut supervisor = SourceProcessingSupervisor::dormant();
    supervisor
        .replace_sources(vec![source.clone()])
        .expect("configure source");
    {
        let mut control = supervisor.shared.control();
        control.dirty_sources.clear();
    }
    let first = CommittedSourceDelta {
        revision: 7,
        changed: vec![wavecrate::sample_sources::scanner::ManifestIdentityDelta {
            identity: String::from("first"),
            relative_path: PathBuf::from("first.wav"),
            content_generation: String::from("generation-7"),
            source_metadata_changed: false,
        }],
        ..CommittedSourceDelta::default()
    };
    supervisor.request_source_delta(
        source.id.as_str(),
        supervisor.lifecycle_generations()[source.id.as_str()],
        &first,
        "first_scan_delta",
    );
    let gap = CommittedSourceDelta {
        revision: 9,
        changed: vec![wavecrate::sample_sources::scanner::ManifestIdentityDelta {
            identity: String::from("gap"),
            relative_path: PathBuf::from("gap.wav"),
            content_generation: String::from("generation-9"),
            source_metadata_changed: false,
        }],
        ..CommittedSourceDelta::default()
    };
    supervisor.request_source_delta(
        source.id.as_str(),
        supervisor.lifecycle_generations()[source.id.as_str()],
        &gap,
        "gap_scan_delta",
    );

    let control = supervisor.shared.control();
    assert!(
        !control
            .pending_readiness_deltas
            .contains_key(source.id.as_str())
    );
    assert!(control.dirty_sources.contains(source.id.as_str()));
    drop(control);
    assert_eq!(supervisor.shutdown()["joined"], true);
}

#[test]
fn scan_registration_only_adds_absent_matching_sources() {
    let (_directory, source) = unhashed_source("scan-registration");
    let mut supervisor = SourceProcessingSupervisor::dormant();

    supervisor
        .register_source_for_scan(source.clone())
        .expect("register source before first scan");
    supervisor
        .register_source_for_scan(source.clone())
        .expect("matching registration is idempotent");
    let permit = supervisor
        .budget_handle()
        .acquire_scan(source.id.as_str())
        .expect("newly registered source admits scan work");

    let replacement_directory = tempfile::tempdir().expect("replacement source root");
    let replacement = SampleSource::new_with_id(
        source.id.clone(),
        replacement_directory.path().to_path_buf(),
    );
    assert!(
        supervisor.register_source_for_scan(replacement).is_err(),
        "scan registration must not replace an authoritative descriptor"
    );
    let control = supervisor.shared.control();
    assert!(source_descriptors_match(
        &control.sources[source.id.as_str()],
        &source
    ));
    drop(control);
    drop(permit);

    let replacement = supervisor
        .shared
        .source_replacement
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    assert_eq!(
        supervisor
            .register_source_for_scan(source.clone())
            .expect_err("scan registration must not wait for source replacement"),
        "Configured sources are currently being replaced"
    );
    drop(replacement);
    assert_eq!(supervisor.shutdown()["joined"], true);
}

#[test]
fn background_scan_registration_waits_for_source_replacement_fence() {
    let (_directory, source) = unhashed_source("scan-registration-waiting");
    let mut supervisor = SourceProcessingSupervisor::dormant();
    supervisor
        .replace_sources(vec![source.clone()])
        .expect("configure authoritative source");
    let replacement = supervisor
        .shared
        .source_replacement
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let budget = supervisor.budget_handle();
    let source_for_worker = source.clone();
    let (sender, receiver) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let result = budget.register_source_for_scan_waiting(source_for_worker);
        sender.send(result).expect("publish registration result");
    });

    assert!(
        receiver.recv_timeout(Duration::from_millis(25)).is_err(),
        "background admission should wait while source replacement owns the fence"
    );
    drop(replacement);
    let generation = receiver
        .recv_timeout(Duration::from_secs(1))
        .expect("registration should resume after replacement")
        .expect("register matching source");
    worker.join().expect("join registration worker");

    assert_eq!(
        supervisor.lifecycle_generations()[source.id.as_str()],
        generation
    );
    let permit = supervisor
        .budget_handle()
        .acquire_scan_for_generation(source.id.as_str(), generation)
        .expect("deferred registration must admit the external scan");
    drop(permit);
    assert_eq!(supervisor.shutdown()["joined"], true);
}

#[test]
fn source_replacement_cancels_before_waiting_and_advances_after_publication() {
    let first_directory = tempfile::tempdir().expect("first source directory");
    let replacement_directory = tempfile::tempdir().expect("replacement source directory");
    let source = SampleSource::new_with_id(
        SourceId::from_string("publication-fenced-replacement"),
        first_directory.path().to_path_buf(),
    );
    let replacement = SampleSource::new_with_id(
        source.id.clone(),
        replacement_directory.path().to_path_buf(),
    );
    let shared = Arc::new(Shared::new(vec![source.clone()], None));
    let old_cancel = shared.control().source_work_cancels[source.id.as_str()].clone();
    let old_generation = shared.control().source_lifecycle_generations[source.id.as_str()];
    let publication = shared.database_writer.lock(DatabasePhase::Publish);
    let replacement_shared = Arc::clone(&shared);
    let replacement_worker = std::thread::spawn(move || {
        SourceProcessingSupervisor {
            shared: replacement_shared,
            coordinator: None,
            retirement_worker: None,
        }
        .replace_sources(vec![replacement])
        .expect("replace source after publication");
    });

    let deadline = Instant::now() + Duration::from_secs(1);
    while shared.database_writer.waiting_count() == 0 {
        assert!(
            Instant::now() < deadline,
            "source replacement did not wait for publication"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        shared.control().source_lifecycle_generations[source.id.as_str()],
        old_generation,
        "lifecycle generation must not advance during an active publication"
    );
    assert!(
        old_cancel.load(Ordering::Acquire),
        "replacement must release a foreground scan before waiting for its publication permit"
    );

    drop(publication);
    replacement_worker.join().expect("replacement worker joins");
    assert!(old_cancel.load(Ordering::Acquire));
    assert_ne!(
        shared.control().source_lifecycle_generations[source.id.as_str()],
        old_generation
    );
}

#[test]
fn background_scan_registration_cannot_readd_source_removed_behind_fence() {
    let (_directory, source) = unhashed_source("scan-registration-removed");
    let mut supervisor = SourceProcessingSupervisor::dormant();
    let replacement = supervisor
        .shared
        .source_replacement
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let budget = supervisor.budget_handle();
    let source_for_worker = source.clone();
    let worker =
        std::thread::spawn(move || budget.register_source_for_scan_waiting(source_for_worker));

    thread::sleep(Duration::from_millis(25));
    drop(replacement);
    let error = worker
        .join()
        .expect("join deferred scan registration")
        .expect_err("removed source must not be registered by a stale scan");

    assert!(error.contains("no longer present"));
    assert!(
        !supervisor
            .lifecycle_generations()
            .contains_key(source.id.as_str()),
        "stale scan admission must not resurrect a removed source"
    );
    assert_eq!(supervisor.shutdown()["joined"], true);
}

#[test]
fn watcher_ready_requests_only_a_gated_lifecycle_probe() {
    let (_directory, source) = unhashed_source("watcher-ready-audit");
    let mut supervisor = SourceProcessingSupervisor::dormant();
    supervisor
        .replace_sources(vec![source.clone()])
        .expect("configure source");
    {
        let mut control = supervisor.shared.control();
        control.force_manifest_audit_sources.clear();
        control.dirty_sources.clear();
        control.safety_probe_sources.clear();
    }

    supervisor.request_lifecycle_audit_probe(
        SourceAuditLifecycleCause::WatcherReady,
        &[source.id.as_str().to_string()],
    );

    let control = supervisor.shared.control();
    assert!(
        !control
            .force_manifest_audit_sources
            .contains(source.id.as_str()),
        "watcher readiness must not force a full audit when durable coverage is current"
    );
    assert!(control.dirty_sources.contains(source.id.as_str()));
    assert!(control.safety_probe_sources.contains(source.id.as_str()));
    assert!(
        control
            .deferred_lifecycle_audit_sources
            .contains(source.id.as_str()),
        "watcher-ready must retain a source whose unavailable fallback still needs a fresh audit barrier"
    );
    drop(control);
    assert_eq!(supervisor.shutdown()["joined"], true);
}

#[test]
fn repeated_focus_regain_requests_never_force_manifest_audits() {
    let (_directory, source) = unhashed_source("focus-regained-audit");
    let mut supervisor = SourceProcessingSupervisor::dormant();
    supervisor
        .replace_sources(vec![source.clone()])
        .expect("configure source");
    {
        let mut control = supervisor.shared.control();
        control.force_manifest_audit_sources.clear();
        control.dirty_sources.clear();
        control.safety_probe_sources.clear();
    }

    for _ in 0..10 {
        supervisor.request_lifecycle_audit_probe(SourceAuditLifecycleCause::FocusRegained, &[]);
        let mut control = supervisor.shared.control();
        assert!(
            !control
                .force_manifest_audit_sources
                .contains(source.id.as_str()),
            "refocus must remain gated by durable health rather than force a traversal"
        );
        assert!(control.dirty_sources.contains(source.id.as_str()));
        assert!(control.safety_probe_sources.contains(source.id.as_str()));
        // Model a current durable no-op probe before the next focus transition.
        control.dirty_sources.clear();
        control.safety_probe_sources.clear();
    }
    assert_eq!(supervisor.shutdown()["joined"], true);
}

#[test]
fn watcher_ready_probe_coalesces_after_an_older_probe_was_captured() {
    let (_directory, source) = unhashed_source("watcher-ready-in-flight-audit");
    let mut supervisor = SourceProcessingSupervisor::dormant();
    supervisor
        .replace_sources(vec![source.clone()])
        .expect("configure source");
    {
        let mut control = supervisor.shared.control();
        // Model the coordinator having already captured the original startup
        // probe and started its candidate.
        control.dirty_sources.clear();
        control.safety_probe_sources.clear();
        control.force_manifest_audit_sources.clear();
    }

    supervisor.request_lifecycle_audit_probe(SourceAuditLifecycleCause::WatcherReady, &[]);

    {
        let mut control = supervisor.shared.control();
        assert!(control.dirty_sources.contains(source.id.as_str()));
        assert!(
            control.safety_probe_sources.contains(source.id.as_str()),
            "the watcher-ready probe must survive an older captured lifecycle probe"
        );
        assert!(
            !control
                .force_manifest_audit_sources
                .contains(source.id.as_str())
        );
        control.dirty_sources.remove(source.id.as_str());
        control.safety_probe_sources.remove(source.id.as_str());
    }
    assert!(
        !supervisor
            .shared
            .control()
            .force_manifest_audit_sources
            .contains(source.id.as_str())
    );
    assert_eq!(supervisor.shutdown()["joined"], true);
}

#[test]
fn watcher_history_gap_forces_only_the_affected_source_audit() {
    let (_first_directory, first) = unhashed_source("watcher-history-gap-first");
    let (_second_directory, second) = unhashed_source("watcher-history-gap-second");
    let mut supervisor = SourceProcessingSupervisor::dormant();
    supervisor
        .replace_sources(vec![first.clone(), second.clone()])
        .expect("configure sources");
    {
        let mut control = supervisor.shared.control();
        control.dirty_sources.clear();
        control.safety_probe_sources.clear();
        control.force_manifest_audit_sources.clear();
    }

    supervisor.request_source_manifest_audit(first.id.as_str(), "watcher_history_gap");

    let control = supervisor.shared.control();
    assert!(
        control
            .force_manifest_audit_sources
            .contains(first.id.as_str())
    );
    assert!(control.dirty_sources.contains(first.id.as_str()));
    assert!(
        !control
            .force_manifest_audit_sources
            .contains(second.id.as_str())
    );
    assert!(!control.dirty_sources.contains(second.id.as_str()));
    drop(control);
    assert_eq!(supervisor.shutdown()["joined"], true);
}

#[test]
fn external_admission_rejects_generation_captured_before_descriptor_replacement() {
    let old_directory = tempfile::tempdir().expect("old source root");
    let replacement_directory = tempfile::tempdir().expect("replacement source root");
    let source_id = SourceId::from_string("replaced-external-admission");
    let old_source =
        SampleSource::new_with_id(source_id.clone(), old_directory.path().to_path_buf());
    let replacement =
        SampleSource::new_with_id(source_id, replacement_directory.path().to_path_buf());
    let mut supervisor = SourceProcessingSupervisor::dormant();
    supervisor
        .replace_sources(vec![old_source.clone()])
        .expect("configure old descriptor");
    let handle = supervisor.budget_handle();
    let old_generation = handle
        .lifecycle_generation(old_source.id.as_str())
        .expect("capture queued request generation");

    supervisor
        .replace_sources(vec![replacement])
        .expect("replace source descriptor before admission");

    assert!(
        handle
            .acquire_scan_for_generation(old_source.id.as_str(), old_generation)
            .is_none(),
        "a queued request must not adopt the replacement descriptor generation"
    );
    assert_ne!(
        handle
            .lifecycle_generation(old_source.id.as_str())
            .expect("replacement generation"),
        old_generation
    );
    assert_eq!(supervisor.shutdown()["joined"], true);
}
fn readiness_delta(revision: u64, identity: &str) -> CommittedSourceDelta {
    CommittedSourceDelta {
        revision,
        changed: vec![wavecrate::sample_sources::scanner::ManifestIdentityDelta {
            identity: identity.to_string(),
            relative_path: PathBuf::from(format!("{identity}.wav")),
            content_generation: format!("generation-{revision}"),
            source_metadata_changed: false,
        }],
        ..CommittedSourceDelta::default()
    }
}

#[test]
fn full_reconciliation_accepts_durable_revision_when_delta_was_not_applied() {
    let full_reconciliation = SourceDiscoveryStats {
        delta_reconciled: false,
        reconciled_manifest_revision: Some(7),
        ..SourceDiscoveryStats::default()
    };
    assert_eq!(
        manifest_revision_to_accept(&full_reconciliation, Some(5)),
        Some(7),
        "a complete full reconciliation must fence through its durable revision"
    );

    let delta_reconciliation = SourceDiscoveryStats {
        delta_reconciled: true,
        reconciled_manifest_revision: Some(7),
        ..SourceDiscoveryStats::default()
    };
    assert_eq!(
        manifest_revision_to_accept(&delta_reconciliation, Some(5)),
        Some(5),
        "an applied bounded delta retains its accepted delta revision"
    );
}

#[test]
fn accepted_manifest_revision_survives_delta_consumption_without_rescheduling() {
    let (_directory, source) = unhashed_source("accepted-revision-consumption");
    let mut supervisor = SourceProcessingSupervisor::dormant();
    supervisor
        .replace_sources(vec![source.clone()])
        .expect("configure source");
    let lifecycle_generation = supervisor.lifecycle_generations()[source.id.as_str()];
    let delta = readiness_delta(4, "consumed");
    {
        let mut control = supervisor.shared.control();
        assert_eq!(
            control.queue_source_delta(
                source.id.as_str(),
                lifecycle_generation,
                &delta,
                "test_consumption",
            ),
            SourceDeltaQueueResult::Queued
        );
        control.accept_reconciled_manifest_revision(
            source.id.as_str(),
            lifecycle_generation,
            delta.revision,
        );
        control.pending_readiness_deltas.remove(source.id.as_str());
        control.dirty_sources.clear();
        let wake_generation = control.wake_generation;
        drop(control);
        supervisor.request_source_delta(
            source.id.as_str(),
            lifecycle_generation,
            &delta,
            "delayed_consumption_completion",
        );
        let control = supervisor.shared.control();
        assert_eq!(
            control.accepted_manifest_revisions[source.id.as_str()].revision,
            delta.revision
        );
        assert!(!control.dirty_sources.contains(source.id.as_str()));
        assert!(
            !control
                .pending_readiness_deltas
                .contains_key(source.id.as_str())
        );
        assert_eq!(control.wake_generation, wake_generation);
    }
    let next = readiness_delta(5, "newly-accepted");
    supervisor.request_source_delta(
        source.id.as_str(),
        lifecycle_generation,
        &next,
        "new_accepted_revision",
    );
    {
        let mut control = supervisor.shared.control();
        assert!(
            control
                .pending_readiness_deltas
                .contains_key(source.id.as_str())
        );
        control.accept_reconciled_manifest_revision(
            source.id.as_str(),
            lifecycle_generation,
            next.revision,
        );
        assert_eq!(
            control.accepted_manifest_revisions[source.id.as_str()].revision,
            next.revision
        );
    }
    let gap = readiness_delta(7, "post_consumption_gap");
    supervisor.request_source_delta(
        source.id.as_str(),
        lifecycle_generation,
        &gap,
        "post_consumption_gap",
    );
    let control = supervisor.shared.control();
    assert!(
        !control
            .pending_readiness_deltas
            .contains_key(source.id.as_str())
    );
    assert_eq!(
        control.accepted_manifest_revisions[source.id.as_str()].recovery_floor,
        Some(gap.revision)
    );
    drop(control);
    assert_eq!(supervisor.shutdown()["joined"], true);
}

#[test]
fn gap_fallback_fences_delayed_revisions_until_recovery_is_reconciled() {
    let (_directory, source) = unhashed_source("accepted-revision-gap");
    let mut supervisor = SourceProcessingSupervisor::dormant();
    supervisor
        .replace_sources(vec![source.clone()])
        .expect("configure source");
    let lifecycle_generation = supervisor.lifecycle_generations()[source.id.as_str()];
    let first = readiness_delta(7, "first-gap");
    let gap = readiness_delta(9, "gap");
    {
        let mut control = supervisor.shared.control();
        assert_eq!(
            control.queue_source_delta(
                source.id.as_str(),
                lifecycle_generation,
                &first,
                "test_gap_first",
            ),
            SourceDeltaQueueResult::Queued
        );
        assert_eq!(
            control.queue_source_delta(
                source.id.as_str(),
                lifecycle_generation,
                &gap,
                "test_gap_fallback",
            ),
            SourceDeltaQueueResult::Fallback
        );
        control.dirty_sources.clear();
    }
    supervisor.request_source_delta(
        source.id.as_str(),
        lifecycle_generation,
        &first,
        "delayed_gap_first",
    );
    supervisor.request_source_delta(
        source.id.as_str(),
        lifecycle_generation,
        &gap,
        "delayed_gap_completion",
    );
    {
        let control = supervisor.shared.control();
        let fence = &control.accepted_manifest_revisions[source.id.as_str()];
        assert_eq!(
            fence.revision, 0,
            "recovery has not been durably reconciled"
        );
        assert_eq!(fence.recovery_floor, Some(gap.revision));
        assert!(!control.dirty_sources.contains(source.id.as_str()));
        assert!(
            !control
                .pending_readiness_deltas
                .contains_key(source.id.as_str())
        );
    }
    {
        let mut control = supervisor.shared.control();
        control.accept_reconciled_manifest_revision(
            source.id.as_str(),
            lifecycle_generation,
            gap.revision,
        );
        assert_eq!(
            control.accepted_manifest_revisions[source.id.as_str()].revision,
            gap.revision
        );
        assert_eq!(
            control.accepted_manifest_revisions[source.id.as_str()].recovery_floor,
            None
        );
    }
    assert_eq!(supervisor.shutdown()["joined"], true);
}

#[test]
fn cancelled_reconciliation_does_not_advance_accepted_manifest_revision() {
    let (_directory, source) = unhashed_source("accepted-revision-cancelled");
    let mut supervisor = SourceProcessingSupervisor::dormant();
    supervisor
        .replace_sources(vec![source.clone()])
        .expect("configure source");
    let lifecycle_generation = supervisor.lifecycle_generations()[source.id.as_str()];
    let delta = readiness_delta(5, "cancelled");
    supervisor.request_source_delta(
        source.id.as_str(),
        lifecycle_generation,
        &delta,
        "cancelled_delta",
    );
    let mut control = supervisor.shared.control();
    control.cancel_source_work(source.id.as_str());
    assert!(
        !control
            .accepted_manifest_revisions
            .contains_key(source.id.as_str())
    );
    assert!(
        control
            .pending_readiness_deltas
            .contains_key(source.id.as_str())
    );
    drop(control);
    assert_eq!(supervisor.shutdown()["joined"], true);
}

#[test]
fn lifecycle_replacement_resets_manifest_revision_fence() {
    let (_directory, source) = unhashed_source("accepted-revision-lifecycle");
    let mut supervisor = SourceProcessingSupervisor::dormant();
    supervisor
        .replace_sources(vec![source.clone()])
        .expect("configure source");
    let old_generation = supervisor.lifecycle_generations()[source.id.as_str()];
    {
        let mut control = supervisor.shared.control();
        control.accept_reconciled_manifest_revision(source.id.as_str(), old_generation, 12);
    }
    let replacement_directory = tempfile::tempdir().expect("replacement source root");
    let replacement = SampleSource::new_with_id(
        source.id.clone(),
        replacement_directory.path().to_path_buf(),
    );
    supervisor
        .replace_sources(vec![replacement])
        .expect("replace source lifecycle");
    let new_generation = supervisor.lifecycle_generations()[source.id.as_str()];
    assert_ne!(new_generation, old_generation);
    let control = supervisor.shared.control();
    assert!(
        !control
            .accepted_manifest_revisions
            .contains_key(source.id.as_str())
    );
    drop(control);
    supervisor.request_source_delta(
        source.id.as_str(),
        old_generation,
        &readiness_delta(12, "old-lifecycle"),
        "delayed_old_lifecycle",
    );
    assert!(
        !supervisor
            .pending_source_delta_contains_identity_for_tests(source.id.as_str(), "old-lifecycle",)
    );
    assert_eq!(supervisor.shutdown()["joined"], true);
}
