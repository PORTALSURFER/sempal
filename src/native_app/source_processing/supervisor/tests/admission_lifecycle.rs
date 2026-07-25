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

    supervisor
        .finish_foreground_source_refresh(source.id.as_str(), "unchanged_foreground_scan");

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
        assert!(!control.force_manifest_audit_sources.contains(source.id.as_str()));
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
    assert!(control.force_manifest_audit_sources.contains(first.id.as_str()));
    assert!(control.dirty_sources.contains(first.id.as_str()));
    assert!(!control.force_manifest_audit_sources.contains(second.id.as_str()));
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
