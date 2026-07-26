#[test]
fn playback_active_does_not_block_hash_backlog_and_shutdown_joins() {
    let (_directory, source) = unhashed_source("playing");
    let mut supervisor =
        SourceProcessingSupervisor::start_with_playback_state(vec![source.clone()], true);

    wait_until(Duration::from_secs(10), || source_is_hashed(&source));
    let report = supervisor.shutdown();
    assert_eq!(report["joined"], true);
}

#[test]
fn playback_and_foreground_activity_do_not_publish_pause_feedback() {
    let (_directory, source) = unhashed_source("activity-feedback");
    let (sender, receiver) = std::sync::mpsc::channel();
    let mut supervisor = SourceProcessingSupervisor::start_with_playback_state_and_event_sink(
        vec![source.clone()],
        true,
        Some(Arc::new(sender)),
    );

    supervisor.set_foreground_activity(true);
    wait_until(Duration::from_secs(10), || source_is_hashed(&source));
    let progress = receiver
        .try_iter()
        .filter_map(|event| {
            let SourceProcessingEvent::Progress(progress) = event else {
                return None;
            };
            Some(progress)
        })
        .collect::<Vec<_>>();
    assert!(
        progress
            .iter()
            .any(|progress| progress.lifecycle.source_id == source.id.as_str()),
        "processing activity must remain visible while playback and foreground loading are active"
    );
    assert_eq!(supervisor.shutdown()["joined"], true);
}

#[test]
fn routine_activity_transitions_are_debug_only() {
    let supervisor = SourceProcessingSupervisor::dormant();

    let info = capture_logs(tracing::Level::INFO, || {
        supervisor.set_playback_active(true);
        supervisor.set_foreground_activity(true);
    });
    assert!(!info.contains("source_processing.playback_activity_changed"));
    assert!(!info.contains("source_processing.foreground_activity_changed"));

    let debug = capture_logs(tracing::Level::DEBUG, || {
        supervisor.set_playback_active(false);
        supervisor.set_foreground_activity(false);
    });
    assert!(debug.contains("source_processing.playback_activity_changed"));
    assert!(debug.contains("source_processing.foreground_activity_changed"));
}

#[test]
fn retirement_logging_is_bounded_at_info_and_detailed_at_debug() {
    fn offline_retirement_supervisor(prefix: &str) -> SourceProcessingSupervisor {
        let first = SampleSource::new_with_id(
            SourceId::from_string(format!("{prefix}-first")),
            PathBuf::from(format!("/missing/{prefix}/first")),
        );
        let second = SampleSource::new_with_id(
            SourceId::from_string(format!("{prefix}-second")),
            PathBuf::from(format!("/missing/{prefix}/second")),
        );
        let supervisor = SourceProcessingSupervisor::dormant();
        supervisor
            .replace_sources(vec![first, second])
            .expect("configure missing sources");
        supervisor
            .replace_sources(Vec::new())
            .expect("remove missing sources");
        supervisor
    }

    let mut info_supervisor = offline_retirement_supervisor("info-retirement");
    let info = capture_logs(tracing::Level::INFO, || {
        process_ready_source_retirements(&info_supervisor.shared);
    });
    assert!(info.contains("source_processing.retirement.sweep"));
    assert!(info.contains("scheduled=2"));
    assert!(info.contains("started=2"));
    assert!(info.contains("offline=2"));
    assert!(!info.contains("source_processing.retirement.started"));
    assert!(!info.contains("source_processing.retirement.offline"));
    assert_eq!(info.matches("source_processing.retirement.sweep").count(), 1);
    assert_eq!(info_supervisor.shutdown()["joined"], true);

    let mut debug_supervisor = offline_retirement_supervisor("debug-retirement");
    let debug = capture_logs(tracing::Level::DEBUG, || {
        process_ready_source_retirements(&debug_supervisor.shared);
    });
    assert!(debug.contains("source_processing.retirement.started"));
    assert!(debug.contains("source_processing.retirement.offline"));
    assert!(debug.contains("source_processing.retirement.sweep"));
    assert_eq!(debug_supervisor.shutdown()["joined"], true);
}

#[test]
fn brief_discovery_reconciliation_does_not_flash_processing_feedback() {
    let source = SampleSource::new_with_id(
        SourceId::from_string("stable-discovery"),
        PathBuf::from("/library/samples"),
    );
    let (sender, receiver) = std::sync::mpsc::channel();
    let shared = Shared::new(vec![source.clone()], Some(Arc::new(sender)));
    let control = shared.control();
    let lifecycle_generation = control.source_lifecycle_generations[source.id.as_str()];
    drop(control);
    let mut publisher = DiscoveryProgressPublisher {
        shared: &shared,
        source_id: source.id.as_str(),
        lifecycle_generation,
        started_at: Instant::now(),
        last_progress: None,
        last_event_publish_at: None,
        last_log_publish_at: None,
        event_published: false,
        work_units: 0,
    };

    publisher.advance(DiscoveryProgressUpdate::indeterminate(
        SourceDiscoveryPhase::InspectingManifest,
    ));
    publisher.advance(DiscoveryProgressUpdate::indeterminate(
        SourceDiscoveryPhase::ComparingReadiness,
    ));
    assert!(
        receiver.try_recv().is_err(),
        "a brief converged-source check must not flash active processing feedback"
    );
    assert!(!publisher.event_published);

    publisher.started_at = Instant::now() - DISCOVERY_PROGRESS_EVENT_GRACE_INTERVAL;
    publisher.advance(DiscoveryProgressUpdate::determinate(
        SourceDiscoveryPhase::QueueingWork,
        3,
        5,
    ));
    let SourceProcessingEvent::Progress(progress) = receiver
        .recv_timeout(Duration::from_secs(1))
        .expect("sustained discovery feedback")
    else {
        panic!("unexpected source-processing event");
    };
    assert_eq!(
        progress.activity,
        SourceProcessingActivity::Discovering {
            phase: SourceDiscoveryPhase::QueueingWork,
        }
    );
    assert_eq!((progress.completed, progress.total), (3, 5));
    assert!(
        progress.source_row_active,
        "grace-surviving discovery must identify its active source row"
    );
    assert!(publisher.event_published);
}

#[test]
fn discovery_progress_from_previous_readded_epoch_is_not_published() {
    let directory = tempfile::tempdir().expect("discovery source");
    let source = SampleSource::new_with_id(
        SourceId::from_string("readded-discovery-source"),
        directory.path().to_path_buf(),
    );
    let (sender, receiver) = std::sync::mpsc::channel();
    let shared = Arc::new(Shared::new(vec![source.clone()], Some(Arc::new(sender))));
    let supervisor = SourceProcessingSupervisor {
        shared: Arc::clone(&shared),
        coordinator: None,
        retirement_worker: None,
    };
    let old_generation = shared.control().source_lifecycle_generations[source.id.as_str()];

    supervisor
        .replace_sources(Vec::new())
        .expect("remove old discovery epoch");
    supervisor
        .replace_sources(vec![source.clone()])
        .expect("re-add source with a new discovery epoch");

    let mut publisher = DiscoveryProgressPublisher {
        shared: shared.as_ref(),
        source_id: source.id.as_str(),
        lifecycle_generation: old_generation,
        started_at: Instant::now() - DISCOVERY_PROGRESS_EVENT_GRACE_INTERVAL,
        last_progress: None,
        last_event_publish_at: None,
        last_log_publish_at: None,
        event_published: false,
        work_units: 0,
    };
    publisher.advance(DiscoveryProgressUpdate::determinate(
        SourceDiscoveryPhase::ComparingReadiness,
        1,
        5,
    ));
    assert!(!publisher.event_published);
    assert!(receiver.try_recv().is_err());
}

#[test]
fn removing_a_source_cancels_its_unstarted_backlog() {
    let (_directory, source) = unhashed_source("removed");
    let mut supervisor =
        SourceProcessingSupervisor::start_with_playback_state(vec![source.clone()], true);
    supervisor
        .replace_sources(Vec::new())
        .expect("remove configured sources");
    supervisor.set_playback_active(false);

    thread::sleep(Duration::from_millis(150));
    assert!(!source_is_hashed(&source));
    assert_eq!(supervisor.shutdown()["joined"], true);
}

#[test]
fn retirement_cleanup_cannot_block_active_source_convergence() {
    let (_retired_directory, retired) = unhashed_source("retirement-background");
    let (_active_directory, active) = unhashed_source("retirement-active-source");
    let mut supervisor = SourceProcessingSupervisor::dormant();
    supervisor
        .replace_sources(vec![retired])
        .expect("configure source that will be retired");
    supervisor
        .replace_sources(vec![active.clone()])
        .expect("replace retired source with active source");

    let shared = Arc::clone(&supervisor.shared);
    let retirement_blocker = shared
        .source_replacement
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let coordinator_shared = Arc::clone(&shared);
    supervisor.coordinator = Some(
        thread::Builder::new()
            .name(String::from("wavecrate-source-supervisor-test"))
            .spawn(move || run_coordinator(coordinator_shared))
            .expect("spawn source processing supervisor"),
    );

    wait_until(Duration::from_secs(10), || source_is_hashed(&active));
    drop(retirement_blocker);
    assert_eq!(supervisor.shutdown()["joined"], true);
}

#[test]
fn shutdown_cancels_blocked_retirement_cleanup_and_joins_worker() {
    let (_directory, source) = unhashed_source("retirement-shutdown");
    let mut supervisor = SourceProcessingSupervisor::start(vec![source.clone()]);
    supervisor
        .shared
        .retirement_cleanup_blocked
        .store(true, Ordering::Release);
    supervisor
        .replace_sources(Vec::new())
        .expect("remove source while retirement cleanup is blocked");
    wait_until(Duration::from_secs(5), || {
        supervisor
            .shared
            .retirement_cleanup_started
            .load(Ordering::Acquire)
    });

    let started_at = Instant::now();
    let report = supervisor.shutdown();

    assert_eq!(report["joined"], true);
    assert!(
        started_at.elapsed() < Duration::from_secs(1),
        "shutdown must cancel blocked retirement cleanup before joining"
    );
}

#[test]
fn source_replacement_cancels_blocked_retirement_without_waiting_for_storage() {
    let (_directory, source) = unhashed_source("retirement-replacement");
    let mut supervisor = SourceProcessingSupervisor::start(vec![source.clone()]);
    supervisor
        .shared
        .retirement_cleanup_blocked
        .store(true, Ordering::Release);
    supervisor
        .replace_sources(Vec::new())
        .expect("remove source while retirement cleanup is blocked");
    wait_until(Duration::from_secs(10), || {
        supervisor
            .shared
            .retirement_cleanup_started
            .load(Ordering::Acquire)
    });

    let started_at = Instant::now();
    supervisor
        .replace_sources(vec![source.clone()])
        .expect("re-add source while retirement child is blocked");
    assert!(
        started_at.elapsed() < Duration::from_millis(100),
        "source replacement must cancel retirement instead of waiting for storage"
    );
    wait_until(Duration::from_secs(2), || {
        supervisor.shared.control().pending_retirements.is_empty()
    });
    assert!(
        supervisor
            .shared
            .control()
            .source_is_active(source.id.as_str())
    );
    assert_eq!(supervisor.shutdown()["joined"], true);
}

#[test]
fn failed_async_retirement_retries_without_reactivating_removed_source() {
    let (_directory, source) = unhashed_source("retirement-fence-failure");
    let database_path = source.db_path().expect("source database path");
    std::fs::remove_file(&database_path).expect("remove source database");
    std::fs::create_dir(&database_path).expect("replace database with invalid directory");
    let mut supervisor = SourceProcessingSupervisor::dormant();
    supervisor
        .replace_sources(vec![source.clone()])
        .expect("configure source");

    supervisor
        .replace_sources(Vec::new())
        .expect("removal returns before asynchronous cleanup");
    process_ready_source_retirements(&supervisor.shared);
    supervisor.wake_source(source.id.as_str(), "late_watcher_event");
    supervisor.set_playback_active(true);
    supervisor.set_playback_active(false);

    let control = supervisor.shared.control();
    assert!(!control.sources.contains_key(source.id.as_str()));
    assert!(!control.dirty_sources.contains(source.id.as_str()));
    assert_eq!(control.pending_retirements.len(), 1);
    drop(control);
    assert!(
        supervisor
            .budget_handle()
            .acquire_scan(source.id.as_str())
            .is_none(),
        "quarantined retirement must reject late external scans"
    );

    std::fs::remove_dir(&database_path).expect("repair invalid database path");
    supervisor
        .shared
        .control()
        .pending_retirements
        .values_mut()
        .for_each(|retirement| retirement.retry_at = 0);
    process_ready_source_retirements(&supervisor.shared);
    assert!(supervisor.shared.control().pending_retirements.is_empty());
    let control = supervisor.shared.control();
    assert!(!control.sources.contains_key(source.id.as_str()));
    assert!(control.quarantined_sources.is_empty());
    drop(control);
    assert_eq!(supervisor.shutdown()["joined"], true);
}

#[test]
fn busy_source_discovery_is_rescheduled_after_retry_deadline() {
    let (_directory, source) = unhashed_source("busy-discovery-retry");
    let database_path = source.db_path().expect("source database path");
    let lock = rusqlite::Connection::open(&database_path).expect("open lock connection");
    lock.execute_batch("BEGIN EXCLUSIVE")
        .expect("hold source database lock");
    let (sender, receiver) = std::sync::mpsc::channel();
    let mut supervisor = SourceProcessingSupervisor::start_with_playback_state_and_event_sink(
        vec![source.clone()],
        false,
        Some(Arc::new(sender)),
    );

    let mut saw_waiting_for_retry = false;
    wait_until(Duration::from_secs(15), || {
        for event in receiver.try_iter() {
            if let SourceProcessingEvent::Health(health) = event
                && health.lifecycle.source_id == source.id.as_str()
                && health.state == SourceProcessingHealthState::WaitingForRetry
            {
                saw_waiting_for_retry = health.retry_at.is_some()
                    && health.failure_codes == ["discovery_retry_pending"];
            }
        }
        let telemetry = supervisor.shared.telemetry();
        telemetry.failed > 0
            && telemetry
                .retry_at_by_source
                .contains_key(source.id.as_str())
            && saw_waiting_for_retry
    });
    assert!(saw_waiting_for_retry);
    let retry_at = supervisor
        .shared
        .telemetry()
        .retry_at_by_source
        .get(source.id.as_str())
        .copied();
    assert!(
        retry_at.is_some_and(|deadline| deadline > now_epoch_seconds()),
        "busy discovery must remain observable as a scheduled retry"
    );

    lock.execute_batch("ROLLBACK").expect("release source lock");
    drop(lock);
    let mut saw_recovered_health = false;
    wait_until(Duration::from_secs(12), || {
        for event in receiver.try_iter() {
            if let SourceProcessingEvent::Health(health) = event
                && health.lifecycle.source_id == source.id.as_str()
                && matches!(
                    health.state,
                    SourceProcessingHealthState::Processing | SourceProcessingHealthState::Ready
                )
            {
                assert!(!health
                    .failure_codes
                    .iter()
                    .any(|code| code == "discovery_retry_pending"));
                saw_recovered_health = true;
            }
        }
        source_is_hashed(&source) && saw_recovered_health
    });
    assert!(saw_recovered_health);
    assert_eq!(supervisor.shutdown()["joined"], true);
}

#[test]
fn missing_removed_source_is_parked_offline_without_retry_wakes() {
    let (directory, source) = unhashed_source("retirement-offline");
    let mut supervisor = SourceProcessingSupervisor::dormant();
    supervisor
        .replace_sources(vec![source.clone()])
        .expect("configure source");
    supervisor
        .replace_sources(Vec::new())
        .expect("remove source");
    drop(directory);

    process_ready_source_retirements(&supervisor.shared);

    let control = supervisor.shared.control();
    let retirement = control
        .pending_retirements
        .values()
        .next()
        .expect("retain offline retirement fence");
    assert!(retirement.terminal_offline);
    assert_eq!(retirement.retry_at, i64::MAX);
    drop(control);
    assert_eq!(supervisor.shutdown()["joined"], true);
}

#[test]
fn source_wakes_cancel_only_that_sources_in_flight_generation() {
    let (_first_directory, first) = unhashed_source("first");
    let (_second_directory, second) = unhashed_source("second");
    let mut supervisor = SourceProcessingSupervisor::dormant();
    supervisor
        .replace_sources(vec![first.clone(), second.clone()])
        .expect("configure sources");
    let (first_generation, second_generation) = {
        let control = supervisor.shared.control();
        (
            Arc::clone(&control.source_work_cancels[first.id.as_str()]),
            Arc::clone(&control.source_work_cancels[second.id.as_str()]),
        )
    };
    let first_scan = supervisor
        .budget_handle()
        .acquire_scan(first.id.as_str())
        .expect("acquire first-source scan permit");
    let first_scan_generation = first_scan.cancel_token();

    supervisor.wake_source(first.id.as_str(), "test_source_wake");

    assert!(first_generation.load(Ordering::Acquire));
    assert!(first_scan_generation.load(Ordering::Acquire));
    assert!(!second_generation.load(Ordering::Acquire));
    let control = supervisor.shared.control();
    assert!(!control.source_work_cancels[first.id.as_str()].load(Ordering::Acquire));
    assert!(Arc::ptr_eq(
        &second_generation,
        &control.source_work_cancels[second.id.as_str()]
    ));
    drop(control);
    drop(first_scan);
    assert_eq!(supervisor.shutdown()["joined"], true);
}
