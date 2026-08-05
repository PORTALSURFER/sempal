use super::*;

#[test]
fn liveness_oracle_rejects_actionable_deficits_without_observable_runtime_work() {
    let directory = tempfile::tempdir().expect("liveness oracle source");
    write_test_wav(&directory.path().join("pending.wav"), 0.25);
    let source = SampleSource::new_with_id(
        SourceId::from_string("liveness-oracle"),
        directory.path().to_path_buf(),
    );
    let database = source.open_db().expect("open liveness oracle database");
    audit_source_and_record(&database, None, usize::MAX, now_epoch_seconds())
        .expect("seed liveness oracle manifest");
    let mut connection = open_connection(&source).expect("open liveness oracle connection");
    publish_current_readiness_targets(&mut connection, source.id.as_str(), now_epoch_seconds())
        .expect("publish liveness oracle targets");
    let snapshot = ReadinessStore::new(&mut connection)
        .reconcile(source.id.as_str(), now_epoch_seconds())
        .expect("reconcile liveness oracle");
    assert!(!snapshot.deficits.is_empty());

    let runtime = RuntimeObservation {
        coordinator_running: true,
        source_configured: true,
        source_active: true,
        source_dirty: false,
        source_quarantined: false,
        wake_generation: 1,
        settled_wake_generation: 1,
        wake_reason: "test",
        lifecycle_generation: Some(1),
        in_flight: 0,
        active_budget: false,
        queue_depth: 0,
        readiness_queue_depth: 0,
        retries_due: 0,
        retry_at: None,
        sweeps: 1,
        claimed: 0,
        completed: 0,
        failed: 0,
        retried: 0,
        stale: 0,
        cancelled: 0,
        contention: 0,
        oldest_job_age_seconds: 0,
    };
    assert!(silently_idle(&snapshot, &runtime));

    let retry_due = RuntimeObservation {
        retry_at: Some(now_epoch_seconds()),
        ..runtime
    };
    assert!(
        !silently_idle(&snapshot, &retry_due),
        "a retry due in the current second remains observable until dispatch"
    );

    let scheduled = RuntimeObservation {
        source_dirty: true,
        ..runtime
    };
    assert!(!silently_idle(&snapshot, &scheduled));
}

#[test]
fn unrelated_source_queue_does_not_mask_silent_idle_source() {
    let first_directory = tempfile::tempdir().expect("first liveness source");
    write_test_wav(&first_directory.path().join("pending.wav"), 0.25);
    let first = SampleSource::new_with_id(
        SourceId::from_string("liveness-source-a"),
        first_directory.path().to_path_buf(),
    );
    let database = first.open_db().expect("open first source");
    audit_source_and_record(&database, None, usize::MAX, now_epoch_seconds())
        .expect("seed first manifest");
    let mut connection = open_connection(&first).expect("open first connection");
    publish_current_readiness_targets(&mut connection, first.id.as_str(), now_epoch_seconds())
        .expect("publish first targets");
    let snapshot = ReadinessStore::new(&mut connection)
        .reconcile(first.id.as_str(), now_epoch_seconds())
        .expect("reconcile first source");
    assert!(!snapshot.deficits.is_empty());

    let second_directory = tempfile::tempdir().expect("second liveness source");
    let second = SampleSource::new_with_id(
        SourceId::from_string("liveness-source-b"),
        second_directory.path().to_path_buf(),
    );
    second.open_db().expect("open second source");
    let mut supervisor = SourceProcessingSupervisor::start_with_playback_state(
        vec![first.clone(), second.clone()],
        true,
    );
    {
        let mut telemetry = supervisor.shared.telemetry();
        telemetry.queue_depth = 9;
        telemetry.readiness_queue_depth = 9;
        telemetry
            .queue_depth_by_source
            .insert(second.id.as_str().to_string(), 9);
        telemetry
            .readiness_queue_depth_by_source
            .insert(second.id.as_str().to_string(), 9);
    }

    let observed = runtime_observation(&supervisor, first.id.as_str());
    assert_eq!(observed.queue_depth, 0);
    assert_eq!(observed.readiness_queue_depth, 0);
    let unpaused = RuntimeObservation {
        source_dirty: false,
        settled_wake_generation: observed.wake_generation,
        ..observed
    };
    assert!(
        silently_idle(&snapshot, &unpaused),
        "source B work must not make source A look observably scheduled"
    );
    assert_eq!(supervisor.shutdown()["joined"], true);
}

#[test]
#[ignore = "explicit deterministic end-to-end source-processing liveness lane"]
fn source_processing_liveness_harness_converges_restart_churn_and_root_recovery() {
    let mut harness = LivenessHarness::new("source-processing-liveness");
    let initial = harness.await_fully_ready();
    let initial_generation = initial.source_generation;

    let nested = PathBuf::from("nested/snare.wav");
    fs::create_dir_all(harness.source.root.join("nested")).expect("create nested folder");
    write_test_wav(&harness.source.root.join(&nested), 0.5);
    harness.commit_targeted_paths(vec![nested.clone()], WatcherStimulus::Targeted);
    let after_nested_create = harness.await_fully_ready();
    assert!(after_nested_create.source_generation > initial_generation);

    let kick = harness.source.root.join("kick.wav");
    let original_modified = fs::metadata(&kick)
        .expect("kick metadata")
        .modified()
        .expect("kick modified time");
    write_test_wav(&kick, 1.0);
    let file = fs::OpenOptions::new()
        .write(true)
        .open(&kick)
        .expect("reopen same-size kick");
    file.set_times(fs::FileTimes::new().set_modified(original_modified))
        .expect("restore same-size kick timestamp");
    harness.commit_targeted_paths(vec![PathBuf::from("kick.wav")], WatcherStimulus::Targeted);
    harness.await_fully_ready();

    let moved = PathBuf::from("nested/moved-snare.wav");
    fs::rename(
        harness.source.root.join(&nested),
        harness.source.root.join(&moved),
    )
    .expect("rename nested sample");
    harness.commit_targeted_paths(vec![nested, moved.clone()], WatcherStimulus::Targeted);
    harness.await_fully_ready();

    fs::remove_file(harness.source.root.join(&moved)).expect("delete moved sample");
    harness.commit_targeted_paths(vec![moved], WatcherStimulus::Targeted);
    harness.await_fully_ready();

    for index in 0..4 {
        let relative = PathBuf::from(format!("storm/sample-{index}.wav"));
        fs::create_dir_all(
            harness
                .source
                .root
                .join(relative.parent().expect("storm parent")),
        )
        .expect("create storm folder");
        write_test_wav(&harness.source.root.join(&relative), index as f32 * 0.125);
    }
    harness.commit_overflow_audit(WatcherStimulus::Overflow);
    harness.await_fully_ready();

    harness.supervisor.set_playback_active(true);
    let playback_churn = harness.source.root.join("playback-churn.wav");
    write_test_wav(&playback_churn, 0.75);
    harness.commit_internal_mutation(
        FileMutationOperation::Duplicate,
        vec![FileMutationChange::created(playback_churn)],
    );
    harness.await_fully_ready();
    harness.supervisor.set_playback_active(false);

    let report_before_restart = harness.shutdown();
    assert_eq!(report_before_restart["joined"], true);
    let kick_before_restart = fs::read(&kick).expect("read kick before closed-app change");
    write_test_wav(&kick, 1.5);
    assert_eq!(
        fs::metadata(&kick).expect("changed kick metadata").len(),
        kick_before_restart.len() as u64,
        "closed-app mutation should preserve file size"
    );
    let database = harness
        .source
        .open_db()
        .expect("open source before restart");
    harness.expected_source_generation = database
        .get_wav_paths_revision()
        .expect("read pre-restart source generation")
        .saturating_add(1) as i64;
    database
        .set_metadata(META_LAST_MANIFEST_AUDIT_AT, "0")
        .expect("make closed-app audit due");
    harness.restart_runtime();
    harness.watcher_stimulus = WatcherStimulus::ClosedAppAudit;
    harness.await_fully_ready();

    let offline_root = harness.source_parent.path().join("source-offline");
    fs::rename(&harness.source.root, &offline_root).expect("remove source root");
    harness.force_root_refresh(WatcherStimulus::RootUnavailable);
    harness.await_availability(SourceAvailability::Offline);

    fs::rename(&offline_root, &harness.source.root).expect("restore source root");
    harness.force_root_refresh(WatcherStimulus::RootAvailable);
    harness.await_fully_ready();

    let retired_root = harness
        .source_parent
        .path()
        .join("source-retired-after-identity-replacement");
    fs::rename(&harness.source.root, &retired_root).expect("retire live source root");
    fs::create_dir(&harness.source.root).expect("create same-path replacement source root");
    write_test_wav(&harness.source.root.join("replacement.wav"), 1.75);
    harness
        .source
        .open_db()
        .expect("create replacement source database");
    harness.force_root_refresh(WatcherStimulus::RootIdentityReplacement);
    harness.await_fully_ready();

    harness
        .watcher
        .as_ref()
        .expect("active watcher")
        .replace_sources(Vec::new());
    harness
        .supervisor
        .replace_sources(Vec::new())
        .expect("remove source during liveness lane");
    harness
        .supervisor
        .replace_sources(vec![harness.source.clone()])
        .expect("re-add retained source");
    harness
        .watcher
        .as_ref()
        .expect("active watcher")
        .replace_sources(vec![SourceProcessingRegistration::new(
            harness.source.clone(),
            1,
        )]);
    harness.force_watcher_restart();
    harness.await_fully_ready();

    let report = harness.shutdown();
    assert_eq!(report["joined"], true);
    assert!(report["completed"].as_u64().unwrap_or_default() > 0);
    assert_eq!(report["queue_depth"], 0);
    assert_eq!(report["readiness_queue_depth"], 0);
}

#[test]
#[ignore = "explicit calibrated large-library source-processing profile"]
fn profile_source_processing_churn_under_playback_and_browser_priority() {
    const FILE_COUNT: usize = 10_000;
    const DISCOVERY_BUDGET: Duration = Duration::from_secs(180);
    const DRAIN_BUDGET: Duration = Duration::from_secs(900);
    const MIN_MATERIALIZATION_THROUGHPUT: f64 = 200.0;
    const MIN_DRAIN_THROUGHPUT: f64 = 40.0;
    const PRIORITY_P99_BUDGET: Duration = Duration::from_millis(10);
    const MEMORY_GROWTH_BUDGET_BYTES: u64 = 512 * 1024 * 1024;
    const CPU_CORE_EQUIVALENT_BUDGET: f64 = 2.0;
    const DISK_IO_BUDGET_BYTES: u64 = 1024 * 1024 * 1024;

    let directory = tempfile::tempdir().expect("large liveness profile source");
    let source = SampleSource::new_with_id(
        SourceId::from_string("source-processing-profile"),
        directory.path().to_path_buf(),
    );
    source.open_db().expect("create profile source database");
    let mut connection = open_connection(&source).expect("open profile database");
    seed_profile_manifest(&mut connection, FILE_COUNT);
    drop(connection);

    let resources_before = process_resource_snapshot();
    let expected_claims = (FILE_COUNT * 3 + 1) as u64;
    let discovery_started = Instant::now();
    let mut supervisor =
        SourceProcessingSupervisor::start_synthetic_profile(vec![source.clone()], true);
    let mut priority_samples = Vec::with_capacity(256);
    for index in 0..256 {
        let started = Instant::now();
        supervisor.set_selected_source(Some(source.id.as_str()));
        supervisor.set_current_folder(source.id.as_str(), "profile");
        supervisor.prioritize_path(
            source.id.as_str(),
            &format!("profile/sample-{:05}.wav", index % FILE_COUNT),
            index % 2 == 0,
        );
        priority_samples.push(started.elapsed());
    }
    priority_samples.sort_unstable();
    let priority_p99 = priority_samples[priority_samples.len() * 99 / 100];
    assert!(priority_p99 <= PRIORITY_P99_BUDGET);

    loop {
        if supervisor.shared.telemetry().claimed > 0 {
            break;
        }
        assert!(
            discovery_started.elapsed() <= DISCOVERY_BUDGET,
            "10k supervisor profile did not discover and claim its first target before budget"
        );
        thread::sleep(Duration::from_millis(20));
    }
    let discovery_elapsed = discovery_started.elapsed();
    let materialization_throughput =
        expected_claims as f64 / discovery_elapsed.as_secs_f64().max(f64::EPSILON);
    assert!(materialization_throughput >= MIN_MATERIALIZATION_THROUGHPUT);

    let drain_started = Instant::now();
    loop {
        let telemetry = supervisor.shared.telemetry();
        let drained = telemetry.claimed == expected_claims
            && telemetry.completed == expected_claims
            && telemetry.queue_depth == 0;
        drop(telemetry);
        let control = supervisor.shared.control();
        let settled = control.dirty_sources.is_empty();
        drop(control);
        if drained && settled {
            break;
        }
        if drain_started.elapsed() > DRAIN_BUDGET {
            let (
                claimed,
                completed,
                failed,
                retried,
                stale,
                cancelled,
                queue_depth,
                readiness_queue_depth,
            ) = {
                let telemetry = supervisor.shared.telemetry();
                (
                    telemetry.claimed,
                    telemetry.completed,
                    telemetry.failed,
                    telemetry.retried,
                    telemetry.stale,
                    telemetry.cancelled,
                    telemetry.queue_depth,
                    telemetry.readiness_queue_depth,
                )
            };
            let control = supervisor.shared.control();
            let dirty_sources = control.dirty_sources.clone();
            let wake_reason = control.wake_reason;
            drop(control);
            let snapshot = readiness_snapshot(&source).expect("timed-out readiness snapshot");
            panic!(
                "10k supervisor profile did not drain {expected_claims} exact targets before \
                 budget: claimed={} completed={} failed={} retried={} stale={} cancelled={} \
                 queue_depth={} readiness_queue_depth={} dirty_sources={dirty_sources:?} \
                 wake_reason={wake_reason} readiness_entries={} deficits={} activity={:?}",
                claimed,
                completed,
                failed,
                retried,
                stale,
                cancelled,
                queue_depth,
                readiness_queue_depth,
                snapshot.entries.len(),
                snapshot.deficits.len(),
                snapshot.activity,
            );
        }
        thread::sleep(Duration::from_millis(20));
    }
    let drain_elapsed = drain_started.elapsed();
    let drain_throughput = expected_claims as f64 / drain_elapsed.as_secs_f64().max(f64::EPSILON);
    assert!(drain_throughput >= MIN_DRAIN_THROUGHPUT);
    let final_snapshot = readiness_snapshot(&source).expect("profile readiness snapshot");
    assert!(final_snapshot.is_fully_ready());

    let resources_after = process_resource_snapshot();
    let memory_growth = resources_after
        .memory_bytes
        .saturating_sub(resources_before.memory_bytes);
    let cpu_time_ms = resources_after
        .cpu_time_ms
        .saturating_sub(resources_before.cpu_time_ms);
    let profile_elapsed = discovery_elapsed.saturating_add(drain_elapsed);
    let cpu_core_equivalent =
        cpu_time_ms as f64 / profile_elapsed.as_secs_f64().max(f64::EPSILON) / 1_000.0;
    let disk_read_bytes = resources_after
        .disk_read_bytes
        .saturating_sub(resources_before.disk_read_bytes);
    let disk_written_bytes = resources_after
        .disk_written_bytes
        .saturating_sub(resources_before.disk_written_bytes);
    assert!(memory_growth <= MEMORY_GROWTH_BUDGET_BYTES);
    assert!(cpu_core_equivalent <= CPU_CORE_EQUIVALENT_BUDGET);
    assert!(disk_read_bytes <= DISK_IO_BUDGET_BYTES);
    assert!(disk_written_bytes <= DISK_IO_BUDGET_BYTES);
    let report = supervisor.shutdown();
    assert_eq!(report["joined"], true);
    assert_eq!(report["claimed"], expected_claims);
    assert_eq!(report["completed"], expected_claims);
    assert_eq!(report["queue_depth"], 0);
    assert_eq!(report["contention"], 0);

    eprintln!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "file_count": FILE_COUNT,
            "candidate_count": expected_claims,
            "discovery_elapsed_ms": discovery_elapsed.as_secs_f64() * 1_000.0,
            "materialization_candidates_per_second": materialization_throughput,
            "drain_elapsed_ms": drain_elapsed.as_secs_f64() * 1_000.0,
            "drain_candidates_per_second": drain_throughput,
            "priority_p99_us": priority_p99.as_micros(),
            "memory_growth_bytes": memory_growth,
            "cpu_time_ms": cpu_time_ms,
            "cpu_core_equivalent": cpu_core_equivalent,
            "disk_read_bytes": disk_read_bytes,
            "disk_written_bytes": disk_written_bytes,
            "budgets": {
                "discovery_elapsed_ms": DISCOVERY_BUDGET.as_millis(),
                "materialization_candidates_per_second": MIN_MATERIALIZATION_THROUGHPUT,
                "drain_elapsed_ms": DRAIN_BUDGET.as_millis(),
                "drain_candidates_per_second": MIN_DRAIN_THROUGHPUT,
                "priority_p99_us": PRIORITY_P99_BUDGET.as_micros(),
                "memory_growth_bytes": MEMORY_GROWTH_BUDGET_BYTES,
                "cpu_core_equivalent": CPU_CORE_EQUIVALENT_BUDGET,
                "disk_read_bytes": DISK_IO_BUDGET_BYTES,
                "disk_written_bytes": DISK_IO_BUDGET_BYTES,
                "db_contention_events": 0,
            },
            "supervisor": report,
        }))
        .expect("serialize source-processing profile")
    );
}

#[test]
#[ignore = "representative fully analyzed 10k-file readiness sweep and delta profile"]
fn profile_revision_gated_readiness_sweeps_10k() {
    const FILE_COUNT: usize = 10_000;
    const SETTLE_BUDGET: Duration = Duration::from_secs(180);
    const INITIAL_TARGET_TIMESTAMP: i64 = 100;

    let directory = tempfile::tempdir().expect("revision-gated profile source");
    let source = SampleSource::new_with_id(
        SourceId::from_string("revision-gated-readiness-profile"),
        directory.path().to_path_buf(),
    );
    source
        .open_db()
        .expect("create revision-gated profile database");
    let mut connection = open_connection(&source).expect("open revision-gated profile database");
    seed_profile_manifest(&mut connection, FILE_COUNT);
    assert!(
        publish_current_readiness_targets(
            &mut connection,
            source.id.as_str(),
            INITIAL_TARGET_TIMESTAMP,
        )
        .expect("publish revision-gated profile targets")
    );
    connection
        .execute(
            "INSERT INTO source_readiness_artifacts (
                source_id, scope_kind, scope_id, relative_path, stage, artifact_version,
                source_generation, content_generation, artifact_ref, completed_at
             )
             SELECT source_id, scope_kind, scope_id, relative_path, stage, required_version,
                    source_generation, content_generation,
                    'profile-seed:' || scope_kind || ':' || scope_id || ':' || stage,
                    ?1
             FROM source_readiness_targets
             WHERE source_id = ?2",
            params![INITIAL_TARGET_TIMESTAMP, source.id.as_str()],
        )
        .expect("seed fully analyzed readiness artifacts");
    connection
        .execute(
            "INSERT INTO metadata (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![META_LAST_MANIFEST_AUDIT_AT, now_epoch_seconds().to_string()],
        )
        .expect("mark profile manifest audit current");
    assert!(
        ReadinessStore::new(&mut connection)
            .reconcile(source.id.as_str(), now_epoch_seconds())
            .expect("reconcile seeded profile")
            .is_fully_ready()
    );
    #[cfg(target_os = "macos")]
    seed_durable_watcher_checkpoint(&mut connection, &source);
    let probe = readiness_safety_probe(&mut connection, &source, now_epoch_seconds(), false)
        .expect("probe seeded revision-gated readiness");
    assert!(
        probe.current,
        "seeded readiness safety probe must be current"
    );
    assert!(
        probe.earliest_deadline.is_none(),
        "seeded readiness safety probe must have no deadline"
    );
    drop(connection);

    let mut supervisor =
        SourceProcessingSupervisor::start_synthetic_profile(vec![source.clone()], true);
    let startup_deadline = Instant::now() + SETTLE_BUDGET;
    loop {
        let telemetry = supervisor.shared.telemetry();
        let queue_empty = telemetry.queue_depth == 0;
        drop(telemetry);
        let settled = supervisor.shared.control().dirty_sources.is_empty();
        let in_flight = !supervisor
            .shared
            .in_flight_work
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .is_empty();
        if queue_empty && settled && !in_flight {
            break;
        }
        assert!(
            Instant::now() < startup_deadline,
            "seeded 10k readiness source did not settle after startup"
        );
        thread::sleep(Duration::from_millis(5));
    }

    let target_rows_before: i64 = open_connection(&source)
        .expect("open profile target count")
        .query_row(
            "SELECT COUNT(*) FROM source_readiness_targets WHERE source_id = ?1",
            [source.id.as_str()],
            |row| row.get(0),
        )
        .expect("count seeded targets");
    assert_eq!(target_rows_before, (FILE_COUNT * 3 + 1) as i64);
    let full_audits_before = supervisor.shared.telemetry().full_audits;
    let steady_resources_before = process_resource_snapshot();
    let steady_started = Instant::now();
    for interval in 0..10 {
        supervisor.set_playback_active(interval >= 5);
        let noops_before = supervisor.shared.telemetry().cheap_noop_sweeps;
        supervisor
            .shared
            .control()
            .mark_all_sources_for_safety_probe();
        supervisor.shared.wake.notify_one();
        let deadline = Instant::now() + SETTLE_BUDGET;
        loop {
            let telemetry = supervisor.shared.telemetry();
            let nooped = telemetry.cheap_noop_sweeps > noops_before;
            let queue_empty = telemetry.queue_depth == 0;
            drop(telemetry);
            let settled = supervisor.shared.control().dirty_sources.is_empty();
            let in_flight = !supervisor
                .shared
                .in_flight_work
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .is_empty();
            if nooped && queue_empty && settled && !in_flight {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "10k readiness safety probe did not settle"
            );
            thread::sleep(Duration::from_millis(2));
        }
    }
    supervisor.set_playback_active(false);
    let steady_elapsed = steady_started.elapsed();
    let steady_resources_after = process_resource_snapshot();
    let steady_target_updates: i64 = open_connection(&source)
        .expect("open safety target verification")
        .query_row(
            "SELECT COUNT(*) FROM source_readiness_targets
             WHERE source_id = ?1 AND updated_at != ?2",
            params![source.id.as_str(), INITIAL_TARGET_TIMESTAMP],
            |row| row.get(0),
        )
        .expect("count safety target updates");
    assert_eq!(steady_target_updates, 0);
    assert_eq!(
        supervisor.shared.telemetry().full_audits,
        full_audits_before
    );

    let one_file_resources_before = process_resource_snapshot();
    let one_file_started = Instant::now();
    let connection = open_connection(&source).expect("open one-file profile database");
    connection
        .execute(
            "UPDATE wav_files
             SET content_hash = 'profile-one-file-change',
                 modified_ns = modified_ns + 1
             WHERE file_identity = 'identity-00000'",
            [],
        )
        .expect("commit one-file profile change");
    connection
        .execute(
            "INSERT INTO metadata (key, value) VALUES (?1, '1')
             ON CONFLICT(key) DO UPDATE
             SET value = CAST(CAST(value AS INTEGER) + 1 AS TEXT)",
            [META_WAV_PATHS_REVISION],
        )
        .expect("advance one-file profile generation");
    drop(connection);
    let delta_reconciliations_before = supervisor.shared.telemetry().delta_reconciliations;
    let completions_before = supervisor.shared.telemetry().completed;
    supervisor.request_source_delta(
        source.id.as_str(),
        supervisor.lifecycle_generations()[source.id.as_str()],
        &CommittedSourceDelta {
            revision: 1,
            changed: vec![ManifestIdentityDelta {
                identity: String::from("identity-00000"),
                relative_path: std::path::PathBuf::from("profile/sample-00000.wav"),
                content_generation: String::from("profile-one-file-change"),
                source_metadata_changed: false,
            }],
            ..CommittedSourceDelta::default()
        },
        "profile_one_file_delta",
    );
    let deadline = Instant::now() + SETTLE_BUDGET;
    loop {
        let telemetry = supervisor.shared.telemetry();
        let reconciled = telemetry.delta_reconciliations > delta_reconciliations_before;
        let completed = telemetry.completed >= completions_before.saturating_add(4);
        let queue_empty = telemetry.queue_depth == 0;
        drop(telemetry);
        let settled = supervisor.shared.control().dirty_sources.is_empty();
        let in_flight = !supervisor
            .shared
            .in_flight_work
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .is_empty();
        if reconciled && completed && queue_empty && settled && !in_flight {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "10k one-file readiness delta did not settle"
        );
        thread::sleep(Duration::from_millis(2));
    }
    let one_file_elapsed = one_file_started.elapsed();
    let one_file_resources_after = process_resource_snapshot();
    let mut connection = open_connection(&source).expect("open one-file profile verification");
    let touched_targets: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM source_readiness_targets
             WHERE source_id = ?1 AND updated_at != ?2",
            params![source.id.as_str(), INITIAL_TARGET_TIMESTAMP],
            |row| row.get(0),
        )
        .expect("count one-file target writes");
    assert_eq!(touched_targets, 4);
    assert!(
        ReadinessStore::new(&mut connection)
            .reconcile(source.id.as_str(), now_epoch_seconds())
            .expect("reconcile completed one-file delta")
            .is_fully_ready()
    );
    drop(connection);
    let report = supervisor.shutdown();
    assert_eq!(report["joined"], true);
    assert_eq!(report["contention"], 0);

    let metric_delta = |after: u64, before: u64| after.saturating_sub(before);
    eprintln!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "file_count": FILE_COUNT,
            "target_count": target_rows_before,
            "steady_state": {
                "sweep_count": 10,
                "playback_off_sweeps": 5,
                "playback_on_sweeps": 5,
                "elapsed_ms": steady_elapsed.as_secs_f64() * 1_000.0,
                "cpu_time_ms": metric_delta(
                    steady_resources_after.cpu_time_ms,
                    steady_resources_before.cpu_time_ms,
                ),
                "heap_growth_bytes": metric_delta(
                    steady_resources_after.heap_bytes_in_use,
                    steady_resources_before.heap_bytes_in_use,
                ),
                "disk_read_bytes": metric_delta(
                    steady_resources_after.disk_read_bytes,
                    steady_resources_before.disk_read_bytes,
                ),
                "disk_written_bytes": metric_delta(
                    steady_resources_after.disk_written_bytes,
                    steady_resources_before.disk_written_bytes,
                ),
                "target_rows_touched": steady_target_updates,
                "full_audits": supervisor.shared.telemetry().full_audits - full_audits_before,
            },
            "one_file_delta": {
                "identity_count": 1,
                "elapsed_ms": one_file_elapsed.as_secs_f64() * 1_000.0,
                "cpu_time_ms": metric_delta(
                    one_file_resources_after.cpu_time_ms,
                    one_file_resources_before.cpu_time_ms,
                ),
                "heap_growth_bytes": metric_delta(
                    one_file_resources_after.heap_bytes_in_use,
                    one_file_resources_before.heap_bytes_in_use,
                ),
                "disk_read_bytes": metric_delta(
                    one_file_resources_after.disk_read_bytes,
                    one_file_resources_before.disk_read_bytes,
                ),
                "disk_written_bytes": metric_delta(
                    one_file_resources_after.disk_written_bytes,
                    one_file_resources_before.disk_written_bytes,
                ),
                "target_rows_touched": touched_targets,
                "readiness_jobs_completed": report["completed"].as_u64().unwrap_or_default()
                    .saturating_sub(completions_before),
            },
            "supervisor": report,
        }))
        .expect("serialize revision-gated readiness profile")
    );
}
