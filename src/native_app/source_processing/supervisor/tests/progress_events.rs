#[test]
fn readiness_progress_publishes_determinate_source_job_feedback() {
    let directory = tempfile::tempdir().expect("progress source");
    let source = SampleSource::new_with_id(
        SourceId::from_string("progress-source"),
        directory.path().to_path_buf(),
    );
    let target = ReadinessTarget::file(
        source.id.as_str(),
        "identity-1",
        "drums/kick.wav",
        ReadinessStage::EmbeddingAspects,
        "embedding-v1",
        1,
        "content-1",
    );
    let candidate = RuntimeCandidate {
        schedule: WorkCandidate::readiness(&target, 1),
        source: source.clone(),
        task: RuntimeTask::Readiness(target),
    };
    let (sender, receiver) = std::sync::mpsc::channel();
    let shared = Shared::new(vec![source], Some(Arc::new(sender)));
    let lifecycle_generation = shared.control().source_lifecycle_generations["progress-source"];

    publish_source_processing_progress(
        &shared,
        &candidate,
        lifecycle_generation,
        SourceDiscoveryStats {
            progress_completed: 313,
            progress_total: 9_985,
            ..SourceDiscoveryStats::default()
        },
    );

    let event = receiver
        .recv_timeout(Duration::from_secs(1))
        .expect("progress event");
    let SourceProcessingEvent::Progress(progress) = event else {
        panic!("unexpected source-processing event: {event:?}");
    };
    assert_eq!(progress.lifecycle.source_id, "progress-source");
    assert_eq!(progress.completed, 313);
    assert_eq!(progress.total, 9_985);
    assert_eq!(
        progress.activity,
        SourceProcessingActivity::Readiness {
            stage: ReadinessStage::EmbeddingAspects,
            relative_path: Some(String::from("drums/kick.wav")),
        }
    );
}

#[test]
fn prerequisite_wait_feedback_preserves_determinate_progress_without_claiming_activity() {
    let directory = tempfile::tempdir().expect("waiting source");
    let source = SampleSource::new_with_id(
        SourceId::from_string("waiting-source"),
        directory.path().to_path_buf(),
    );
    let (sender, receiver) = std::sync::mpsc::channel();
    let shared = Shared::new(vec![source.clone()], Some(Arc::new(sender)));
    let lifecycle_generation = shared.control().source_lifecycle_generations["waiting-source"];
    let lifecycle_generations =
        BTreeMap::from([(String::from("waiting-source"), lifecycle_generation)]);
    let mut source_stats = BTreeMap::from([(
        String::from("waiting-source"),
        SourceDiscoveryStats {
            prerequisites_blocked: 1,
            earliest_retry_at: Some(now_epoch_seconds().saturating_add(60)),
            progress_completed: 72,
            progress_total: 77,
            ..SourceDiscoveryStats::default()
        },
    )]);

    assert!(publish_source_processing_prerequisite_wait(
        &shared,
        &lifecycle_generations,
        &source_stats,
    ));

    let SourceProcessingEvent::Progress(progress) = receiver
        .recv_timeout(Duration::from_secs(1))
        .expect("blocked prerequisite message")
    else {
        panic!("unexpected source-processing event");
    };
    assert_eq!(progress.lifecycle.source_id, "waiting-source");
    assert_eq!(progress.lifecycle.generation, lifecycle_generation);
    assert_eq!(progress.completed, 72);
    assert_eq!(progress.total, 77);
    assert_eq!(
        progress.activity,
        SourceProcessingActivity::WaitingForPrerequisites { retry_at: None }
    );

    source_stats
        .get_mut("waiting-source")
        .expect("waiting source stats")
        .prerequisite_retry_at = Some(now_epoch_seconds().saturating_add(60));
    assert!(publish_source_processing_prerequisite_wait(
        &shared,
        &lifecycle_generations,
        &source_stats,
    ));
    let SourceProcessingEvent::Progress(progress) = receiver
        .recv_timeout(Duration::from_secs(1))
        .expect("retrying prerequisite message")
    else {
        panic!("unexpected source-processing event");
    };
    assert!(matches!(
        progress.activity,
        SourceProcessingActivity::WaitingForPrerequisites { retry_at: Some(_) }
    ));
}

#[test]
fn similarity_blocker_state_ignores_unrelated_retries_and_non_file_targets() {
    let source_id = "dependency-specific-retry";
    let layout = ReadinessTarget::source(
        source_id,
        ReadinessStage::SimilarityLayout,
        "layout-v1",
        1,
        "membership-v1",
    );
    let file_target = |stage| {
        ReadinessTarget::file(
            source_id,
            "identity-1",
            "kick.wav",
            stage,
            "v1",
            1,
            "content-1",
        )
    };
    let mut snapshot = ReadinessSnapshot {
        source_id: source_id.to_string(),
        source_generation: 1,
        readiness_revision: 1,
        availability: SourceAvailability::Active,
        entries: vec![
            wavecrate::sample_sources::readiness::ReadinessEntry {
                target: layout.clone(),
                classification: ReadinessClassification::Pending,
            },
            wavecrate::sample_sources::readiness::ReadinessEntry {
                target: file_target(ReadinessStage::IndexedIdentity),
                classification: ReadinessClassification::Current,
            },
            wavecrate::sample_sources::readiness::ReadinessEntry {
                target: file_target(ReadinessStage::AnalysisFeatures),
                classification: ReadinessClassification::Current,
            },
            wavecrate::sample_sources::readiness::ReadinessEntry {
                target: file_target(ReadinessStage::EmbeddingAspects),
                classification: ReadinessClassification::PermanentFailure {
                    code: String::from("embedding_failed"),
                    reason: String::from("embedding failed permanently"),
                },
            },
            wavecrate::sample_sources::readiness::ReadinessEntry {
                target: {
                    let mut target = file_target(ReadinessStage::AnalysisFeatures);
                    target.source_id = String::from("unrelated-source");
                    target
                },
                classification: ReadinessClassification::RetryableFailure {
                    retry_at: 200,
                    code: String::from("source_retry"),
                    reason: String::from("unrelated source retry"),
                },
            },
            wavecrate::sample_sources::readiness::ReadinessEntry {
                target: ReadinessTarget::source(
                    source_id,
                    ReadinessStage::AnalysisFeatures,
                    "malformed-source-analysis-v1",
                    1,
                    "malformed-source-analysis-generation",
                ),
                classification: ReadinessClassification::Pending,
            },
        ],
        deficits: Vec::new(),
        stage_counts: BTreeMap::new(),
        activity: wavecrate::sample_sources::readiness::ReadinessActivity::Idle,
    };

    assert_eq!(similarity_prerequisite_blocker_stats(&snapshot), (1, None));

    snapshot
        .entries
        .iter_mut()
        .find(|entry| entry.target.stage == ReadinessStage::EmbeddingAspects)
        .expect("embedding blocker")
        .classification = ReadinessClassification::RetryableFailure {
            retry_at: 300,
            code: String::from("embedding_retry"),
            reason: String::from("embedding retry"),
        };
    assert_eq!(
        similarity_prerequisite_blocker_stats(&snapshot),
        (1, Some(300))
    );

    snapshot
        .entries
        .iter_mut()
        .find(|entry| entry.target.stage == ReadinessStage::EmbeddingAspects)
        .expect("embedding blocker")
        .classification = ReadinessClassification::Current;
    assert!(snapshot.prerequisites_are_current(&layout));
    assert_eq!(similarity_prerequisite_blocker_stats(&snapshot), (0, None));
}

#[test]
fn executing_candidate_remains_active_at_discovery_counter_boundary() {
    let directory = tempfile::tempdir().expect("progress source");
    let source = SampleSource::new_with_id(
        SourceId::from_string("boundary-source"),
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
    let shared = Shared::new(vec![source], Some(Arc::new(sender)));
    let lifecycle_generation = shared.control().source_lifecycle_generations["boundary-source"];

    publish_source_processing_progress(
        &shared,
        &candidate,
        lifecycle_generation,
        SourceDiscoveryStats {
            progress_completed: 25_000,
            progress_total: 25_000,
            ..SourceDiscoveryStats::default()
        },
    );

    let SourceProcessingEvent::Progress(progress) = receiver
        .recv_timeout(Duration::from_secs(1))
        .expect("progress event")
    else {
        panic!("unexpected source-processing event");
    };
    assert_eq!(progress.completed, 0);
    assert_eq!(progress.total, 0);
    assert!(matches!(
        progress.activity,
        SourceProcessingActivity::Readiness {
            stage: ReadinessStage::AnalysisFeatures,
            ..
        }
    ));
}
