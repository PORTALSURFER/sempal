#[test]
fn fast_readd_waits_for_old_epoch_then_preserves_shared_source_storage() {
    let (_directory, source) = unhashed_source("fast-readd-retains-storage");
    let database_root = source.database_root().expect("database root");
    let mut connection = SourceDatabase::open_connection_with_role_and_database_root(
        &source.root,
        &database_root,
        SourceDatabaseConnectionRole::JobWorker,
    )
    .expect("open source database");
    publish_current_readiness_targets(&mut connection, source.id.as_str(), 1)
        .expect("publish readiness targets");
    drop(connection);

    let mut supervisor = SourceProcessingSupervisor::dormant();
    supervisor
        .replace_sources(vec![source.clone()])
        .expect("configure source");
    let old_cancel =
        Arc::clone(&supervisor.shared.control().source_work_cancels[source.id.as_str()]);
    let old_work = supervisor
        .shared
        .begin_in_flight_work(source.id.as_str(), &old_cancel)
        .expect("register old epoch work");

    supervisor
        .replace_sources(Vec::new())
        .expect("remove source immediately");
    supervisor
        .replace_sources(vec![source.clone()])
        .expect("re-add source immediately");
    for retirement in supervisor.shared.control().pending_retirements.values_mut() {
        retirement.retry_at = now_epoch_seconds().saturating_add(60);
    }
    let waiting_handle = supervisor.budget_handle();
    let waiting_source_id = source.id.as_str().to_string();
    let (waiting_sender, waiting_receiver) = std::sync::mpsc::channel();
    let waiting = thread::spawn(move || {
        let permit = waiting_handle.acquire_scan(&waiting_source_id);
        waiting_sender
            .send(permit)
            .expect("report re-added source admission");
    });
    assert!(
        waiting_receiver
            .recv_timeout(Duration::from_millis(50))
            .is_err(),
        "same-storage admission must wait while the retired epoch is still active"
    );
    process_ready_source_retirements(&supervisor.shared);
    assert_eq!(supervisor.shared.control().pending_retirements.len(), 1);

    drop(old_work);
    process_ready_source_retirements(&supervisor.shared);
    assert!(supervisor.shared.control().pending_retirements.is_empty());
    let permit = waiting_receiver
        .recv_timeout(Duration::from_secs(1))
        .expect("re-added source admission should resume after the old epoch drains")
        .expect("re-added source receives its current-generation permit");
    assert_eq!(
        permit.lifecycle_generation(),
        supervisor.lifecycle_generations()[source.id.as_str()]
    );
    drop(permit);
    waiting.join().expect("join re-added source admission");
    let availability: String = SourceDatabase::open_connection_with_role_and_database_root(
        &source.root,
        &database_root,
        SourceDatabaseConnectionRole::JobWorker,
    )
    .expect("reopen retained source database")
    .query_row(
        "SELECT availability FROM source_readiness_sources WHERE source_id = ?1",
        [source.id.as_str()],
        |row| row.get(0),
    )
    .expect("read retained source readiness");
    assert_eq!(availability, "active");
    assert_eq!(supervisor.shutdown()["joined"], true);
}

#[test]
fn startup_recovery_enqueues_retained_sources_missing_from_configuration() {
    let config_base = tempfile::tempdir().expect("config base");
    let _guard = wavecrate::app_dirs::ConfigBaseGuard::set(config_base.path().to_path_buf());
    let source_root = tempfile::tempdir().expect("retained source root");
    let source = SampleSource::new_with_id(
        SourceId::from_string("startup-retirement"),
        source_root.path().to_path_buf(),
    );
    wavecrate::sample_sources::library::save(&wavecrate::sample_sources::library::LibraryState {
        sources: vec![source.clone()],
    })
    .expect("remember retained source");
    wavecrate::sample_sources::library::save(
        &wavecrate::sample_sources::library::LibraryState::default(),
    )
    .expect("remove active configuration while retaining descriptor");

    let mut next_generation = 1;
    let (pending, next_retirement_id) =
        recovered_source_retirements(&BTreeMap::new(), &mut next_generation)
            .expect("recover inactive retained source");

    assert_eq!(pending.len(), 1);
    assert_eq!(pending[&1].source.id, source.id);
    assert_eq!(next_retirement_id, 2);
    assert_eq!(next_generation, 2);
}

#[test]
fn discovery_progress_converged_safety_probe_is_a_silent_noop() {
    let (_directory, source) = unhashed_source("revision-gated-safety-probe");
    let database_root = source.database_root().expect("database root");
    let mut connection = SourceDatabase::open_connection_with_role_and_database_root(
        &source.root,
        &database_root,
        SourceDatabaseConnectionRole::JobWorker,
    )
    .expect("open source database");
    publish_current_readiness_targets(&mut connection, source.id.as_str(), 100)
        .expect("publish initial readiness");
    let metadata = std::fs::metadata(&source.root).expect("source root metadata");
    let root_identity = wavecrate_library::filesystem_identity::stable_filesystem_identity(
        &source.root,
        &metadata,
    )
    .expect("stable source root identity");
    connection
        .execute(
            "INSERT INTO metadata (key, value) VALUES (?1, ?2)",
            rusqlite::params![
                wavecrate_library::sample_sources::db::META_SOURCE_WATCHER_CHECKPOINT,
                serde_json::json!({ "root_identity": root_identity, "event_id": 1_u64 }).to_string(),
            ],
        )
        .expect("persist durable watcher coverage");
    let durable_before = discovery_durable_counts(&connection);
    let source_before = ReadinessStore::new(&mut connection)
        .source_state(source.id.as_str())
        .expect("read source state")
        .expect("source state exists");
    drop(connection);
    let cancel = AtomicBool::new(false);

    for interval in 0..10 {
        let Cancellable::Completed((candidates, stats, _health)) =
            discover_source_candidates_with_progress(
            &source,
            101 + interval,
            false,
            false,
            None,
            true,
            &cancel,
            &mut |_| panic!("cheap safety probe must not materialize target work"),
        )
        .expect("run revision-gated safety probe") else {
            panic!("safety probe unexpectedly cancelled");
        };
        assert!(candidates.is_empty());
        assert!(stats.cheap_noop_sweep);
    }

    let mut connection = SourceDatabase::open_connection_with_role_and_database_root(
        &source.root,
        &database_root,
        SourceDatabaseConnectionRole::JobWorker,
    )
    .expect("reopen source database");
    assert_eq!(discovery_durable_counts(&connection), durable_before);
    assert_eq!(
        ReadinessStore::new(&mut connection)
            .source_state(source.id.as_str())
            .expect("read final source state")
            .expect("source state exists"),
        source_before
    );
}

#[test]
fn safety_probe_recovers_durable_retry_and_lease_deadlines_after_restart() {
    let (_directory, source) = unhashed_source("durable-readiness-deadlines");
    let database_root = source.database_root().expect("database root");
    let mut connection = SourceDatabase::open_connection_with_role_and_database_root(
        &source.root,
        &database_root,
        SourceDatabaseConnectionRole::JobWorker,
    )
    .expect("open source database");
    publish_current_readiness_targets(&mut connection, source.id.as_str(), 100)
        .expect("publish readiness targets");
    let snapshot = reconcile_readiness(&connection, source.id.as_str(), 100)
        .expect("reconcile readiness targets");
    let target = snapshot
        .entries
        .iter()
        .find(|entry| entry.target.stage == ReadinessStage::IndexedIdentity)
        .expect("indexed identity target")
        .target
        .clone();
    persist_readiness_deficits(&mut connection, &snapshot.deficits, 100)
        .expect("persist readiness work");
    connection
        .execute(
            "UPDATE analysis_jobs
             SET status = 'done', running_at = NULL, retry_at = NULL,
                 failure_kind = NULL, failure_code = NULL, lease_expires_at = NULL
             WHERE readiness_managed = 1",
            [],
        )
        .expect("settle unrelated readiness work");
    connection
        .execute(
            "UPDATE analysis_jobs
             SET status = 'pending'
             WHERE readiness_managed = 1
               AND readiness_scope_id = ?1
               AND readiness_stage = 'indexed_identity'",
            [target.scope_id.as_str()],
        )
        .expect("retain one readiness job");
    connection
        .execute(
            "INSERT INTO metadata (key, value) VALUES (?1, '100')
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [META_LAST_MANIFEST_AUDIT_AT],
        )
        .expect("persist recent manifest audit");
    #[cfg(target_os = "macos")]
    {
        let metadata = std::fs::metadata(&source.root).expect("source root metadata");
        let root_identity =
            wavecrate_library::filesystem_identity::stable_filesystem_identity(&source.root, &metadata)
                .expect("stable source root identity");
        connection
            .execute(
                "INSERT INTO metadata (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                rusqlite::params![
                    wavecrate_library::sample_sources::db::META_SOURCE_WATCHER_CHECKPOINT,
                    serde_json::json!({ "root_identity": root_identity, "event_id": 1_u64 })
                        .to_string(),
                ],
            )
            .expect("persist durable watcher coverage");
    }

    let claim = claim_readiness_target(&mut connection, &target, 100, 50)
        .expect("claim readiness target")
        .expect("target is claimable");
    drop(connection);

    let Cancellable::Completed((candidates, stats, _health)) =
        discover_source_candidates_with_progress(
            &source,
            101,
            false,
            false,
            None,
            true,
            &AtomicBool::new(false),
            &mut |_| panic!("future deadline probe must not materialize targets"),
        )
        .expect("probe future lease deadline")
    else {
        panic!("future deadline probe unexpectedly cancelled");
    };
    assert!(candidates.is_empty());
    assert!(!stats.cheap_noop_sweep);
    assert_eq!(stats.earliest_retry_at, Some(150));

    let connection = SourceDatabase::open_connection_with_role_and_database_root(
        &source.root,
        &database_root,
        SourceDatabaseConnectionRole::JobWorker,
    )
    .expect("reopen source database");
    let claim_generation_before = claim.claim_generation;
    connection
        .execute(
            "UPDATE analysis_jobs SET lease_expires_at = 99
             WHERE readiness_managed = 1 AND readiness_claim_generation = ?1",
            [claim_generation_before],
        )
        .expect("expire readiness lease");
    drop(connection);

    let Cancellable::Completed((candidates, stats, _health)) =
        discover_source_candidates_with_progress(
            &source,
            101,
            false,
            false,
            None,
            true,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .expect("probe expired lease")
    else {
        panic!("expired lease probe unexpectedly cancelled");
    };
    assert!(!stats.cheap_noop_sweep);
    assert!(!candidates.is_empty(), "expired lease must become candidate work");

    let connection = SourceDatabase::open_connection_with_role_and_database_root(
        &source.root,
        &database_root,
        SourceDatabaseConnectionRole::JobWorker,
    )
    .expect("reopen source database for null lease");
    connection
        .execute(
            "UPDATE analysis_jobs
             SET status = 'running', lease_expires_at = NULL
             WHERE readiness_managed = 1",
            [],
        )
        .expect("simulate compatibility-null running lease");
    drop(connection);

    let Cancellable::Completed((candidates, stats, _health)) =
        discover_source_candidates_with_progress(
            &source,
            101,
            false,
            false,
            None,
            true,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .expect("probe compatibility-null lease")
    else {
        panic!("compatibility-null lease probe unexpectedly cancelled");
    };
    assert!(!stats.cheap_noop_sweep);
    assert!(
        !candidates.is_empty(),
        "compatibility-null lease must become candidate work"
    );

    let connection = SourceDatabase::open_connection_with_role_and_database_root(
        &source.root,
        &database_root,
        SourceDatabaseConnectionRole::JobWorker,
    )
    .expect("reopen source database for retry");
    connection
        .execute(
            "UPDATE analysis_jobs
             SET status = 'failed', failure_kind = NULL, failure_code = 'test_retry',
                 retry_at = 250, lease_expires_at = NULL
             WHERE readiness_managed = 1",
            [],
        )
        .expect("park retryable readiness work");
    drop(connection);

    let Cancellable::Completed((candidates, stats, _health)) =
        discover_source_candidates_with_progress(
            &source,
            101,
            false,
            false,
            None,
            true,
            &AtomicBool::new(false),
            &mut |_| panic!("future retry probe must not materialize targets"),
        )
        .expect("probe future retry deadline")
    else {
        panic!("future retry probe unexpectedly cancelled");
    };
    assert!(candidates.is_empty());
    assert!(!stats.cheap_noop_sweep);
    assert_eq!(stats.earliest_retry_at, Some(250));

    let connection = SourceDatabase::open_connection_with_role_and_database_root(
        &source.root,
        &database_root,
        SourceDatabaseConnectionRole::JobWorker,
    )
    .expect("reopen source database for due retry");
    connection
        .execute(
            "UPDATE analysis_jobs SET retry_at = NULL WHERE readiness_managed = 1",
            [],
        )
        .expect("make retry compatibility-null and due");
    drop(connection);

    let Cancellable::Completed((candidates, stats, _health)) =
        discover_source_candidates_with_progress(
            &source,
            101,
            false,
            false,
            None,
            true,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .expect("probe due retry deadline")
    else {
        panic!("due retry probe unexpectedly cancelled");
    };
    assert!(!stats.cheap_noop_sweep);
    assert!(!candidates.is_empty(), "due retry must become candidate work");
}

#[test]
fn macos_safety_probe_requires_durable_watcher_coverage() {
    let (_directory, source) = unhashed_source(&format!(
        "missing-watcher-coverage-{}",
        uuid::Uuid::new_v4()
    ));
    let database_root = source.database_root().expect("database root");
    let mut connection = SourceDatabase::open_connection_with_role_and_database_root(
        &source.root,
        &database_root,
        SourceDatabaseConnectionRole::JobWorker,
    )
    .expect("open source database");
    publish_current_readiness_targets(&mut connection, source.id.as_str(), 100)
        .expect("publish initial readiness");
    drop(connection);

    let Cancellable::Completed((candidates, stats, _health)) =
        discover_source_candidates_with_progress(
            &source,
            101,
            false,
            false,
            None,
            true,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .expect("run safety probe")
    else {
        panic!("safety probe unexpectedly cancelled");
    };

    #[cfg(target_os = "macos")]
    assert!(
        candidates
            .iter()
            .any(|candidate| matches!(candidate.task, RuntimeTask::ManifestAudit { .. })),
        "a macOS source without a durable watcher checkpoint must remain audit-eligible"
    );
    #[cfg(not(target_os = "macos"))]
    assert!(candidates.is_empty());
    assert!(!stats.cheap_noop_sweep);
}

#[test]
fn safety_probe_recovers_manifest_commit_without_delta_publication() {
    let (_directory, source) = unhashed_source("revision-gap-recovery");
    let database_root = source.database_root().expect("database root");
    let mut connection = SourceDatabase::open_connection_with_role_and_database_root(
        &source.root,
        &database_root,
        SourceDatabaseConnectionRole::JobWorker,
    )
    .expect("open source database");
    publish_current_readiness_targets(&mut connection, source.id.as_str(), 100)
        .expect("publish initial readiness");
    let previous_generation = ReadinessStore::new(&mut connection)
        .source_state(source.id.as_str())
        .expect("read source state")
        .expect("source state exists")
        .source_generation;
    connection
        .execute(
            "UPDATE wav_files SET content_hash = 'changed-after-crash'
                 WHERE path = 'pending.wav'",
            [],
        )
        .expect("commit manifest content change");
    connection
        .execute(
            "INSERT INTO metadata (key, value) VALUES (?1, '1')
                 ON CONFLICT(key) DO UPDATE
                 SET value = CAST(CAST(value AS INTEGER) + 1 AS TEXT)",
            [META_WAV_PATHS_REVISION],
        )
        .expect("advance manifest generation");

    let Cancellable::Completed((_candidates, stats, _health)) =
        discover_source_candidates_with_connection_and_progress(
            &source,
            &mut connection,
            101,
            false,
            false,
            None,
            true,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .expect("recover readiness publication")
    else {
        panic!("recovery unexpectedly cancelled");
    };
    assert!(!stats.cheap_noop_sweep);
    let recovered = ReadinessStore::new(&mut connection)
        .source_state(source.id.as_str())
        .expect("read recovered source state")
        .expect("source state exists");
    assert_eq!(
        recovered.source_generation,
        previous_generation.saturating_add(1)
    );
}

#[test]
fn discovery_progress_committed_delta_uses_changed_target_counts() {
    let (_directory, source) = unhashed_source("one-file-readiness-delta");
    let database_root = source.database_root().expect("database root");
    let mut connection = SourceDatabase::open_connection_with_role_and_database_root(
        &source.root,
        &database_root,
        SourceDatabaseConnectionRole::JobWorker,
    )
    .expect("open source database");
    publish_current_readiness_targets(&mut connection, source.id.as_str(), 100)
        .expect("publish initial readiness");
    let identity: String = connection
        .query_row(
            "SELECT file_identity FROM wav_files WHERE path = 'pending.wav'",
            [],
            |row| row.get(0),
        )
        .expect("read file identity");
    let state_before = ReadinessStore::new(&mut connection)
        .source_state(source.id.as_str())
        .expect("read source state")
        .expect("source state exists");
    connection
        .execute(
            "UPDATE wav_files
                 SET content_hash = 'one-file-new-hash',
                     file_size = file_size + 1,
                     modified_ns = modified_ns + 1
                 WHERE path = 'pending.wav'",
            [],
        )
        .expect("commit one-file manifest change");
    connection
        .execute(
            "INSERT INTO metadata (key, value) VALUES (?1, '1')
                 ON CONFLICT(key) DO UPDATE
                 SET value = CAST(CAST(value AS INTEGER) + 1 AS TEXT)",
            [META_WAV_PATHS_REVISION],
        )
        .expect("advance manifest generation");
    let delta = PendingReadinessDelta {
        scope_ids: [identity.clone()].into_iter().collect(),
        state_machine_inputs: Default::default(),
    };

    let mut progress_updates = Vec::new();
    let Cancellable::Completed((_candidates, stats, _health)) =
        discover_source_candidates_with_connection_and_progress(
            &source,
            &mut connection,
            101,
            false,
            false,
            Some(&delta),
            false,
            &AtomicBool::new(false),
            &mut |update| progress_updates.push(update),
        )
        .expect("reconcile committed delta")
    else {
        panic!("delta reconciliation unexpectedly cancelled");
    };
    assert!(stats.delta_reconciled);
    assert!(
        progress_updates.iter().any(|update| {
            update.phase == SourceDiscoveryPhase::ComparingChangedReadiness
                && update.completed == 4
                && update.total == 4
        }),
        "the changed file's three targets plus the source target must provide the denominator"
    );
    assert!(
        !progress_updates
            .iter()
            .any(|update| update.phase == SourceDiscoveryPhase::InspectingManifest),
        "a committed delta must not claim a complete manifest inspection"
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM source_readiness_targets
                     WHERE source_id = ?1 AND scope_kind = 'file'",
                [source.id.as_str()],
                |row| row.get::<_, i64>(0),
            )
            .expect("count file targets"),
        3
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM source_readiness_targets
                     WHERE source_id = ?1
                       AND scope_id = ?2
                       AND content_generation = 'one-file-new-hash'",
                params![source.id.as_str(), identity],
                |row| row.get::<_, i64>(0),
            )
            .expect("count changed identity targets"),
        3
    );
    let state_after = ReadinessStore::new(&mut connection)
        .source_state(source.id.as_str())
        .expect("read source state")
        .expect("source state exists");
    assert_eq!(
        state_after.source_generation,
        state_before.source_generation.saturating_add(1)
    );
    assert_eq!(
        state_after.readiness_revision,
        state_before.readiness_revision.saturating_add(1)
    );
}
