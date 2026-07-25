#[test]
fn readiness_lease_heartbeat_keeps_long_claim_current() {
    let (_directory, source) = unhashed_source("lease-heartbeat");
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
                 SET file_identity = 'identity-lease', content_hash = 'content-lease'
                 WHERE path = 'pending.wav'",
            [],
        )
        .expect("assign file identity");
    let target = ReadinessTarget::file(
        source.id.as_str(),
        "identity-lease",
        "pending.wav",
        ReadinessStage::IndexedIdentity,
        "manifest-v1",
        1,
        "content-lease",
    );
    let mut targets = vec![target.clone()];
    for stage in [
        ReadinessStage::AnalysisFeatures,
        ReadinessStage::EmbeddingAspects,
    ] {
        let mut terminal = target.clone();
        terminal.stage = stage;
        terminal.eligibility = ReadinessEligibility::Unsupported;
        targets.push(terminal);
    }
    targets.push(
        ReadinessTarget::source(
            source.id.as_str(),
            ReadinessStage::SimilarityLayout,
            "layout-v1",
            1,
            "members-1",
        )
        .with_eligibility(ReadinessEligibility::Unsupported),
    );
    let now = now_epoch_seconds();
    replace_readiness_targets(
        &mut connection,
        source.id.as_str(),
        1,
        1,
        SourceAvailability::Active,
        &targets,
        now,
    )
    .expect("publish readiness targets");
    let snapshot =
        reconcile_readiness(&connection, source.id.as_str(), now).expect("reconcile readiness");
    persist_readiness_deficits(&mut connection, &snapshot.deficits, now)
        .expect("persist readiness work");
    let claim = claim_readiness_target(&mut connection, &target, now, 2)
        .expect("claim readiness")
        .expect("claim available");
    let cancel = AtomicBool::new(false);

    let ((), stale) = run_with_readiness_lease_heartbeat(
        &source,
        &claim,
        &cancel,
        2,
        &DatabaseWriterGate::default(),
        |_| thread::sleep(Duration::from_millis(2_500)),
    )
    .expect("run with heartbeat");

    assert!(!stale);
    assert_eq!(
        complete_readiness_work(&mut connection, &claim, now_epoch_seconds())
            .expect("complete renewed claim"),
        ArtifactPublishOutcome::Recorded
    );
}

#[test]
fn retry_deadline_shortens_coordinator_wait_deterministically() {
    assert_eq!(
        coordinator_wait_duration(Some(105), 100, SAFETY_SWEEP_INTERVAL),
        Duration::from_secs(5)
    );
    assert_eq!(
        coordinator_wait_duration(Some(100), 100, SAFETY_SWEEP_INTERVAL),
        Duration::ZERO
    );
    assert_eq!(
        coordinator_wait_duration(Some(200), 100, SAFETY_SWEEP_INTERVAL),
        SAFETY_SWEEP_INTERVAL
    );
    assert_eq!(
        coordinator_wait_duration(None, 100, SAFETY_SWEEP_INTERVAL),
        SAFETY_SWEEP_INTERVAL
    );
    assert_eq!(
        coordinator_wait_duration(None, 100, Duration::from_secs(3)),
        Duration::from_secs(3),
        "priority wakes must preserve the remaining absolute safety-sweep deadline"
    );
}

#[test]
fn stale_scope_outcomes_discard_already_discovered_dependents() {
    let indexed = RuntimeTask::Readiness(ReadinessTarget::file(
        "source",
        "identity",
        "changing.wav",
        ReadinessStage::IndexedIdentity,
        "manifest-v1",
        1,
        "pending-identity",
    ));
    let mut analysis_task = indexed.clone();
    let RuntimeTask::Readiness(analysis_target) = &mut analysis_task else {
        unreachable!();
    };
    analysis_target.stage = ReadinessStage::AnalysisFeatures;

    assert_eq!(
        candidate_invalidation_scope(&indexed, Some(ExecutionOutcome::Retried { retry_at: 5 })),
        CandidateInvalidationScope::TargetScope
    );
    assert_eq!(
        candidate_invalidation_scope(&indexed, Some(ExecutionOutcome::Completed)),
        CandidateInvalidationScope::TargetScope,
        "committing a content hash invalidates pending-generation dependents"
    );
    let mut indexed_exact = indexed.clone();
    let RuntimeTask::Readiness(indexed_exact_target) = &mut indexed_exact else {
        unreachable!();
    };
    indexed_exact_target.content_generation = String::from("exact-content-hash");
    assert_eq!(
        candidate_invalidation_scope(&indexed_exact, Some(ExecutionOutcome::Completed)),
        CandidateInvalidationScope::None,
        "recording an already exact indexed identity must preserve same-generation dependents"
    );
    assert_eq!(
        candidate_invalidation_scope(&analysis_task, Some(ExecutionOutcome::Completed)),
        CandidateInvalidationScope::None
    );
    assert_eq!(
        candidate_invalidation_scope(
            &RuntimeTask::ManifestAudit { accelerated: false },
            Some(ExecutionOutcome::Completed)
        ),
        CandidateInvalidationScope::Source
    );
}

#[test]
fn retry_only_source_releases_owner_before_another_source_runs() {
    let mut scheduler = FairScheduler::default();
    let budgets = BudgetTracker::new(ProcessingBudgets::default());
    let first = [WorkCandidate::source(
        "recording",
        ProcessingLane::Hashing,
        0,
        0,
    )];
    assert_eq!(
        scheduler.choose(&first, &PriorityContext::default(), &budgets),
        Some(0)
    );

    let configured = ["recording".to_string(), "next".to_string()]
        .into_iter()
        .collect();
    let stats = [(
        "recording".to_string(),
        SourceDiscoveryStats {
            readiness_queue_depth: 1,
            earliest_retry_at: Some(100),
            ..SourceDiscoveryStats::default()
        },
    )]
    .into_iter()
    .collect();
    release_converged_source_owner(&mut scheduler, &configured, &stats, &[], false);
    assert_eq!(scheduler.active_source(), None);

    let next = [WorkCandidate::source("next", ProcessingLane::Hashing, 0, 0)];
    assert_eq!(
        scheduler.choose(&next, &PriorityContext::default(), &budgets),
        Some(0)
    );
    assert_eq!(scheduler.active_source(), Some("next"));
}

#[test]
fn in_flight_manifest_audit_keeps_active_source_owner_across_wake() {
    let mut scheduler = FairScheduler::default();
    let budgets = BudgetTracker::new(ProcessingBudgets::default());
    let audit = [WorkCandidate::source(
        "audited",
        ProcessingLane::Hashing,
        0,
        0,
    )];
    assert_eq!(
        scheduler.choose(&audit, &PriorityContext::default(), &budgets),
        Some(0)
    );
    let configured = ["audited".to_string()].into_iter().collect();
    let stats = [(
        "audited".to_string(),
        SourceDiscoveryStats::default(),
    )]
    .into_iter()
    .collect();

    release_converged_source_owner(&mut scheduler, &configured, &stats, &[], true);

    assert_eq!(scheduler.active_source(), Some("audited"));
}

#[test]
fn discovery_selects_exactly_one_source_and_keeps_the_active_owner() {
    let first = SampleSource::new_with_id(
        SourceId::from_string("first"),
        PathBuf::from("/source/first"),
    );
    let second = SampleSource::new_with_id(
        SourceId::from_string("second"),
        PathBuf::from("/source/second"),
    );
    let sources = vec![first, second];
    let pending = ["first".to_string(), "second".to_string()]
        .into_iter()
        .collect();
    let priority = PriorityContext {
        selected_source: Some("second".to_string()),
        ..PriorityContext::default()
    };

    assert_eq!(
        select_source_for_discovery(&sources, &pending, None, &priority).as_deref(),
        Some("second")
    );
    assert_eq!(
        select_source_for_discovery(&sources, &pending, Some("first"), &priority).as_deref(),
        Some("first"),
        "interactive priority must not switch an active source"
    );
    let pending = ["second".to_string()].into_iter().collect();
    assert_eq!(
        select_source_for_discovery(&sources, &pending, Some("first"), &priority),
        Some("second".to_string()),
        "another source should be discovered for bounded secondary execution"
    );
}
