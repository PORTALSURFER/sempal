#[cfg(unix)]
#[test]
fn unavailable_hash_path_backs_off_without_starving_later_files() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().expect("temporary hash source");
    let unavailable_path = directory.path().join("a-unavailable.wav");
    let good_path = directory.path().join("z-good.wav");
    std::fs::write(&unavailable_path, [3_u8; 64]).expect("write temporarily unavailable sample");
    std::fs::write(&good_path, [7_u8; 64]).expect("write hashable sample");
    let source = SampleSource::new_with_id(
        SourceId::from_string("hash-fairness"),
        directory.path().to_path_buf(),
    );
    let db = source.open_db().expect("open hash source database");
    db.upsert_file(Path::new("a-unavailable.wav"), 64, 1)
        .expect("insert unavailable hash row");
    db.upsert_file(Path::new("z-good.wav"), 64, 1)
        .expect("insert good hash row");
    let database_root = source.database_root().expect("database root");
    let mut connection = SourceDatabase::open_connection_with_role_and_database_root(
        &source.root,
        &database_root,
        SourceDatabaseConnectionRole::JobWorker,
    )
    .expect("open hash database");
    connection
        .execute_batch(
            "UPDATE wav_files SET file_identity = 'identity-bad'
                 WHERE path = 'a-unavailable.wav';
                 UPDATE wav_files SET file_identity = 'identity-good'
                 WHERE path = 'z-good.wav';",
        )
        .expect("assign hash identities");
    let now = now_epoch_seconds();
    assert!(publish_current_readiness_targets(&mut connection, source.id.as_str(), now)
        .expect("publish readiness targets"));
    let snapshot = reconcile_readiness(&connection, source.id.as_str(), now)
        .expect("reconcile readiness targets");
    persist_readiness_deficits(&mut connection, &snapshot.deficits, now)
        .expect("persist readiness work");
    let targets = snapshot
        .entries
        .iter()
        .filter(|entry| entry.target.stage == ReadinessStage::IndexedIdentity)
        .map(|entry| (entry.target.relative_path.clone(), entry.target.clone()))
        .collect::<Vec<_>>();
    let unavailable_target = targets
        .iter()
        .find(|(path, _)| path.as_deref() == Some("a-unavailable.wav"))
        .expect("unavailable readiness target")
        .1
        .clone();
    let good_target = targets
        .iter()
        .find(|(path, _)| path.as_deref() == Some("z-good.wav"))
        .expect("healthy readiness target")
        .1
        .clone();
    drop(connection);

    std::fs::set_permissions(&unavailable_path, std::fs::Permissions::from_mode(0o000))
        .expect("make deferred hash path unavailable");

    let writer = DatabaseWriterGate::default();
    let unavailable_outcome = execute_readiness_target(
        &source,
        &unavailable_target,
        &AtomicBool::new(false),
        &writer,
    )
    .expect("execute unavailable readiness target");
    assert!(matches!(
        unavailable_outcome,
        ExecutionOutcome::Retried { .. }
    ));

    let good_outcome = execute_readiness_target(
        &source,
        &good_target,
        &AtomicBool::new(false),
        &writer,
    )
    .expect("execute healthy readiness target");
    assert!(!matches!(good_outcome, ExecutionOutcome::Failed));
    assert!(source
        .open_db()
        .expect("reopen hash source")
        .entry_for_path(Path::new("z-good.wav"))
        .expect("read good hash row")
        .and_then(|entry| entry.content_hash)
        .is_some());

    let connection = SourceDatabase::open_connection_with_role_and_database_root(
        &source.root,
        &database_root,
        SourceDatabaseConnectionRole::JobWorker,
    )
    .expect("reopen hash database");
    let failure: (String, Option<String>, Option<String>, Option<i64>, i64) = connection
        .query_row(
            "SELECT status, failure_kind, failure_code, retry_at, attempts
                 FROM analysis_jobs
                 WHERE readiness_managed = 1
                   AND readiness_stage = 'indexed_identity'
                   AND relative_path = 'a-unavailable.wav'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )
        .expect("read durable hash failure");
    assert_eq!(failure.0, "failed");
    assert_eq!(failure.1.as_deref(), Some("retryable"));
    assert_eq!(failure.2.as_deref(), Some("scanner_io"));
    assert!(failure.3.is_some());
    assert_eq!(failure.4, 1);

    std::fs::set_permissions(&unavailable_path, std::fs::Permissions::from_mode(0o644))
        .expect("restore deferred hash path");
    connection
        .execute(
            "UPDATE analysis_jobs SET retry_at = ?1
             WHERE readiness_managed = 1
               AND readiness_stage = 'indexed_identity'
               AND relative_path = 'a-unavailable.wav'",
            [now_epoch_seconds().saturating_sub(1)],
        )
        .expect("make retry due");
    drop(connection);

    let restored_outcome = execute_readiness_target(
        &source,
        &unavailable_target,
        &AtomicBool::new(false),
        &writer,
    )
    .expect("retry restored readiness target");
    assert!(!matches!(restored_outcome, ExecutionOutcome::Failed));
    assert!(source
        .open_db()
        .expect("reopen restored hash source")
        .entry_for_path(Path::new("a-unavailable.wav"))
        .expect("read restored hash row")
        .and_then(|entry| entry.content_hash)
        .is_some());
}

#[test]
fn cancelled_readiness_claim_returns_without_blocking_publication() {
    let (_directory, source) = unhashed_source("cancelled-readiness-claim");
    let database_root = source.database_root().expect("database root");
    let mut connection = SourceDatabase::open_connection_with_role_and_database_root(
        &source.root,
        &database_root,
        SourceDatabaseConnectionRole::JobWorker,
    )
    .expect("open readiness database");
    let now = now_epoch_seconds();
    assert!(publish_current_readiness_targets(&mut connection, source.id.as_str(), now)
        .expect("publish readiness targets"));
    let snapshot = reconcile_readiness(&connection, source.id.as_str(), now)
        .expect("reconcile readiness targets");
    persist_readiness_deficits(&mut connection, &snapshot.deficits, now)
        .expect("persist readiness work");
    let target = snapshot
        .entries
        .iter()
        .find(|entry| entry.target.stage == ReadinessStage::IndexedIdentity)
        .expect("indexed identity target")
        .target
        .clone();
    drop(connection);

    let outcome = execute_readiness_target(
        &source,
        &target,
        &AtomicBool::new(true),
        &DatabaseWriterGate::default(),
    )
    .expect("cancel readiness target");
    assert_eq!(outcome, ExecutionOutcome::Cancelled);

    let connection = SourceDatabase::open_connection_with_role_and_database_root(
        &source.root,
        &database_root,
        SourceDatabaseConnectionRole::JobWorker,
    )
    .expect("reopen readiness database");
    let state: (String, Option<String>, Option<String>) = connection
        .query_row(
            "SELECT status, failure_kind, failure_code
             FROM analysis_jobs
             WHERE readiness_managed = 1
               AND readiness_stage = 'indexed_identity'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("read cancelled readiness work");
    assert_eq!(state, ("pending".to_string(), Some("cancelled".to_string()), Some("cancelled".to_string())));
}

#[test]
fn legacy_source_schema_is_not_eligible_for_automatic_processing() {
    let mut connection = rusqlite::Connection::open_in_memory().unwrap();
    connection
        .execute_batch(
            "CREATE TABLE wav_files (
                    path TEXT PRIMARY KEY,
                    file_size INTEGER NOT NULL,
                    modified_ns INTEGER NOT NULL
                 );
                 CREATE TABLE analysis_jobs (
                    id INTEGER PRIMARY KEY,
                    relative_path TEXT NOT NULL,
                    job_type TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    status TEXT NOT NULL
                 );
                 CREATE TABLE metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )
        .unwrap();

    assert!(!source_processing_schema_available(&mut connection).unwrap());
}

#[test]
fn real_hash_execution_waits_for_shared_scan_database_budget() {
    let (_directory, source) = unhashed_source("shared-budget");
    let (sender, receiver) = std::sync::mpsc::channel();
    let shared = Arc::new(Shared::new(vec![source.clone()], Some(Arc::new(sender))));
    let permit = SourceProcessingBudgetHandle {
        shared: Arc::clone(&shared),
    }
    .acquire_scan(source.id.as_str())
    .expect("acquire external scan budget");
    let coordinator_shared = Arc::clone(&shared);
    let coordinator = thread::Builder::new()
        .name(String::from("wavecrate-source-supervisor-test"))
        .spawn(move || run_coordinator(coordinator_shared))
        .expect("spawn source processing supervisor");
    let mut supervisor = SourceProcessingSupervisor {
        shared,
        coordinator: Some(coordinator),
        retirement_worker: None,
    };
    thread::sleep(Duration::from_millis(150));
    assert!(!source_is_hashed(&source));
    assert!(
        supervisor.shared.telemetry().sweeps <= 2,
        "a source blocked by an external scan must park instead of rediscovering in a tight loop"
    );
    assert!(
        receiver
            .try_iter()
            .all(|event| matches!(event, SourceProcessingEvent::Completed)),
        "queued work must not publish active progress while foreground admission owns the lane"
    );

    drop(permit);
    wait_until(Duration::from_secs(3), || source_is_hashed(&source));
    assert_eq!(supervisor.shutdown()["joined"], true);
}

#[test]
fn external_scan_tokens_survive_playback_but_cancel_for_removal_and_shutdown() {
    let (_directory, source) = unhashed_source("external-cancel");
    let mut supervisor = SourceProcessingSupervisor::dormant();
    supervisor
        .replace_sources(vec![source.clone()])
        .expect("configure source");

    let playback_permit = supervisor
        .budget_handle()
        .acquire_scan(source.id.as_str())
        .expect("acquire playback permit");
    let playback_cancel = playback_permit.cancel_token();
    assert!(!playback_cancel.load(Ordering::Acquire));
    supervisor.set_playback_active(true);
    assert!(
        !playback_cancel.load(Ordering::Acquire),
        "playback must not cancel source scanning"
    );
    drop(playback_permit);

    supervisor.set_playback_active(false);
    let removal_permit = supervisor
        .budget_handle()
        .acquire_scan(source.id.as_str())
        .expect("acquire removal permit");
    let removal_cancel = removal_permit.cancel_token();
    let removal_cancel_for_releaser = Arc::clone(&removal_cancel);
    let releaser = thread::spawn(move || {
        while !removal_cancel_for_releaser.load(Ordering::Acquire) {
            thread::yield_now();
        }
        drop(removal_permit);
    });
    supervisor
        .replace_sources(Vec::new())
        .expect("remove configured source");
    releaser.join().expect("join removal scan releaser");
    assert!(removal_cancel.load(Ordering::Acquire));

    supervisor
        .replace_sources(vec![source.clone()])
        .expect("restore configured source");
    process_ready_source_retirements(&supervisor.shared);
    let shutdown_permit = supervisor
        .budget_handle()
        .acquire_scan(source.id.as_str())
        .expect("acquire shutdown permit");
    let shutdown_cancel = shutdown_permit.cancel_token();
    let release_cancel = Arc::clone(&shutdown_cancel);
    let releaser = thread::spawn(move || {
        while !release_cancel.load(Ordering::Acquire) {
            thread::yield_now();
        }
        drop(shutdown_permit);
    });
    let wake_generation = supervisor.shared.control().wake_generation;
    let report = supervisor.shutdown();
    releaser.join().expect("join external scan releaser");
    assert_eq!(report["joined"], true);
    assert_eq!(report["external_scans_joined"], true);
    assert!(shutdown_cancel.load(Ordering::Acquire));
    assert!(supervisor.shared.control().wake_generation > wake_generation);
}

#[test]
fn readiness_candidates_preserve_durable_queue_creation_time() {
    let (_directory, source) = unhashed_source("queue-age");
    let database_root = source.database_root().expect("database root");
    let mut connection = SourceDatabase::open_connection_with_role_and_database_root(
        &source.root,
        &database_root,
        SourceDatabaseConnectionRole::JobWorker,
    )
    .expect("open readiness database");
    connection
        .execute(
            "UPDATE wav_files
                 SET file_identity = 'queue-identity', content_hash = 'queue-content'
                 WHERE path = 'pending.wav'",
            [],
        )
        .expect("assign queue identity");
    assert!(
        publish_current_readiness_targets(&mut connection, source.id.as_str(), 100)
            .expect("publish current targets")
    );
    let snapshot =
        reconcile_readiness(&connection, source.id.as_str(), 100).expect("reconcile targets");
    persist_readiness_deficits(&mut connection, &snapshot.deficits, 100).expect("persist deficits");
    drop(connection);

    let cancel = AtomicBool::new(false);
    let Cancellable::Completed((candidates, _)) =
        discover_source_candidates(&source, 250, false, &cancel).expect("rediscover work")
    else {
        panic!("source rediscovery unexpectedly cancelled");
    };
    let readiness = candidates
        .iter()
        .filter(|candidate| matches!(candidate.task, RuntimeTask::Readiness(_)))
        .collect::<Vec<_>>();
    assert!(!readiness.is_empty());
    assert!(
        readiness
            .iter()
            .all(|candidate| candidate.schedule.enqueued_at == 100)
    );
    assert!(
        readiness.iter().all(|candidate| !matches!(
            candidate.task,
            RuntimeTask::Readiness(ref target)
                if target.stage == ReadinessStage::SimilarityLayout
        )),
        "similarity layout must stay parked behind the pending embedding target"
    );
    assert_eq!(oldest_job_age_seconds(&candidates, 250), 150);
}
