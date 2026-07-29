#[test]
#[ignore = "representative 10k-file source discovery profile"]
fn profile_large_source_discovery_baseline() {
    const FILE_COUNT: usize = 10_000;
    let directory = tempfile::tempdir().expect("large profile source");
    let source = SampleSource::new_with_id(
        SourceId::from_string("large-discovery-profile"),
        directory.path().to_path_buf(),
    );
    source.open_db().expect("create profile source database");
    let database_root = source.database_root().expect("profile database root");
    let mut connection = SourceDatabase::open_connection_with_role_and_database_root(
        &source.root,
        &database_root,
        SourceDatabaseConnectionRole::JobWorker,
    )
    .expect("open profile database");
    let transaction = connection.transaction().expect("start profile seed");
    {
        let mut insert = transaction
            .prepare(
                "INSERT INTO wav_files (
                        path, file_size, modified_ns, file_identity, content_hash, missing,
                        extension
                     ) VALUES (?1, 1024, 1, ?2, ?3, 0, 'wav')",
            )
            .expect("prepare profile insert");
        for index in 0..FILE_COUNT {
            insert
                .execute(params![
                    format!("profile/sample-{index:05}.wav"),
                    format!("identity-{index:05}"),
                    format!("content-{index:05}"),
                ])
                .expect("insert profile row");
        }
    }
    transaction.commit().expect("commit profile seed");
    drop(connection);

    let started_at = Instant::now();
    let cancel = AtomicBool::new(false);
    let Cancellable::Completed((candidates, stats)) =
        discover_source_candidates(&source, 100, false, &cancel).expect("discover large source")
    else {
        panic!("large source discovery unexpectedly cancelled");
    };
    let elapsed = started_at.elapsed();

    assert_eq!(candidates.len(), FILE_COUNT * 3);
    assert_eq!(stats.readiness_queue_depth, FILE_COUNT * 3);
    assert_eq!(
        stats.prerequisites_blocked,
        FILE_COUNT * 3,
        "the source-wide similarity layout must remain parked until file embeddings converge"
    );
    eprintln!(
        "large_source_discovery file_count={FILE_COUNT} candidate_count={} elapsed_ms={:.3}",
        candidates.len(),
        elapsed.as_secs_f64() * 1_000.0,
    );
}

#[test]
fn discovery_progress_reports_phase_specific_truthful_counts() {
    const FILE_COUNT: usize = 8;
    let directory = tempfile::tempdir().expect("progress source");
    let source = SampleSource::new_with_id(
        SourceId::from_string("discovery-progress"),
        directory.path().to_path_buf(),
    );
    source.open_db().expect("create progress source database");
    let database_root = source.database_root().expect("progress database root");
    let mut connection = SourceDatabase::open_connection_with_role_and_database_root(
        &source.root,
        &database_root,
        SourceDatabaseConnectionRole::JobWorker,
    )
    .expect("open progress database");
    let transaction = connection.transaction().expect("start progress seed");
    for index in 0..FILE_COUNT {
        transaction
            .execute(
                "INSERT INTO wav_files (
                        path, file_size, modified_ns, file_identity, content_hash, missing,
                        extension
                     ) VALUES (?1, 1024, 1, ?2, ?3, 0, 'wav')",
                params![
                    format!("progress/sample-{index:02}.wav"),
                    format!("progress-identity-{index:02}"),
                    format!("progress-content-{index:02}"),
                ],
            )
            .expect("insert progress row");
    }
    transaction.commit().expect("commit progress seed");
    drop(connection);

    let mut updates = Vec::new();
    let Cancellable::Completed(_) = discover_source_candidates_with_progress(
        &source,
        100,
        false,
        false,
        None,
        false,
        &AtomicBool::new(false),
        &mut |update| updates.push(update),
    )
    .expect("discover source with progress") else {
        panic!("progress discovery unexpectedly cancelled");
    };

    assert!(updates.len() > FILE_COUNT);
    assert!(
        updates
            .iter()
            .any(|update| update.phase == SourceDiscoveryPhase::InspectingManifest)
    );
    assert!(
        updates
            .iter()
            .any(|update| update.phase == SourceDiscoveryPhase::PreparingTargets
                && update.completed == FILE_COUNT
                && update.total == FILE_COUNT)
    );
    assert!(
        updates.iter().any(|update| {
            update.phase == SourceDiscoveryPhase::ComparingReadiness
                && update.completed == FILE_COUNT * 3 + 1
                && update.total == FILE_COUNT * 3 + 1
        })
    );
    assert!(
        updates.iter().any(|update| {
            update.phase == SourceDiscoveryPhase::QueueingWork
                && update.completed == FILE_COUNT * 3 + 1
                && update.total == FILE_COUNT * 3 + 1
        })
    );
}

#[test]
fn discovery_progress_publisher_exposes_determinate_target_progress() {
    let directory = tempfile::tempdir().expect("progress source");
    let source = SampleSource::new_with_id(
        SourceId::from_string("progress-publisher"),
        directory.path().to_path_buf(),
    );
    let (sender, receiver) = std::sync::mpsc::channel();
    let shared = Shared::new(vec![source], Some(Arc::new(sender)));
    let lifecycle_generation = shared.control().source_lifecycle_generations["progress-publisher"];
    let mut publisher = DiscoveryProgressPublisher {
        shared: &shared,
        source_id: "progress-publisher",
        lifecycle_generation,
        started_at: Instant::now() - DISCOVERY_PROGRESS_EVENT_GRACE_INTERVAL,
        last_progress: None,
        last_event_publish_at: None,
        last_log_publish_at: None,
        event_published: false,
        work_units: 0,
        presentation: SourceProcessingPresentation::UserRelevant,
    };

    publisher.advance(DiscoveryProgressUpdate::determinate(
        SourceDiscoveryPhase::ComparingReadiness,
        128,
        512,
    ));
    publisher.last_event_publish_at = Some(Instant::now() - DISCOVERY_PROGRESS_REFRESH_INTERVAL);
    publisher.advance(DiscoveryProgressUpdate::determinate(
        SourceDiscoveryPhase::ComparingReadiness,
        256,
        512,
    ));

    let updates = receiver
        .try_iter()
        .filter_map(|event| match event {
            SourceProcessingEvent::Progress(progress) => Some(progress),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(updates.len(), 2);
    assert_eq!(updates[0].lifecycle.source_id, "progress-publisher");
    assert_eq!(updates[0].lifecycle.generation, lifecycle_generation);
    assert_eq!((updates[0].completed, updates[0].total), (128, 512));
    assert_eq!(
        updates[0].activity,
        SourceProcessingActivity::Discovering {
            phase: SourceDiscoveryPhase::ComparingReadiness,
        }
    );
    assert_eq!((updates[1].completed, updates[1].total), (256, 512));
    assert_eq!(
        updates[1].activity,
        SourceProcessingActivity::Discovering {
            phase: SourceDiscoveryPhase::ComparingReadiness,
        }
    );
}

#[test]
fn discovery_progress_publisher_rejects_phase_and_count_regressions() {
    let source = SampleSource::new_with_id(
        SourceId::from_string("progress-regression"),
        PathBuf::from("/library/progress-regression"),
    );
    let (sender, receiver) = std::sync::mpsc::channel();
    let shared = Shared::new(vec![source], Some(Arc::new(sender)));
    let lifecycle_generation = shared.control().source_lifecycle_generations["progress-regression"];
    let mut publisher = DiscoveryProgressPublisher {
        shared: &shared,
        source_id: "progress-regression",
        lifecycle_generation,
        started_at: Instant::now() - DISCOVERY_PROGRESS_EVENT_GRACE_INTERVAL,
        last_progress: None,
        last_event_publish_at: None,
        last_log_publish_at: None,
        event_published: false,
        work_units: 0,
        presentation: SourceProcessingPresentation::UserRelevant,
    };

    publisher.advance(DiscoveryProgressUpdate::determinate(
        SourceDiscoveryPhase::QueueingWork,
        4,
        8,
    ));
    publisher.last_event_publish_at = Some(Instant::now() - DISCOVERY_PROGRESS_REFRESH_INTERVAL);
    publisher.advance(DiscoveryProgressUpdate::determinate(
        SourceDiscoveryPhase::QueueingWork,
        3,
        8,
    ));
    publisher.advance(DiscoveryProgressUpdate::determinate(
        SourceDiscoveryPhase::QueueingWork,
        4,
        6,
    ));
    publisher.advance(DiscoveryProgressUpdate::determinate(
        SourceDiscoveryPhase::ComparingReadiness,
        8,
        8,
    ));

    let updates = receiver
        .try_iter()
        .filter(|event| matches!(event, SourceProcessingEvent::Progress(_)))
        .count();
    assert_eq!(updates, 1, "stale progress must not replace newer progress");
    assert_eq!(
        publisher.last_progress,
        Some(DiscoveryProgressUpdate::determinate(
            SourceDiscoveryPhase::QueueingWork,
            4,
            8,
        ))
    );
}

#[test]
fn discovery_phase_progress_is_debug_only() {
    let source = SampleSource::new_with_id(
        SourceId::from_string("progress-log-policy"),
        PathBuf::from("/library/progress-log-policy"),
    );
    let shared = Shared::new(vec![source], None);
    let lifecycle_generation = shared.control().source_lifecycle_generations["progress-log-policy"];
    let mut publisher = DiscoveryProgressPublisher {
        shared: &shared,
        source_id: "progress-log-policy",
        lifecycle_generation,
        started_at: Instant::now(),
        last_progress: None,
        last_event_publish_at: None,
        last_log_publish_at: None,
        event_published: false,
        work_units: 0,
        presentation: SourceProcessingPresentation::UserRelevant,
    };

    let info = capture_logs(tracing::Level::INFO, || {
        publisher.advance(DiscoveryProgressUpdate::indeterminate(
            SourceDiscoveryPhase::InspectingManifest,
        ));
    });
    assert!(!info.contains("source_processing.discovery_progress"));

    publisher.last_progress = None;
    publisher.last_log_publish_at = None;
    let debug = capture_logs(tracing::Level::DEBUG, || {
        publisher.advance(DiscoveryProgressUpdate::determinate(
            SourceDiscoveryPhase::ComparingReadiness,
            1,
            2,
        ));
    });
    assert!(debug.contains("source_processing.discovery_progress"));
    assert!(debug.contains("work_units=2"));
}

#[test]
fn discovery_progress_cancellation_mid_manifest_resumes_cleanly() {
    const FILE_COUNT: usize = 512;
    let directory = tempfile::tempdir().expect("large cancellation source");
    let source = SampleSource::new_with_id(
        SourceId::from_string("large-discovery-cancel"),
        directory.path().to_path_buf(),
    );
    source
        .open_db()
        .expect("create cancellation source database");
    let database_root = source.database_root().expect("cancellation database root");
    let mut connection = SourceDatabase::open_connection_with_role_and_database_root(
        &source.root,
        &database_root,
        SourceDatabaseConnectionRole::JobWorker,
    )
    .expect("open cancellation database");
    let transaction = connection.transaction().expect("start cancellation seed");
    {
        let mut insert = transaction
            .prepare(
                "INSERT INTO wav_files (
                        path, file_size, modified_ns, file_identity, content_hash, missing,
                        extension
                     ) VALUES (?1, 1024, 1, ?2, ?3, 0, 'wav')",
            )
            .expect("prepare cancellation seed");
        for index in 0..FILE_COUNT {
            insert
                .execute(params![
                    format!("cancel/sample-{index:05}.wav"),
                    format!("cancel-identity-{index:05}"),
                    format!("cancel-content-{index:05}"),
                ])
                .expect("insert cancellation row");
        }
    }
    transaction.commit().expect("commit cancellation seed");

    let cancel = AtomicBool::new(false);
    let mut checkpoints = 0_usize;
    let started_at = Instant::now();
    let cancelled_outcome = publish_current_readiness_targets_with_cancel_and_checkpoint(
        &mut connection,
        source.id.as_str(),
        100,
        &cancel,
        false,
        &mut |_| {
            checkpoints += 1;
            if checkpoints == 128 {
                cancel.store(true, Ordering::Release);
            }
        },
    )
    .expect("cancel manifest discovery");

    assert!(matches!(cancelled_outcome, Cancellable::Cancelled));
    assert!(started_at.elapsed() < Duration::from_secs(1));
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM source_readiness_targets WHERE source_id = ?1",
                [source.id.as_str()],
                |row| row.get::<_, i64>(0),
            )
            .expect("count cancelled readiness targets"),
        0,
        "cancelled discovery must not publish a partial desired state"
    );

    cancel.store(false, Ordering::Release);
    assert!(
        publish_current_readiness_targets_with_cancel(
            &mut connection,
            source.id.as_str(),
            101,
            &cancel,
        )
        .is_ok_and(|outcome| matches!(outcome, Cancellable::Completed(true)))
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM source_readiness_targets WHERE source_id = ?1",
                [source.id.as_str()],
                |row| row.get::<_, i64>(0),
            )
            .expect("count resumed readiness targets"),
        i64::try_from(FILE_COUNT * 3 + 1).expect("target count fits i64")
    );
}
