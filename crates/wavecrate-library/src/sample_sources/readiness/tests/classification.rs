use rusqlite::params;

use super::*;

#[test]
fn readiness_classifies_missing_pending_current_and_stale_generations() {
    let (_root, mut connection) = open_fixture();
    let target = file_target("one", ReadinessStage::AnalysisFeatures, 1);
    replace(&mut connection, 1, std::slice::from_ref(&target));

    let missing = reconcile_readiness(&connection, SOURCE_ID, 100).expect("missing snapshot");
    assert_eq!(missing.activity, ReadinessActivity::Actionable);
    assert!(!missing.is_idle());
    assert_eq!(missing.deficits.len(), 1);
    assert_eq!(
        entry_for(&missing, "one", ReadinessStage::AnalysisFeatures).classification,
        ReadinessClassification::Pending
    );

    assert_eq!(
        publish_readiness_artifact(
            &mut connection,
            &ReadinessArtifact::for_target(&target, 101)
        )
        .expect("publish current artifact"),
        ArtifactPublishOutcome::Recorded
    );
    let current = reconcile_readiness(&connection, SOURCE_ID, 102).expect("current snapshot");
    assert_eq!(
        entry_for(&current, "one", ReadinessStage::AnalysisFeatures).classification,
        ReadinessClassification::Current
    );
    assert!(current.is_idle());
    assert!(current.is_fully_ready());

    let changed = file_target("one", ReadinessStage::AnalysisFeatures, 2);
    replace(&mut connection, 2, std::slice::from_ref(&changed));
    let stale = reconcile_readiness(&connection, SOURCE_ID, 103).expect("stale snapshot");
    assert_eq!(
        entry_for(&stale, "one", ReadinessStage::AnalysisFeatures).classification,
        ReadinessClassification::StaleByGeneration
    );
    assert_eq!(stale.deficits.len(), 1);
    assert!(!stale.is_idle());
}

#[test]
fn reconciliation_reads_one_snapshot_during_concurrent_publication() {
    let (root, mut connection) = open_fixture();
    let generation_one = file_target("one", ReadinessStage::AnalysisFeatures, 1);
    replace(&mut connection, 1, std::slice::from_ref(&generation_one));
    let mut writer =
        SourceDatabase::open_connection_for_background_job(root.path()).expect("writer connection");
    let generation_two = file_target("two", ReadinessStage::AnalysisFeatures, 2);

    let snapshot = reconcile_readiness_with_hook(&connection, SOURCE_ID, 10, || {
        replace(&mut writer, 2, std::slice::from_ref(&generation_two));
    })
    .expect("consistent snapshot");
    assert_eq!(snapshot.source_generation, 1);
    assert_eq!(snapshot.readiness_revision, 101);
    assert_eq!(snapshot.entries.len(), 4);
    assert_eq!(
        entry_for(&snapshot, "one", ReadinessStage::AnalysisFeatures)
            .target
            .source_generation,
        1
    );

    let current = reconcile_readiness(&connection, SOURCE_ID, 11).expect("current snapshot");
    assert_eq!(current.source_generation, 2);
    assert_eq!(current.readiness_revision, 102);
    assert_eq!(current.entries.len(), 4);
    assert_eq!(
        entry_for(&current, "two", ReadinessStage::AnalysisFeatures)
            .target
            .scope_id,
        "two"
    );
}

#[test]
fn equal_row_counts_cannot_hide_missing_current_identities() {
    let (_root, mut connection) = open_fixture();
    let targets = [
        file_target("current-a", ReadinessStage::AnalysisFeatures, 4),
        file_target("current-b", ReadinessStage::AnalysisFeatures, 4),
    ];
    replace(&mut connection, 4, &targets);
    for obsolete in ["obsolete-a", "obsolete-b"] {
        connection
            .execute(
                "INSERT INTO source_readiness_artifacts (
                    source_id, scope_kind, scope_id, stage, artifact_version,
                    source_generation, content_generation, completed_at
                 ) VALUES (?1, 'file', ?2, 'analysis_features', 'v1', 4, ?3, 999)",
                params![SOURCE_ID, obsolete, format!("content-{obsolete}-4")],
            )
            .expect("seed obsolete artifact");
    }

    let snapshot = reconcile_readiness(&connection, SOURCE_ID, 1_000).expect("snapshot");

    assert_eq!(snapshot.deficits.len(), 2);
    assert!(
        snapshot
            .entries
            .iter()
            .filter(|entry| entry.target.eligibility == ReadinessEligibility::Eligible)
            .all(|entry| entry.classification == ReadinessClassification::Pending)
    );
    assert!(!snapshot.is_fully_ready());
}

#[test]
fn source_level_similarity_requires_the_exact_membership_generation() {
    let (_root, mut connection) = open_fixture();
    let target = ReadinessTarget::source(
        SOURCE_ID,
        ReadinessStage::SimilarityLayout,
        "layout-v2",
        7,
        "membership-v7",
    );
    replace_readiness_targets(
        &mut connection,
        SOURCE_ID,
        7,
        7,
        SourceAvailability::Active,
        std::slice::from_ref(&target),
        100,
    )
    .unwrap();

    let mut stale = ReadinessArtifact::for_target(&target, 100);
    stale.content_generation = "membership-v6".to_string();
    connection
        .execute(
            "INSERT INTO source_readiness_artifacts (
                source_id, scope_kind, scope_id, stage, artifact_version,
                source_generation, content_generation, completed_at
             ) VALUES (?1, 'source', ?1, 'similarity_layout', ?2, ?3, ?4, ?5)",
            params![
                stale.source_id,
                stale.artifact_version,
                stale.source_generation,
                stale.content_generation,
                stale.completed_at
            ],
        )
        .unwrap();

    let snapshot = reconcile_readiness(&connection, SOURCE_ID, 100).unwrap();
    assert_eq!(
        snapshot.entries[0].classification,
        ReadinessClassification::StaleByGeneration
    );
    assert_eq!(snapshot.deficits.len(), 1);
    assert!(!snapshot.is_fully_ready());
}

#[test]
fn unrelated_manifest_change_preserves_current_file_artifacts() {
    let (_root, mut connection) = open_fixture();
    let unchanged = file_target("unchanged", ReadinessStage::AnalysisFeatures, 1);
    let changed = file_target("changed", ReadinessStage::AnalysisFeatures, 1);
    replace(&mut connection, 1, &[unchanged.clone(), changed.clone()]);
    publish_readiness_artifact(
        &mut connection,
        &ReadinessArtifact::for_target(&unchanged, 100),
    )
    .expect("publish unchanged artifact");

    let mut unchanged_after_manifest_change = unchanged;
    unchanged_after_manifest_change.source_generation = 2;
    let changed_after_manifest_change = file_target("changed", ReadinessStage::AnalysisFeatures, 2);
    replace(
        &mut connection,
        2,
        &[
            unchanged_after_manifest_change,
            changed_after_manifest_change,
        ],
    );

    let snapshot = reconcile_readiness(&connection, SOURCE_ID, 101).expect("snapshot");
    let unchanged_entry = snapshot
        .entries
        .iter()
        .find(|entry| entry.target.scope_id == "unchanged")
        .expect("unchanged entry");
    assert_eq!(
        unchanged_entry.classification,
        ReadinessClassification::Current
    );
    assert_eq!(snapshot.deficits.len(), 1);
    assert_eq!(snapshot.deficits[0].target.scope_id, "changed");
}

#[test]
fn persisted_work_deduplicates_and_survives_restart() {
    let (root, mut connection) = open_fixture();
    let targets = [
        file_target("a", ReadinessStage::AnalysisFeatures, 7),
        file_target("b", ReadinessStage::EmbeddingAspects, 7),
    ];
    replace(&mut connection, 7, &targets);
    let snapshot = reconcile_readiness(&connection, SOURCE_ID, 20).expect("initial snapshot");
    let mut duplicate_deficits = snapshot.deficits.clone();
    duplicate_deficits.extend(snapshot.deficits.clone());

    persist_readiness_deficits(&mut connection, &duplicate_deficits, 20).expect("persist deficits");
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM analysis_jobs WHERE readiness_managed = 1",
            [],
            |row| row.get(0),
        )
        .expect("count jobs");
    assert_eq!(count, 2);
    drop(connection);

    let reopened =
        SourceDatabase::open_connection_for_background_job(root.path()).expect("reopen source db");
    let restarted = reconcile_readiness(&reopened, SOURCE_ID, 21).expect("restart snapshot");
    assert_eq!(restarted.source_generation, 7);
    assert_eq!(restarted.deficits.len(), 2);
    assert!(
        restarted
            .entries
            .iter()
            .filter(|entry| entry.target.eligibility == ReadinessEligibility::Eligible)
            .all(|entry| entry.classification == ReadinessClassification::Pending)
    );
}

#[test]
fn overlapping_manifest_snapshots_preserve_unchanged_file_lease() {
    let (_root, mut connection) = open_fixture();
    let original = file_target("leased", ReadinessStage::AnalysisFeatures, 1);
    replace(&mut connection, 1, std::slice::from_ref(&original));
    let original_snapshot =
        reconcile_readiness(&connection, SOURCE_ID, 10).expect("original snapshot");

    let mut unchanged_after_manifest_change = original;
    unchanged_after_manifest_change.source_generation = 2;
    replace(
        &mut connection,
        2,
        std::slice::from_ref(&unchanged_after_manifest_change),
    );
    let newer_snapshot = reconcile_readiness(&connection, SOURCE_ID, 11).expect("newer snapshot");
    persist_readiness_deficits(&mut connection, &newer_snapshot.deficits, 11)
        .expect("persist newer deficit");
    connection
        .execute(
            "UPDATE analysis_jobs
             SET status = 'running', running_at = 12, lease_expires_at = 50
             WHERE readiness_managed = 1",
            [],
        )
        .expect("claim newer work");

    persist_readiness_deficits(&mut connection, &original_snapshot.deficits, 20)
        .expect("persist overlapping older deficit");
    let (status, running_at, lease_expires_at): (String, Option<i64>, Option<i64>) = connection
        .query_row(
            "SELECT status, running_at, lease_expires_at
             FROM analysis_jobs
             WHERE readiness_managed = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("read preserved lease");
    assert_eq!(status, "running");
    assert_eq!(running_at, Some(12));
    assert_eq!(lease_expires_at, Some(50));
}

#[test]
fn stale_content_deficit_cannot_reset_newer_running_work() {
    let (_root, mut connection) = open_fixture();
    let original = file_target("changed", ReadinessStage::AnalysisFeatures, 1);
    replace(&mut connection, 1, std::slice::from_ref(&original));
    let stale_snapshot = reconcile_readiness(&connection, SOURCE_ID, 10).expect("stale snapshot");

    let current = file_target("changed", ReadinessStage::AnalysisFeatures, 2);
    replace(&mut connection, 2, std::slice::from_ref(&current));
    let current_snapshot =
        reconcile_readiness(&connection, SOURCE_ID, 11).expect("current snapshot");
    persist_readiness_deficits(&mut connection, &current_snapshot.deficits, 11)
        .expect("persist current deficit");
    connection
        .execute(
            "UPDATE analysis_jobs
             SET status = 'running', running_at = 12, lease_expires_at = 50
             WHERE readiness_managed = 1",
            [],
        )
        .expect("claim current work");

    assert_eq!(
        persist_readiness_deficits(&mut connection, &stale_snapshot.deficits, 20)
            .expect("ignore stale deficit"),
        0
    );
    let state: (
        String,
        Option<i64>,
        Option<i64>,
        Option<i64>,
        Option<String>,
    ) = connection
        .query_row(
            "SELECT status, source_generation, running_at, lease_expires_at, content_generation
             FROM analysis_jobs
             WHERE readiness_managed = 1",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .expect("read current work");
    assert_eq!(state.0, "running");
    assert_eq!(state.1, Some(2));
    assert_eq!(state.2, Some(12));
    assert_eq!(state.3, Some(50));
    assert_eq!(state.4.as_deref(), Some("content-changed-2"));
}

#[test]
fn path_only_rename_refreshes_running_work_without_losing_lease() {
    let (_root, mut connection) = open_fixture();
    let original = file_target("renamed", ReadinessStage::AnalysisFeatures, 1);
    replace(&mut connection, 1, std::slice::from_ref(&original));
    let original_snapshot =
        reconcile_readiness(&connection, SOURCE_ID, 10).expect("original snapshot");
    persist_readiness_deficits(&mut connection, &original_snapshot.deficits, 10)
        .expect("persist original work");
    connection
        .execute(
            "UPDATE analysis_jobs
             SET status = 'running', running_at = 11, lease_expires_at = 50
             WHERE readiness_managed = 1",
            [],
        )
        .expect("claim original work");

    let mut renamed = original;
    renamed.source_generation = 2;
    renamed.relative_path = Some("Renamed/current.wav".to_string());
    replace(&mut connection, 2, &[renamed]);
    let renamed_snapshot =
        reconcile_readiness(&connection, SOURCE_ID, 20).expect("renamed snapshot");
    assert!(renamed_snapshot.deficits.is_empty());
    assert_eq!(
        persist_readiness_deficits(&mut connection, &renamed_snapshot.deficits, 20)
            .expect("current snapshot requires no repair"),
        0
    );

    let state: (String, i64, String, Option<i64>, Option<i64>) = connection
        .query_row(
            "SELECT relative_path, source_generation, status, running_at, lease_expires_at
             FROM analysis_jobs
             WHERE readiness_managed = 1",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .expect("read refreshed work");
    assert_eq!(state.0, "Renamed/current.wav");
    assert_eq!(state.1, 2);
    assert_eq!(state.2, "running");
    assert_eq!(state.3, Some(11));
    assert_eq!(state.4, Some(50));
}

#[test]
fn running_lease_expiry_becomes_an_actionable_retry() {
    let (_root, mut connection) = open_fixture();
    let target = file_target("leased", ReadinessStage::AnalysisFeatures, 2);
    replace(&mut connection, 2, std::slice::from_ref(&target));
    let pending = reconcile_readiness(&connection, SOURCE_ID, 10).expect("pending snapshot");
    persist_readiness_deficits(&mut connection, &pending.deficits, 10).expect("persist work");
    connection
        .execute(
            "UPDATE analysis_jobs
             SET status = 'running', running_at = 10, lease_expires_at = 50
             WHERE readiness_managed = 1",
            [],
        )
        .expect("claim work");

    let running = reconcile_readiness(&connection, SOURCE_ID, 49).expect("running snapshot");
    assert_eq!(running.activity, ReadinessActivity::Running);
    assert_eq!(
        entry_for(&running, "leased", ReadinessStage::AnalysisFeatures).classification,
        ReadinessClassification::Running {
            lease_expires_at: 50
        }
    );
    assert!(running.deficits.is_empty());

    let expired = reconcile_readiness(&connection, SOURCE_ID, 50).expect("expired snapshot");
    assert_eq!(expired.activity, ReadinessActivity::Actionable);
    assert_eq!(expired.deficits.len(), 1);
    assert_eq!(
        entry_for(&expired, "leased", ReadinessStage::AnalysisFeatures).classification,
        ReadinessClassification::RetryableFailure {
            retry_at: 50,
            code: "lease_expired".to_string(),
            reason: "lease_expired".to_string(),
        }
    );

    persist_readiness_deficits(&mut connection, &expired.deficits, 50)
        .expect("persist expired lease deficit");
    let (status, running_at, lease_expires_at): (String, Option<i64>, Option<i64>) = connection
        .query_row(
            "SELECT status, running_at, lease_expires_at
             FROM analysis_jobs
             WHERE readiness_managed = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("read reclaimed lease");
    assert_eq!(status, "pending");
    assert_eq!(running_at, None);
    assert_eq!(lease_expires_at, None);
}

#[test]
fn future_retry_waits_and_terminal_states_do_not_spin() {
    let (_root, mut connection) = open_fixture();
    let targets = [
        file_target("retry", ReadinessStage::AnalysisFeatures, 3),
        file_target("permanent", ReadinessStage::AnalysisFeatures, 3),
        file_target("unsupported", ReadinessStage::EmbeddingAspects, 3),
        file_target("deleted", ReadinessStage::AnalysisFeatures, 3)
            .with_eligibility(ReadinessEligibility::Deleted),
    ];
    replace(&mut connection, 3, &targets);
    let initial = reconcile_readiness(&connection, SOURCE_ID, 20).expect("initial snapshot");
    persist_readiness_deficits(&mut connection, &initial.deficits, 20).expect("persist work");
    connection
        .execute(
            "UPDATE analysis_jobs
             SET status = 'failed', failure_kind = 'retryable', retry_at = 40,
                 failure_code = 'sqlite_busy', last_error = 'database busy'
             WHERE readiness_scope_id = 'retry'",
            [],
        )
        .expect("seed retryable failure");
    connection
        .execute(
            "UPDATE analysis_jobs SET status = 'failed', failure_kind = 'permanent',
                 failure_code = 'malformed_audio', last_error = 'malformed audio'
             WHERE readiness_scope_id = 'permanent'",
            [],
        )
        .expect("seed permanent failure");
    connection
        .execute(
            "UPDATE analysis_jobs SET status = 'failed', failure_kind = 'unsupported',
                 failure_code = 'decoder_unsupported', last_error = 'unsupported format'
             WHERE readiness_scope_id = 'unsupported'",
            [],
        )
        .expect("seed unsupported failure");

    let waiting = reconcile_readiness(&connection, SOURCE_ID, 30).expect("waiting snapshot");
    assert_eq!(waiting.activity, ReadinessActivity::WaitingForRetry);
    assert!(waiting.deficits.is_empty());
    assert!(waiting.entries.iter().any(|entry| matches!(
        &entry.classification,
        ReadinessClassification::PermanentFailure { code, .. } if code == "malformed_audio"
    )));
    assert!(waiting.entries.iter().any(|entry| matches!(
        &entry.classification,
        ReadinessClassification::RetryableFailure { code, .. } if code == "sqlite_busy"
    )));
    assert!(!waiting.is_converged());
    assert!(!waiting.is_fully_ready());
    assert!(waiting.entries.iter().any(|entry| {
        entry.target.scope_id == "unsupported"
            && entry.classification
                == ReadinessClassification::Unsupported {
                    code: Some("decoder_unsupported".to_string()),
                }
    }));
    assert!(waiting.entries.iter().any(|entry| {
        entry.target.scope_id == "deleted"
            && entry.classification == ReadinessClassification::Deleted
    }));

    let due = reconcile_readiness(&connection, SOURCE_ID, 40).expect("due snapshot");
    assert_eq!(due.activity, ReadinessActivity::Actionable);
    assert_eq!(due.deficits.len(), 1);
    assert_eq!(due.deficits[0].target.scope_id, "retry");

    connection
        .execute(
            "UPDATE analysis_jobs SET failure_kind = 'permanent', retry_at = NULL
             WHERE readiness_scope_id = 'retry'",
            [],
        )
        .expect("make final failure terminal");
    let terminal = reconcile_readiness(&connection, SOURCE_ID, 41).expect("terminal snapshot");
    assert_eq!(terminal.activity, ReadinessActivity::Idle);
    assert!(terminal.deficits.is_empty());
    assert!(terminal.is_converged());
    assert!(!terminal.is_fully_ready());
}

#[test]
fn offline_and_unsupported_targets_remain_observable_without_work() {
    let (_root, mut connection) = open_fixture();
    let unsupported = file_target("unsupported", ReadinessStage::AnalysisFeatures, 1)
        .with_eligibility(ReadinessEligibility::Unsupported);
    let complete = complete_targets(1, std::slice::from_ref(&unsupported));
    sync_manifest(&connection, &complete);
    replace_readiness_targets(
        &mut connection,
        SOURCE_ID,
        1,
        1,
        SourceAvailability::Offline,
        &complete,
        1,
    )
    .expect("replace offline source");

    let offline = reconcile_readiness(&connection, SOURCE_ID, 2).expect("offline snapshot");
    assert_eq!(
        entry_for(&offline, "unsupported", ReadinessStage::AnalysisFeatures).classification,
        ReadinessClassification::Offline
    );
    assert!(offline.deficits.is_empty());
    assert!(offline.is_idle());
    assert!(!offline.is_fully_ready());

    replace(&mut connection, 1, &[unsupported]);
    let active = reconcile_readiness(&connection, SOURCE_ID, 3).expect("active snapshot");
    assert_eq!(
        entry_for(&active, "unsupported", ReadinessStage::AnalysisFeatures).classification,
        ReadinessClassification::Unsupported { code: None }
    );
    assert!(active.deficits.is_empty());
    assert!(active.is_fully_ready());
}
