use super::coordinator_completion::handle_completion;

#[test]
fn late_progress_is_rejected_across_remove_and_readd() {
    let directory = tempfile::tempdir().expect("progress source");
    let source = SampleSource::new_with_id(
        SourceId::from_string("readded-progress-source"),
        directory.path().to_path_buf(),
    );
    let target = ReadinessTarget::file(
        source.id.as_str(),
        "identity-1",
        "drums/kick.wav",
        ReadinessStage::AnalysisFeatures,
        "analysis-v1",
        1,
        "content-1",
    );
    let candidate = RuntimeCandidate {
        schedule: WorkCandidate::readiness(&target, 1),
        source: source.clone(),
        task: RuntimeTask::Readiness(target),
    };
    let (sender, receiver) = std::sync::mpsc::channel();
    let shared = Arc::new(Shared::new(vec![source.clone()], Some(Arc::new(sender))));
    let supervisor = SourceProcessingSupervisor {
        shared: Arc::clone(&shared),
        coordinator: None,
        retirement_worker: None,
    };
    let executing_generation = supervisor.lifecycle_generations()["readded-progress-source"];

    supervisor
        .replace_sources(Vec::new())
        .expect("remove source while work is executing");
    supervisor
        .replace_sources(vec![source])
        .expect("re-add source before late progress is published");
    let readded_generation = supervisor.lifecycle_generations()["readded-progress-source"];
    assert_ne!(executing_generation, readded_generation);

    publish_source_processing_progress(
        shared.as_ref(),
        &candidate,
        executing_generation,
        SourceDiscoveryStats {
            progress_completed: 1,
            progress_total: 2,
            ..SourceDiscoveryStats::default()
        },
    );

    assert!(
        receiver.try_recv().is_err(),
        "an event from a retired lifecycle must be fenced before reaching the sink"
    );
}

#[test]
fn held_execution_worker_defers_finished_event_until_result_is_handled() {
    let (sender, receiver) = std::sync::mpsc::channel();
    let shared = Shared::new(vec![], Some(Arc::new(sender)));

    assert!(!publish_finished_if_idle(
        &shared,
        true,
        Duration::from_secs(1),
        false,
        true,
    ));
    assert!(receiver.try_recv().is_err());

    assert!(publish_finished_if_idle(
        &shared,
        true,
        Duration::from_secs(1),
        false,
        false,
    ));
    assert!(matches!(receiver.try_recv(), Ok(SourceProcessingEvent::Completed)));
}

#[test]
fn routine_only_progress_does_not_publish_completion() {
    let (sender, receiver) = std::sync::mpsc::channel();
    let shared = Shared::new(vec![], Some(Arc::new(sender)));

    assert!(!publish_finished_if_idle(
        &shared,
        false,
        Duration::from_secs(1),
        false,
        false,
    ));
    assert!(receiver.try_recv().is_err());
}

#[test]
fn completion_from_removed_lifecycle_cannot_mutate_readded_source_state() {
    let directory = tempfile::tempdir().expect("temporary source");
    let source = SampleSource::new_with_id(
        SourceId::from_string("stale-completion-source"),
        directory.path().to_path_buf(),
    );
    let shared = Arc::new(Shared::new(vec![source.clone()], None));
    let supervisor = SourceProcessingSupervisor {
        shared: Arc::clone(&shared),
        coordinator: None,
        retirement_worker: None,
    };
    let old_cancel = shared.control().source_work_cancels[source.id.as_str()].clone();
    let in_flight = shared
        .begin_in_flight_work(source.id.as_str(), &old_cancel)
        .expect("begin old lifecycle work");
    let old_generation = in_flight.lifecycle_generation;
    let permit = shared
        .budgets()
        .try_acquire(source.id.as_str(), ProcessingLane::Scan)
        .expect("reserve old lifecycle budget");

    supervisor
        .replace_sources(Vec::new())
        .expect("remove old lifecycle");
    supervisor
        .replace_sources(vec![source.clone()])
        .expect("re-add source");
    assert_ne!(
        shared.control().source_lifecycle_generations[source.id.as_str()],
        old_generation
    );

    let candidate = || RuntimeCandidate {
        schedule: WorkCandidate::source(source.id.as_str(), ProcessingLane::Scan, 0, 0),
        source: source.clone(),
        task: RuntimeTask::ManifestAudit { accelerated: false },
    };
    let mut candidates = vec![candidate()];
    let mut source_stats = BTreeMap::new();
    let mut state = CoordinatorExecutionState {
        next_retry_at: None,
        pending_similarity_refresh_lifecycles: BTreeSet::new(),
        last_similarity_refresh_publish_at: None,
        active_progress_source: None,
        last_progress_publish_at: None,
        progress_visible: false,
        routine_maintenance_sources: BTreeSet::new(),
    };
    handle_completion(
        &shared,
        &mut candidates,
        &mut source_stats,
        &mut state,
        ExecutionResult {
            candidate: candidate(),
            permit,
            lifecycle_generation: old_generation,
            result: Ok(ExecutionOutcome::CompletedAwaitingForegroundRefresh),
            elapsed_ms: 1.0,
            in_flight,
        },
    );

    let control = shared.control();
    assert!(
        !control
            .awaiting_foreground_refresh_sources
            .contains(source.id.as_str()),
        "old completion must not block discovery for the re-added lifecycle"
    );
    assert_eq!(candidates.len(), 1, "new lifecycle candidate must remain queued");
    drop(control);
    assert_eq!(shared.telemetry().stale, 1);
}

#[test]
fn final_similarity_layout_completion_wakes_durable_reconciliation() {
    let directory = tempfile::tempdir().expect("temporary source");
    let source = SampleSource::new_with_id(
        SourceId::from_string("similarity-layout-refresh-source"),
        directory.path().to_path_buf(),
    );
    let (sender, receiver) = std::sync::mpsc::channel();
    let shared = Arc::new(Shared::new(vec![source.clone()], Some(Arc::new(sender))));
    let cancel = shared.control().source_work_cancels[source.id.as_str()].clone();
    let in_flight = shared
        .begin_in_flight_work(source.id.as_str(), &cancel)
        .expect("begin similarity layout work");
    let lifecycle_generation = in_flight.lifecycle_generation;
    let target = ReadinessTarget::source(
        source.id.as_str(),
        ReadinessStage::SimilarityLayout,
        "layout-v1",
        1,
        "layout-generation-1",
    );
    let candidate = RuntimeCandidate {
        schedule: WorkCandidate::readiness(&target, 1),
        source: source.clone(),
        task: RuntimeTask::Readiness(target),
    };
    let permit = shared
        .budgets()
        .try_acquire(source.id.as_str(), ProcessingLane::Finalization)
        .expect("reserve finalization budget");
    let mut candidates = Vec::new();
    let mut source_stats = BTreeMap::new();
    let mut state = CoordinatorExecutionState {
        next_retry_at: None,
        pending_similarity_refresh_lifecycles: BTreeSet::new(),
        last_similarity_refresh_publish_at: None,
        active_progress_source: None,
        last_progress_publish_at: None,
        progress_visible: true,
        routine_maintenance_sources: BTreeSet::new(),
    };

    handle_completion(
        &shared,
        &mut candidates,
        &mut source_stats,
        &mut state,
        ExecutionResult {
            candidate,
            permit,
            lifecycle_generation,
            result: Ok(ExecutionOutcome::Completed),
            elapsed_ms: 1.0,
            in_flight,
        },
    );

    let control = shared.control();
    assert!(
        control
            .dirty_sources
            .contains(source.id.as_str()),
        "final similarity layout completion must request a fresh durable snapshot"
    );
    assert_eq!(control.wake_reason, "source_stage_progress");
    drop(control);
    assert!(receiver.try_recv().is_err());
}

#[test]
fn lifecycle_fence_remains_held_until_event_delivery_finishes() {
    #[derive(Default)]
    struct BlockingSink {
        state: Mutex<(bool, bool, Vec<SourceProcessingEvent>)>,
        wake: Condvar,
    }

    impl SourceProcessingEventSink for BlockingSink {
        fn try_publish(&self, event: SourceProcessingEvent) -> bool {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            state.0 = true;
            self.wake.notify_all();
            while !state.1 {
                state = self
                    .wake
                    .wait(state)
                    .unwrap_or_else(|poison| poison.into_inner());
            }
            state.2.push(event);
            true
        }
    }

    let directory = tempfile::tempdir().expect("progress source");
    let source = SampleSource::new_with_id(
        SourceId::from_string("atomic-progress-source"),
        directory.path().to_path_buf(),
    );
    let sink = Arc::new(BlockingSink::default());
    let shared = Arc::new(Shared::new(
        vec![source.clone()],
        Some(Arc::clone(&sink) as Arc<dyn SourceProcessingEventSink>),
    ));
    let lifecycle_generation = shared.control().source_lifecycle_generations[source.id.as_str()];
    let publisher_shared = Arc::clone(&shared);
    let publisher = thread::spawn(move || {
        publisher_shared.publish_event(SourceProcessingEvent::Progress(
            SourceProcessingProgressEvent {
                lifecycle: SourceProcessingLifecycle::new(
                    "atomic-progress-source",
                    lifecycle_generation,
                ),
                presentation: SourceProcessingPresentation::UserRelevant,
                source_row_active: true,
                completed: 1,
                total: 2,
                activity: SourceProcessingActivity::Readiness {
                    stage: ReadinessStage::AnalysisFeatures,
                    relative_path: Some(String::from("drums/kick.wav")),
                },
            },
        ))
    });

    let mut sink_state = sink
        .state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    while !sink_state.0 {
        sink_state = sink
            .wake
            .wait(sink_state)
            .unwrap_or_else(|poison| poison.into_inner());
    }
    drop(sink_state);

    let replacement_supervisor = SourceProcessingSupervisor {
        shared: Arc::clone(&shared),
        coordinator: None,
        retirement_worker: None,
    };
    let (replacement_started, replacement_started_rx) = std::sync::mpsc::channel();
    let (replacement_finished, replacement_finished_rx) = std::sync::mpsc::channel();
    let replacement = thread::spawn(move || {
        replacement_started.send(()).expect("replacement start");
        replacement_supervisor
            .replace_sources(Vec::new())
            .expect("remove source");
        replacement_supervisor
            .replace_sources(vec![source])
            .expect("re-add source");
        replacement_finished.send(()).expect("replacement finish");
        replacement_supervisor
    });
    replacement_started_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("replacement thread started");
    assert!(
        replacement_finished_rx
            .recv_timeout(Duration::from_millis(50))
            .is_err(),
        "source replacement must wait until admitted event delivery finishes"
    );

    let mut sink_state = sink
        .state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    sink_state.1 = true;
    sink.wake.notify_all();
    drop(sink_state);

    assert!(publisher.join().expect("publisher joined"));
    replacement_finished_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("replacement finished after event delivery");
    let replacement_supervisor = replacement.join().expect("replacement joined");
    assert_ne!(
        replacement_supervisor.lifecycle_generations()["atomic-progress-source"],
        lifecycle_generation
    );
    assert_eq!(
        sink.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .2
            .len(),
        1
    );
}

#[test]
fn readiness_progress_counts_remain_scoped_to_the_reported_source() {
    let mut source_stats = BTreeMap::from([
        (
            String::from("first"),
            SourceDiscoveryStats {
                readiness_queue_depth: 1,
                progress_completed: 25_000,
                progress_total: 26_000,
                ..SourceDiscoveryStats::default()
            },
        ),
        (
            String::from("second"),
            SourceDiscoveryStats {
                progress_completed: 24_000,
                progress_total: 25_000,
                ..SourceDiscoveryStats::default()
            },
        ),
    ]);

    let progress = advance_source_progress(&mut source_stats, "first")
        .expect("first source progress advances");

    assert_eq!(progress.progress_completed, 25_001);
    assert_eq!(progress.progress_total, 26_000);
    assert_eq!(progress.readiness_queue_depth, 0);
    assert_eq!(source_stats["second"].progress_completed, 24_000);
    assert_eq!(source_stats["second"].progress_total, 25_000);
}

#[test]
fn periodic_manifest_audit_wakes_browser_projection_after_committed_repair() {
    let directory = tempfile::tempdir().expect("manifest audit source");
    let source = SampleSource::new_with_id(
        SourceId::from_string("audit-browser-wake"),
        directory.path().to_path_buf(),
    );
    source.open_db().expect("create source database");
    std::fs::write(directory.path().join("missed.wav"), [7_u8; 32])
        .expect("write missed watcher file");
    let (sender, receiver) = std::sync::mpsc::channel();

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
    assert_eq!(
        execute_candidate(
            &candidate,
            0,
            &AtomicBool::new(false),
            &DatabaseWriterGate::default(),
            ContentAuditActivity::default(),
            &mut |event| sender.send(event).is_ok(),
        )
        .expect("execute manifest audit"),
        ExecutionOutcome::CompletedAwaitingForegroundRefresh
    );
    let events = receiver.try_iter().collect::<Vec<_>>();
    let progress = events
        .iter()
        .find_map(|event| match event {
            SourceProcessingEvent::Progress(progress) => Some(progress),
            _ => None,
        })
        .expect("audit should publish checked-file progress");
    let (lifecycle, committed_delta) = events
        .iter()
        .find_map(|event| match event {
            SourceProcessingEvent::ManifestAuditCommitted {
                lifecycle,
                committed_delta,
                ..
            } => Some((lifecycle, committed_delta)),
            _ => None,
        })
        .expect("audit should publish a browser projection wake");

    assert_eq!(lifecycle.source_id, "audit-browser-wake");
    assert_eq!(lifecycle.generation, 0);
    assert_eq!(progress.lifecycle.source_id, "audit-browser-wake");
    assert_eq!(progress.completed, 1);
    assert_eq!(progress.total, 1);
    assert!(
        !progress.source_row_active,
        "manifest maintenance remains visible without claiming the active source pulse"
    );
    assert_eq!(
        progress.activity,
        SourceProcessingActivity::ManifestAudit {
            checked: Some(1),
            relative_path: Some(PathBuf::from("missed.wav")),
        }
    );
    assert_eq!(committed_delta.created.len(), 1);
    assert_eq!(
        committed_delta.created[0].relative_path,
        Path::new("missed.wav")
    );
}

#[test]
fn delivered_manifest_handoff_survives_post_commit_cancellation() {
    assert_eq!(
        manifest_audit_execution_outcome(true, false, true),
        ExecutionOutcome::CompletedAwaitingForegroundRefresh
    );
    assert_eq!(
        manifest_audit_execution_outcome(true, true, true),
        ExecutionOutcome::FailedAwaitingForegroundRefresh
    );
    assert_eq!(
        manifest_audit_execution_outcome(false, false, true),
        ExecutionOutcome::Cancelled
    );
    assert_eq!(
        manifest_audit_execution_outcome(false, true, false),
        ExecutionOutcome::Failed
    );
}

#[test]
fn manifest_projection_handoff_defers_discovery_until_external_scan_releases() {
    let directory = tempfile::tempdir().expect("manifest handoff source");
    let source = SampleSource::new_with_id(
        SourceId::from_string("audit-foreground-handoff"),
        directory.path().to_path_buf(),
    );
    source.open_db().expect("create source database");
    std::fs::write(directory.path().join("missed.wav"), [7_u8; 32])
        .expect("write missed watcher file");
    let (sender, receiver) = std::sync::mpsc::channel();
    let mut supervisor = SourceProcessingSupervisor::start_with_playback_state_and_event_sink(
        vec![source.clone()],
        false,
        Some(Arc::new(sender)),
    );

    let deadline = Instant::now() + Duration::from_secs(10);
    let lifecycle_generation = loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let event = receiver
            .recv_timeout(remaining)
            .expect("manifest audit should publish its committed delta");
        if let SourceProcessingEvent::ManifestAuditCommitted {
            lifecycle,
            committed_delta,
            ..
        } = event
            && lifecycle.source_id == source.id.as_str()
            && !committed_delta.created.is_empty()
        {
            break lifecycle.generation;
        }
    };
    let permit = supervisor
        .budget_handle()
        .acquire_scan_for_generation(source.id.as_str(), lifecycle_generation)
        .expect("foreground refresh should acquire the source scan lane");
    let discoveries_before = supervisor.shared.telemetry().source_discoveries;
    supervisor.wake_source(source.id.as_str(), "change_during_manifest_refresh");

    thread::sleep(Duration::from_millis(250));
    assert_eq!(
        supervisor.shared.telemetry().source_discoveries,
        discoveries_before,
        "coordinator must not rediscover while foreground refresh owns reconciliation"
    );

    drop(permit);
    thread::sleep(Duration::from_millis(100));
    assert_eq!(
        supervisor.shared.telemetry().source_discoveries,
        discoveries_before,
        "scan permit release alone must not bypass SourceScanFinished reconciliation"
    );
    supervisor.finish_foreground_source_refresh(source.id.as_str(), "source_scan_finished");
    wait_until(Duration::from_secs(10), || {
        supervisor.shared.telemetry().source_discoveries > discoveries_before
    });
    assert_eq!(supervisor.shutdown()["joined"], true);
}

#[test]
fn unavailable_foreground_refresh_releases_handoff_for_offline_discovery() {
    let (directory, source) = unhashed_source("audit-refresh-unavailable");
    let mut supervisor = SourceProcessingSupervisor::dormant();
    supervisor
        .replace_sources(vec![source.clone()])
        .expect("configure source");
    {
        let mut control = supervisor.shared.control();
        control.dirty_sources.clear();
        control
            .awaiting_foreground_refresh_sources
            .insert(source.id.as_str().to_string());
    }
    drop(directory);

    supervisor
        .finish_foreground_source_refresh(source.id.as_str(), "source_refresh_root_unavailable");

    let control = supervisor.shared.control();
    assert!(
        !control
            .awaiting_foreground_refresh_sources
            .contains(source.id.as_str())
    );
    assert!(control.dirty_sources.contains(source.id.as_str()));
    drop(control);
    assert_eq!(supervisor.shutdown()["joined"], true);
}
