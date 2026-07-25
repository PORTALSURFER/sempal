#[test]
fn shutdown_waits_for_external_scan_admissions_and_rejects_late_permits() {
    let (_directory, source) = unhashed_source("admission-race");
    let mut supervisor = SourceProcessingSupervisor::dormant();
    supervisor
        .replace_sources(vec![source.clone()])
        .expect("configure source");
    let handle = supervisor.budget_handle();
    let first = handle
        .acquire_scan(source.id.as_str())
        .expect("reserve the only scan lane");
    let first_cancel = first.cancel_token();
    let waiting_handle = handle.clone();
    let source_id = source.id.to_string();
    let waiting = thread::spawn(move || waiting_handle.acquire_scan(&source_id).is_none());
    wait_until(Duration::from_secs(2), || {
        supervisor.shared.external_scans().admissions.len() == 1
    });

    let shutdown = thread::spawn(move || supervisor.shutdown());
    wait_until(Duration::from_secs(2), || {
        first_cancel.load(Ordering::Acquire)
    });
    drop(first);

    assert!(waiting.join().expect("join waiting admission"));
    let report = shutdown.join().expect("join supervisor shutdown");
    assert_eq!(report["joined"], true);
    assert_eq!(report["external_scans_joined"], true);
}

#[test]
fn foreground_scan_admission_waits_without_cancelling_background_work() {
    let (_first_directory, first) = unhashed_source("background-holder");
    let (_second_directory, second) = unhashed_source("foreground-waiter");
    let shared = Arc::new(Shared::new(vec![first.clone(), second.clone()], None));
    let background_cancel = {
        let control = shared.control();
        Arc::clone(&control.source_work_cancels[first.id.as_str()])
    };
    let background_permit = shared
        .budgets()
        .try_acquire(first.id.as_str(), ProcessingLane::Hashing)
        .expect("reserve database capacity for background hashing");
    let waiting_shared = Arc::clone(&shared);
    let foreground_source_id = second.id.to_string();
    let foreground_generation = shared.control().source_lifecycle_generations[second.id.as_str()];
    let states = Arc::new(Mutex::new(Vec::new()));
    let worker_states = Arc::clone(&states);
    let waiting = thread::spawn(move || {
        SourceProcessingBudgetHandle {
            shared: waiting_shared,
        }
        .acquire_scan_for_generation_with_state(
            &foreground_source_id,
            foreground_generation,
            |state| worker_states.lock().unwrap().push(state),
        )
    });

    wait_until(Duration::from_secs(2), || {
        shared.external_scans().admissions.len() == 1
    });
    assert!(
        !background_cancel.load(Ordering::Acquire),
        "external scan admission must let active source work finish"
    );
    shared.budgets().release(background_permit);
    shared.budget_wake.notify_all();

    let foreground_permit = waiting
        .join()
        .expect("join foreground admission")
        .expect("foreground scan acquires released lane");
    assert_eq!(
        foreground_permit
            .permit
            .as_ref()
            .expect("owned budget permit")
            .source_id(),
        second.id.as_str()
    );
    assert_eq!(
        states.lock().unwrap().as_slice(),
        [
            SourceScanAdmissionState::WaitingForCapacity {
                current_owner: Some(first.id.to_string()),
            },
            SourceScanAdmissionState::Admitted,
        ],
        "admission must publish each semantic wait transition once"
    );
    drop(foreground_permit);
}

#[test]
fn foreground_scan_admission_reserves_all_processing_capacity() {
    let (_directory, source) = unhashed_source("foreground-reservation");
    let candidates = vec![
        RuntimeCandidate {
            schedule: WorkCandidate::source(source.id.as_str(), ProcessingLane::Scan, 0, 0),
            source: source.clone(),
            task: RuntimeTask::ManifestAudit { accelerated: false },
        },
        RuntimeCandidate {
            schedule: WorkCandidate::source(source.id.as_str(), ProcessingLane::Hashing, 0, 0),
            source,
            task: RuntimeTask::ManifestAudit { accelerated: false },
        },
    ];

    assert_eq!(scheduler_candidate_indices(&candidates, false), vec![0, 1]);
    assert!(scheduler_candidate_indices(&candidates, true).is_empty());
}

#[test]
fn foreground_activity_does_not_cancel_in_flight_work_or_external_scans() {
    let (_directory, source) = unhashed_source("foreground");
    let mut supervisor = SourceProcessingSupervisor::dormant();
    supervisor
        .replace_sources(vec![source.clone()])
        .expect("configure source");
    let source_generation = {
        let control = supervisor.shared.control();
        Arc::clone(&control.source_work_cancels[source.id.as_str()])
    };
    let scan_permit = supervisor
        .budget_handle()
        .acquire_scan(source.id.as_str())
        .expect("acquire external scan permit");
    let scan_generation = scan_permit.cancel_token();

    supervisor.set_foreground_activity(true);

    assert!(!source_generation.load(Ordering::Acquire));
    assert!(!scan_generation.load(Ordering::Acquire));
    drop(scan_permit);

    supervisor.set_foreground_activity(false);

    let control = supervisor.shared.control();
    assert!(!control.source_work_cancels[source.id.as_str()].load(Ordering::Acquire));
    drop(control);
    assert_eq!(supervisor.shutdown()["joined"], true);
}

#[test]
fn read_only_discovery_does_not_publish_or_mutate_work() {
    let (_directory, source) = ready_analysis_source("read-only-discovery");
    let database_root = source.database_root().expect("database root");
    let mut connection = SourceDatabase::open_connection_with_role_and_database_root(
        &source.root,
        &database_root,
        SourceDatabaseConnectionRole::UiRead,
    )
    .expect("open read-only source database");
    assert!(connection.is_readonly(rusqlite::MAIN_DB).unwrap());
    let counts_before = discovery_durable_counts(&connection);

    let cancel = AtomicBool::new(false);
    let Cancellable::Completed((candidates, stats)) =
        discover_source_candidates_with_connection(&source, &mut connection, 100, false, &cancel)
            .expect("skip read-only source processing")
    else {
        panic!("read-only discovery unexpectedly cancelled");
    };

    assert!(candidates.is_empty());
    assert_eq!(stats.readiness_queue_depth, 0);
    assert_eq!(discovery_durable_counts(&connection), counts_before);
}

#[test]
fn manifest_audit_is_scheduled_only_when_the_active_source_is_due() {
    let directory = tempfile::tempdir().expect("manifest audit source");
    let source = SampleSource::new_with_id(
        SourceId::from_string("manifest-audit"),
        directory.path().to_path_buf(),
    );
    let db = source.open_db().expect("open manifest audit source");
    let cancel = AtomicBool::new(false);

    let Cancellable::Completed((due, _)) =
        discover_source_candidates(&source, MANIFEST_AUDIT_INTERVAL_SECONDS, false, &cancel)
            .expect("discover due manifest audit")
    else {
        panic!("manifest audit discovery unexpectedly cancelled");
    };
    assert!(
        due.iter()
            .any(|candidate| matches!(
                candidate.task,
                RuntimeTask::ManifestAudit { accelerated: false }
            ))
    );

    db.set_metadata(
        META_LAST_MANIFEST_AUDIT_AT,
        &MANIFEST_AUDIT_INTERVAL_SECONDS.to_string(),
    )
    .expect("record manifest audit");
    let Cancellable::Completed((not_due, _)) = discover_source_candidates(
        &source,
        MANIFEST_AUDIT_INTERVAL_SECONDS * 2 - 1,
        false,
        &cancel,
    )
    .expect("discover recent manifest audit") else {
        panic!("manifest audit discovery unexpectedly cancelled");
    };
    assert!(
        not_due
            .iter()
            .all(|candidate| !matches!(candidate.task, RuntimeTask::ManifestAudit { .. }))
    );

    let Cancellable::Completed((forced, _)) = discover_source_candidates(
        &source,
        MANIFEST_AUDIT_INTERVAL_SECONDS * 2 - 1,
        true,
        &cancel,
    )
    .expect("discover forced startup manifest audit") else {
        panic!("forced manifest audit discovery unexpectedly cancelled");
    };
    assert!(
        forced
            .iter()
            .any(|candidate| matches!(
                candidate.task,
                RuntimeTask::ManifestAudit { accelerated: true }
            ))
    );
}

#[test]
fn missing_manifest_identity_schedules_self_healing_audit_even_when_recent() {
    let (_directory, source) = unhashed_source("manifest-identity-repair");
    let db = source
        .open_db()
        .expect("open manifest identity repair source");
    let mut batch = db.write_batch().expect("open missing identity batch");
    batch
        .set_file_identity(Path::new("pending.wav"), None)
        .expect("clear manifest identity");
    batch.commit().expect("commit missing manifest identity");
    db.set_metadata(META_LAST_MANIFEST_AUDIT_AT, "100")
        .expect("record recent audit");
    let cancel = AtomicBool::new(false);

    let Cancellable::Completed((candidates, _)) =
        discover_source_candidates(&source, 100, false, &cancel)
            .expect("discover manifest identity repair")
    else {
        panic!("manifest identity repair discovery unexpectedly cancelled");
    };

    assert!(
        candidates
            .iter()
            .any(|candidate| matches!(candidate.task, RuntimeTask::ManifestAudit { .. }))
    );
}

#[test]
fn appledouble_sidecars_do_not_keep_manifest_audits_permanently_due() {
    let directory = tempfile::tempdir().expect("AppleDouble source");
    let source = SampleSource::new_with_id(
        SourceId::from_string("appledouble-audit"),
        directory.path().to_path_buf(),
    );
    let db = source.open_db().expect("open AppleDouble source");
    db.upsert_file(Path::new("folder/._sidecar.wav"), 4_096, 1)
        .expect("seed legacy AppleDouble row");
    db.set_metadata(META_LAST_MANIFEST_AUDIT_AT, "100")
        .expect("record recent audit");
    let cancel = AtomicBool::new(false);

    let Cancellable::Completed((candidates, _)) =
        discover_source_candidates(&source, 100, false, &cancel)
            .expect("discover source with ignored AppleDouble row")
    else {
        panic!("AppleDouble source discovery unexpectedly cancelled");
    };

    assert!(
        candidates
            .iter()
            .all(|candidate| !matches!(candidate.task, RuntimeTask::ManifestAudit { .. }))
    );
}

#[test]
fn missing_source_discovery_updates_external_metadata_without_recreating_audio_root() {
    let parent = tempfile::tempdir().expect("missing source parent");
    let root = parent.path().join("source");
    std::fs::create_dir(&root).expect("create source root");
    let source = SampleSource::new_with_id(SourceId::from_string("missing-source"), root.clone())
        .protected();
    let database_root = source.database_root().expect("external metadata root");
    let connection = SourceDatabase::open_connection_with_role_and_database_root(
        &source.root,
        &database_root,
        SourceDatabaseConnectionRole::JobWorker,
    )
    .expect("create external source database");
    connection
        .execute(
            "INSERT INTO source_readiness_sources (
                    source_id, source_generation, readiness_revision, availability, updated_at
                 ) VALUES (?1, 1, 1, 'active', 1)",
            [source.id.as_str()],
        )
        .expect("publish active source readiness");
    drop(connection);
    std::fs::remove_dir_all(&root).expect("remove source root");
    let cancel = AtomicBool::new(false);

    let Cancellable::Completed((candidates, _)) =
        discover_source_candidates(&source, 100, false, &cancel)
            .expect("discover unavailable source")
    else {
        panic!("missing source discovery unexpectedly cancelled");
    };

    assert!(candidates.is_empty());
    assert!(
        !root.exists(),
        "discovery must not recreate a missing source"
    );
    let connection = SourceDatabase::open_unavailable_source_metadata_connection(
        &database_root,
        SourceDatabaseConnectionRole::JobWorker,
    )
    .expect("reopen external source metadata");
    let availability: String = connection
        .query_row(
            "SELECT availability FROM source_readiness_sources WHERE source_id = ?1",
            [source.id.as_str()],
            |row| row.get(0),
        )
        .expect("read missing source availability");
    assert_eq!(availability, "offline");
}

#[test]
fn scheduled_manifest_audit_does_not_recreate_source_removed_after_discovery() {
    let parent = tempfile::tempdir().expect("missing source parent");
    let root = parent.path().join("source");
    std::fs::create_dir(&root).expect("create source root");
    let source = SampleSource::new_with_id(
        SourceId::from_string("removed-after-discovery"),
        root.clone(),
    );
    source.open_db().expect("create source database");
    let candidate = RuntimeCandidate {
        schedule: WorkCandidate::source(
            source.id.as_str(),
            ProcessingLane::Scan,
            0,
            now_epoch_seconds(),
        ),
        source,
        task: RuntimeTask::ManifestAudit { accelerated: false },
    };
    std::fs::remove_dir_all(&root).expect("remove source after scheduling");

    assert_eq!(
        execute_candidate(
            &candidate,
            0,
            &AtomicBool::new(false),
            &DatabaseWriterGate::default(),
            ContentAuditActivity::default(),
            &mut |_| false,
        )
            .expect("unavailable audit is parked"),
        ExecutionOutcome::Parked
    );
    assert!(
        !should_requeue_cancelled(Some(ExecutionOutcome::Parked), true, false),
        "unavailable roots must wait for a later availability or safety wake"
    );
    assert!(
        !root.exists(),
        "executing stale scheduled work must not recreate the source"
    );
}
