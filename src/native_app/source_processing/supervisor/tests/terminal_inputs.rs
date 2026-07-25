#[test]
fn zero_byte_audio_is_terminally_non_analyzable_and_never_enters_the_work_queue() {
    let directory = tempfile::tempdir().expect("zero-byte source");
    let source = SampleSource::new_with_id(
        SourceId::from_string("zero-byte-source"),
        directory.path().to_path_buf(),
    );
    let db = source.open_db().expect("open zero-byte source");
    db.upsert_file(Path::new("empty.wav"), 0, 1)
        .expect("insert zero-byte manifest row");
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
                 SET file_identity = 'zero-byte-identity',
                     content_hash = 'zero-byte-content'
                 WHERE path = 'empty.wav'",
            [],
        )
        .expect("assign zero-byte identity");

    assert!(
        publish_current_readiness_targets(&mut connection, source.id.as_str(), 100)
            .expect("publish zero-byte targets")
    );
    let targets = {
        let mut statement = connection
            .prepare(
                "SELECT stage, eligibility
                     FROM source_readiness_targets
                     WHERE source_id = ?1 AND scope_id = 'zero-byte-identity'
                     ORDER BY stage",
            )
            .expect("prepare zero-byte targets");
        statement
            .query_map([source.id.as_str()], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .expect("query zero-byte targets")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect zero-byte targets")
    };
    assert_eq!(
        targets,
        vec![
            (
                String::from("analysis_features"),
                String::from("unsupported")
            ),
            (
                String::from("embedding_aspects"),
                String::from("unsupported")
            ),
            (String::from("indexed_identity"), String::from("eligible")),
        ]
    );

    let snapshot = reconcile_readiness(&connection, source.id.as_str(), 100)
        .expect("reconcile zero-byte targets");
    persist_readiness_deficits(&mut connection, &snapshot.deficits, 100)
        .expect("persist zero-byte deficits");
    let queued_non_analyzable: i64 = connection
        .query_row(
            "SELECT COUNT(*)
                 FROM analysis_jobs
                 WHERE readiness_scope_id = 'zero-byte-identity'
                   AND readiness_stage IN ('analysis_features', 'embedding_aspects')",
            [],
            |row| row.get(0),
        )
        .expect("count zero-byte readiness work");
    assert_eq!(queued_non_analyzable, 0);
}

#[test]
fn hard_link_identity_is_parked_without_retry_until_manifest_revision_changes() {
    let (_directory, source) = unhashed_source("hard-link-identity-terminal");
    let canonical_path = source.root.join("pending.wav");
    let alias_path = source.root.join("alias.wav");
    std::fs::hard_link(&canonical_path, &alias_path).expect("create hard-link alias");
    let db = source.open_db().expect("open hard-link source");
    db.upsert_file(Path::new("alias.wav"), 64, 1)
        .expect("insert hard-link alias manifest row");
    let mut identity_batch = db.write_batch().expect("open hard-link identity batch");
    identity_batch
        .set_file_identity(Path::new("pending.wav"), Some("hard-link-identity"))
        .expect("assign canonical hard-link identity");
    identity_batch
        .set_file_identity(Path::new("alias.wav"), Some("hard-link-identity"))
        .expect("assign alias hard-link identity");
    identity_batch
        .commit()
        .expect("commit shared hard-link identity");
    drop(db);

    let database_root = source.database_root().expect("database root");
    let mut connection = SourceDatabase::open_connection_with_role_and_database_root(
        &source.root,
        &database_root,
        SourceDatabaseConnectionRole::JobWorker,
    )
    .expect("open readiness database");
    let Cancellable::Completed((candidates, stats, health)) =
        discover_source_candidates_with_connection_and_progress(
            &source,
            &mut connection,
            100,
            false,
            false,
            None,
            false,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .expect("classify hard-link identity")
    else {
        panic!("hard-link discovery unexpectedly cancelled");
    };
    assert!(candidates.is_empty());
    assert_eq!(stats.earliest_retry_at, None);
    let health = health.expect("hard-link terminal health");
    assert_eq!(health.retry_at_for_test(), None);
    assert_eq!(health.failure_codes_for_test(), ["duplicate_manifest_identity"]);

    let marker: String = connection
        .query_row(
            "SELECT value FROM metadata WHERE key = ?1",
            [wavecrate_library::sample_sources::db::META_READINESS_DUPLICATE_IDENTITY],
            |row| row.get(0),
        )
        .expect("read duplicate identity marker");
    let marker: serde_json::Value = serde_json::from_str(&marker).expect("decode marker");
    assert_eq!(marker["identity_revision"].as_i64(), Some(4));
    assert_eq!(
        marker["identities"][0]["paths"],
        serde_json::json!(["alias.wav", "pending.wav"])
    );

    let Cancellable::Completed((candidates, stats, health)) =
        discover_source_candidates_with_connection_and_progress(
            &source,
            &mut connection,
            101,
            false,
            false,
            None,
            true,
            &AtomicBool::new(false),
            &mut |_| panic!("unchanged duplicate identity must not run a safety probe"),
        )
        .expect("park unchanged hard-link identity")
    else {
        panic!("hard-link rediscovery unexpectedly cancelled");
    };
    assert!(candidates.is_empty());
    assert_eq!(stats, SourceDiscoveryStats::default());
    assert_eq!(
        health
            .expect("parked terminal health")
            .retry_at_for_test(),
        None
    );

    drop(connection);
    let db = source.open_db().expect("open missing-row repair source");
    db.set_missing(Path::new("alias.wav"), true)
        .expect("mark duplicate alias missing");
    drop(db);
    let mut connection = SourceDatabase::open_connection_with_role_and_database_root(
        &source.root,
        &database_root,
        SourceDatabaseConnectionRole::JobWorker,
    )
    .expect("reopen missing-row repair database");
    let Cancellable::Completed((candidates, _stats, health)) =
        discover_source_candidates_with_connection_and_progress(
            &source,
            &mut connection,
            102,
            false,
            false,
            None,
            false,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .expect("resume after missing-row repair")
    else {
        panic!("missing-row repair discovery unexpectedly cancelled");
    };
    assert!(candidates.iter().any(|candidate| {
        matches!(&candidate.task, RuntimeTask::Readiness(_))
    }));
    assert!(health
        .expect("missing-row repaired source health")
        .failure_codes_for_test()
        .is_empty());

    drop(connection);
    std::fs::remove_file(&alias_path).expect("remove hard-link alias");
    std::fs::write(&alias_path, [2_u8; 64]).expect("replace hard-link alias contents");
    let db = source.open_db().expect("reopen hard-link source");
    let mut identity_batch = db.write_batch().expect("open repaired identity batch");
    identity_batch
        .set_missing(Path::new("alias.wav"), false)
        .expect("restore repaired alias row");
    identity_batch
        .set_file_identity(Path::new("alias.wav"), Some("repaired-identity"))
        .expect("assign repaired alias identity");
    identity_batch
        .commit()
        .expect("commit repaired alias identity");
    drop(db);
    let mut connection = SourceDatabase::open_connection_with_role_and_database_root(
        &source.root,
        &database_root,
        SourceDatabaseConnectionRole::JobWorker,
    )
    .expect("reopen repaired readiness database");
    let Cancellable::Completed((candidates, _stats, health)) =
        discover_source_candidates_with_connection_and_progress(
            &source,
            &mut connection,
            102,
            false,
            false,
            None,
            false,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .expect("resume after hard-link repair")
    else {
        panic!("repaired discovery unexpectedly cancelled");
    };
    assert!(candidates.iter().any(|candidate| {
        matches!(&candidate.task, RuntimeTask::Readiness(_))
    }));
    assert!(health
        .expect("repaired source health")
        .failure_codes_for_test()
        .is_empty());
    let marker_exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM metadata WHERE key = ?1)",
            [wavecrate_library::sample_sources::db::META_READINESS_DUPLICATE_IDENTITY],
            |row| row.get(0),
        )
        .expect("check cleared duplicate identity marker");
    assert!(!marker_exists);
}

#[test]
fn deferred_full_hash_blocks_all_content_derived_targets_until_identity_is_exact() {
    let (_directory, source) = unhashed_source("deferred-full-hash");
    let database_root = source.database_root().expect("database root");
    let mut connection = SourceDatabase::open_connection_with_role_and_database_root(
        &source.root,
        &database_root,
        SourceDatabaseConnectionRole::JobWorker,
    )
    .expect("open readiness database");

    assert!(
        publish_current_readiness_targets(&mut connection, source.id.as_str(), 100)
            .expect("publish pending identity")
    );
    let pending_stages = readiness_stages_for_identity(
        &connection,
        source.id.as_str(),
        "identity-deferred-full-hash",
    );
    assert_eq!(
        pending_stages,
        vec![
            String::from("analysis_features"),
            String::from("embedding_aspects"),
            String::from("indexed_identity"),
        ]
    );
    let pending_content_generations = {
        let mut statement = connection
            .prepare(
                "SELECT DISTINCT content_generation
                     FROM source_readiness_targets
                     WHERE source_id = ?1 AND scope_id = 'identity-deferred-full-hash'",
            )
            .expect("prepare pending content generations");
        statement
            .query_map([source.id.as_str()], |row| row.get::<_, String>(0))
            .expect("query pending content generations")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect pending content generations")
    };
    assert_eq!(pending_content_generations.len(), 1);
    assert!(pending_content_generations[0].starts_with("pending-"));
    let pending_membership: String = connection
        .query_row(
            "SELECT content_generation FROM source_readiness_targets
                 WHERE source_id = ?1 AND scope_kind = 'source'",
            [source.id.as_str()],
            |row| row.get(0),
        )
        .expect("read pending membership");

    connection
        .execute(
            "UPDATE wav_files SET content_hash = 'full-content-hash'
                 WHERE path = 'pending.wav'",
            [],
        )
        .expect("commit full content identity");
    assert!(
        publish_current_readiness_targets(&mut connection, source.id.as_str(), 101)
            .expect("publish full identity")
    );
    let exact_stages = readiness_stages_for_identity(
        &connection,
        source.id.as_str(),
        "identity-deferred-full-hash",
    );
    assert_eq!(
        exact_stages,
        vec![
            String::from("analysis_features"),
            String::from("embedding_aspects"),
            String::from("indexed_identity"),
        ]
    );
    let exact_membership: String = connection
        .query_row(
            "SELECT content_generation FROM source_readiness_targets
                 WHERE source_id = ?1 AND scope_kind = 'source'",
            [source.id.as_str()],
            |row| row.get(0),
        )
        .expect("read exact membership");
    assert_ne!(pending_membership, exact_membership);
}

#[test]
fn unsupported_exact_content_is_terminal_and_excluded_from_similarity_membership() {
    let (_directory, source) = unhashed_source("unsupported-membership");
    let database_root = source.database_root().expect("database root");
    let mut connection = SourceDatabase::open_connection_with_role_and_database_root(
        &source.root,
        &database_root,
        SourceDatabaseConnectionRole::JobWorker,
    )
    .expect("open readiness database");
    connection
        .execute(
            "UPDATE wav_files SET content_hash = 'unsupported-content'
                 WHERE path = 'pending.wav'",
            [],
        )
        .expect("commit unsupported content identity");
    assert!(
        publish_current_readiness_targets(&mut connection, source.id.as_str(), 100)
            .expect("publish exact targets")
    );
    let snapshot =
        reconcile_readiness(&connection, source.id.as_str(), 100).expect("reconcile exact targets");
    persist_readiness_deficits(&mut connection, &snapshot.deficits, 100)
        .expect("persist exact work");
    connection
        .execute(
            "UPDATE analysis_jobs
                 SET status = 'failed', failure_kind = 'unsupported',
                     last_error = 'unsupported codec'
                 WHERE readiness_managed = 1
                   AND readiness_scope_id = 'identity-unsupported-membership'
                   AND readiness_stage = 'analysis_features'",
            [],
        )
        .expect("record terminal unsupported content");

    assert!(
        publish_current_readiness_targets(&mut connection, source.id.as_str(), 101)
            .expect("republish unsupported eligibility")
    );
    let embedding_eligibility: String = connection
        .query_row(
            "SELECT eligibility FROM source_readiness_targets
                 WHERE source_id = ?1
                   AND scope_id = 'identity-unsupported-membership'
                   AND stage = 'embedding_aspects'",
            [source.id.as_str()],
            |row| row.get(0),
        )
        .expect("read terminal embedding eligibility");
    assert_eq!(embedding_eligibility, "unsupported");
    let source_membership: String = connection
        .query_row(
            "SELECT content_generation FROM source_readiness_targets
                 WHERE source_id = ?1 AND scope_kind = 'source'",
            [source.id.as_str()],
            |row| row.get(0),
        )
        .expect("read supported source membership");
    assert_eq!(
        source_membership,
        ReadinessMembership::default().generation()
    );
}

#[test]
fn missing_analysis_payload_requeues_its_prerequisite_without_consuming_a_retry() {
    let (_directory, source) = unhashed_source("missing-analysis-payload");
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
                 SET file_identity = 'missing-payload-identity',
                     content_hash = 'missing-payload-content'
                 WHERE path = 'pending.wav'",
            [],
        )
        .expect("assign readiness identity");
    let now = now_epoch_seconds();
    assert!(
        publish_current_readiness_targets(&mut connection, source.id.as_str(), now)
            .expect("publish current targets")
    );
    let snapshot =
        reconcile_readiness(&connection, source.id.as_str(), now).expect("reconcile targets");
    persist_readiness_deficits(&mut connection, &snapshot.deficits, now)
        .expect("persist readiness work");
    let analysis = snapshot
        .entries
        .iter()
        .find(|entry| entry.target.stage == ReadinessStage::AnalysisFeatures)
        .expect("analysis target")
        .target
        .clone();
    let embedding = snapshot
        .entries
        .iter()
        .find(|entry| entry.target.stage == ReadinessStage::EmbeddingAspects)
        .expect("embedding target")
        .target
        .clone();
    assert_eq!(
        publish_readiness_artifact(
            &mut connection,
            &ReadinessArtifact::for_target(&analysis, now),
        )
        .expect("publish inconsistent analysis marker"),
        ArtifactPublishOutcome::Recorded
    );
    drop(connection);

    let outcome = execute_readiness_target(
        &source,
        &embedding,
        &AtomicBool::new(false),
        &DatabaseWriterGate::default(),
    )
        .expect("repair inconsistent prerequisite");
    assert!(matches!(
        outcome,
        ExecutionOutcome::PrerequisiteInvalidated {
            reason: "analysis prerequisite artifact payload is missing",
            ..
        }
    ));

    let connection = SourceDatabase::open_connection_with_role_and_database_root(
        &source.root,
        &database_root,
        SourceDatabaseConnectionRole::JobWorker,
    )
    .expect("reopen readiness database");
    let repaired =
        reconcile_readiness(&connection, source.id.as_str(), now + 1).expect("repair snapshot");
    let repaired_analysis = repaired
        .entries
        .iter()
        .find(|entry| entry.target.stage == ReadinessStage::AnalysisFeatures)
        .expect("repaired analysis target");
    assert_ne!(
        repaired_analysis.classification,
        wavecrate::sample_sources::readiness::ReadinessClassification::Current
    );
    let stats = readiness_work_stats(&connection, now + 1).expect("repaired work stats");
    assert_eq!(stats.cancelled, 0);
}
