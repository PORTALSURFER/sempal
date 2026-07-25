mod error;
mod persistence;
mod reconcile;
mod similarity;
mod view;
mod work;

pub use error::ReadinessError;
pub use similarity::{
    ReadinessEmbeddingArtifactTarget, ReadinessSimilarityManifest,
    ReadinessSimilarityManifestRequest, ReadinessSimilarityManifestRow,
    ReadinessSimilarityPayloadContract,
};
use std::{collections::BTreeSet, sync::atomic::AtomicBool};
pub use view::ReadinessView;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use super::{
    ClaimedReadinessWork, ReadinessArtifact, ReadinessDeficit, ReadinessFailureClassification,
    ReadinessFailureOutcome, ReadinessLeaseRenewalOutcome, ReadinessMembership,
    ReadinessRetryPolicy, ReadinessSnapshot, ReadinessStage, ReadinessTarget,
    ReadinessWorkMutationOutcome, ReadinessWorkStats, SourceAvailability,
};
use crate::sample_sources::readiness::ArtifactPublishOutcome;

/// Typed request for atomically publishing a source's complete desired readiness set.
///
/// Keeping the publication inputs together makes the generation, revision, availability, and
/// target set one durable operation instead of a convention spread across native orchestration.
#[derive(Debug)]
pub struct ReadinessTargetPublication<'a> {
    source_id: &'a str,
    source_generation: i64,
    readiness_revision: i64,
    availability: SourceAvailability,
    contract_version: &'a str,
    targets: &'a [ReadinessTarget],
    updated_at: i64,
}

impl<'a> ReadinessTargetPublication<'a> {
    /// Build one complete readiness publication for a source generation.
    pub fn new(
        source_id: &'a str,
        source_generation: i64,
        readiness_revision: i64,
        availability: SourceAvailability,
        contract_version: &'a str,
        targets: &'a [ReadinessTarget],
        updated_at: i64,
    ) -> Self {
        Self {
            source_id,
            source_generation,
            readiness_revision,
            availability,
            contract_version,
            targets,
            updated_at,
        }
    }
}

/// Typed request for atomically applying a bounded readiness target delta.
#[derive(Debug)]
pub struct ReadinessTargetDeltaPublication<'a> {
    source_id: &'a str,
    source_generation: i64,
    readiness_revision: i64,
    availability: SourceAvailability,
    contract_version: &'a str,
    upsert_targets: &'a [ReadinessTarget],
    deleted_file_scope_ids: &'a [String],
    similarity_artifact_version: &'a str,
    updated_at: i64,
}

impl<'a> ReadinessTargetDeltaPublication<'a> {
    /// Build one revision-fenced delta publication.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        source_id: &'a str,
        source_generation: i64,
        readiness_revision: i64,
        availability: SourceAvailability,
        contract_version: &'a str,
        upsert_targets: &'a [ReadinessTarget],
        deleted_file_scope_ids: &'a [String],
        similarity_artifact_version: &'a str,
        updated_at: i64,
    ) -> Self {
        Self {
            source_id,
            source_generation,
            readiness_revision,
            availability,
            contract_version,
            upsert_targets,
            deleted_file_scope_ids,
            similarity_artifact_version,
            updated_at,
        }
    }
}

/// Result of attempting a bounded target delta.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadinessDeltaPublicationOutcome {
    /// The delta committed atomically.
    Applied {
        /// Current source-scoped membership generation after the delta.
        membership_generation: String,
        /// Number of durable target, job, and source rows changed.
        changed: usize,
    },
    /// Durable state was absent or incompatible, so a complete publication is required.
    RequiresFullPublication,
}

/// The only production persistence boundary for durable source readiness.
///
/// The repository borrows one source-database connection, owns readiness transactions, and
/// exposes domain operations rather than readiness table access.
pub struct ReadinessStore<'connection> {
    connection: &'connection mut Connection,
}

/// The result of an idempotent compatibility cleanup after its transaction commits.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ReadinessCompatibilityCleanup {
    /// Number of obsolete readiness rows removed.
    pub changed: usize,
    /// Cache payloads no longer referenced by the retired readiness rows.
    pub retired_artifact_refs: Vec<String>,
}

/// Durable readiness publication state for one configured source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadinessSourceState {
    /// Last accepted manifest generation.
    pub source_generation: i64,
    /// Last accepted readiness publication revision.
    pub readiness_revision: i64,
    /// Current processing availability.
    pub availability: SourceAvailability,
    /// Artifact/readiness contract that owns the current target set.
    pub contract_version: String,
    /// Incremental source similarity membership checkpoint.
    pub membership: ReadinessMembership,
}

impl<'connection> ReadinessStore<'connection> {
    /// Bind readiness persistence to one caller-selected source database connection.
    pub fn new(connection: &'connection mut Connection) -> Self {
        Self { connection }
    }

    /// Atomically publish one complete, validated target generation.
    pub fn publish_targets(
        &mut self,
        publication: &ReadinessTargetPublication<'_>,
    ) -> Result<(), ReadinessError> {
        persistence::replace_readiness_targets_inner(
            self.connection,
            publication.source_id,
            publication.source_generation,
            publication.readiness_revision,
            publication.availability,
            publication.contract_version,
            publication.targets,
            publication.updated_at,
            None,
        )
    }

    /// Publish targets while rolling back if cancellation is observed.
    pub fn publish_targets_with_cancel(
        &mut self,
        publication: &ReadinessTargetPublication<'_>,
        cancel: &AtomicBool,
    ) -> Result<(), ReadinessError> {
        persistence::replace_readiness_targets_inner(
            self.connection,
            publication.source_id,
            publication.source_generation,
            publication.readiness_revision,
            publication.availability,
            publication.contract_version,
            publication.targets,
            publication.updated_at,
            Some(cancel),
        )
    }

    /// Apply a bounded readiness target delta or request a complete fallback publication.
    pub fn publish_target_delta_with_cancel(
        &mut self,
        publication: &ReadinessTargetDeltaPublication<'_>,
        cancel: &AtomicBool,
    ) -> Result<ReadinessDeltaPublicationOutcome, ReadinessError> {
        persistence::apply_readiness_target_delta_inner(
            self.connection,
            publication.source_id,
            publication.source_generation,
            publication.readiness_revision,
            publication.availability,
            publication.contract_version,
            publication.upsert_targets,
            publication.deleted_file_scope_ids,
            publication.similarity_artifact_version,
            publication.updated_at,
            Some(cancel),
        )
    }

    /// Classify durable readiness for one source at a point in time.
    pub fn reconcile(
        &mut self,
        source_id: &str,
        now: i64,
    ) -> Result<ReadinessSnapshot, ReadinessError> {
        reconcile::reconcile_readiness_inner(self.connection, source_id, now, None, &mut |_| {})
    }

    /// Reconcile a source while reporting checkpoints and honoring cancellation.
    pub fn reconcile_with_cancel_and_progress(
        &mut self,
        source_id: &str,
        now: i64,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(super::ReadinessProgress),
    ) -> Result<ReadinessSnapshot, ReadinessError> {
        reconcile::reconcile_readiness_inner(
            self.connection,
            source_id,
            now,
            Some(cancel),
            progress,
        )
    }

    /// Reconcile only committed file identities plus the source-scoped similarity target.
    ///
    /// This is valid after an atomic readiness delta publication: unchanged file targets retain
    /// their exact artifacts and work, while the source target is always included because its
    /// membership generation changes with the affected identities.
    pub fn reconcile_scopes_with_cancel_and_progress(
        &mut self,
        source_id: &str,
        scope_ids: &BTreeSet<String>,
        now: i64,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(super::ReadinessProgress),
    ) -> Result<ReadinessSnapshot, ReadinessError> {
        reconcile::reconcile_readiness_scopes_inner(
            self.connection,
            source_id,
            scope_ids,
            now,
            Some(cancel),
            progress,
        )
    }

    /// Atomically enqueue actionable deficits.
    pub fn persist_deficits(
        &mut self,
        deficits: &[ReadinessDeficit],
        created_at: i64,
    ) -> Result<usize, ReadinessError> {
        persistence::persist_readiness_deficits_inner(
            self.connection,
            deficits,
            created_at,
            None,
            &mut |_| {},
        )
    }

    /// Enqueue deficits with cancellation-safe progress reporting.
    pub fn persist_deficits_with_cancel_and_progress(
        &mut self,
        deficits: &[ReadinessDeficit],
        created_at: i64,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(super::ReadinessProgress),
    ) -> Result<usize, ReadinessError> {
        persistence::persist_readiness_deficits_inner(
            self.connection,
            deficits,
            created_at,
            Some(cancel),
            progress,
        )
    }

    /// Claim one current target under a durable lease.
    pub fn claim(
        &mut self,
        target: &ReadinessTarget,
        now: i64,
        lease_duration_seconds: i64,
    ) -> Result<Option<ClaimedReadinessWork>, ReadinessError> {
        work::claim_readiness_target(self.connection, target, now, lease_duration_seconds)
    }
    /// Renew an active claim without changing its generation.
    pub fn renew_lease(
        &mut self,
        claim: &ClaimedReadinessWork,
        now: i64,
        lease_duration_seconds: i64,
    ) -> Result<ReadinessLeaseRenewalOutcome, ReadinessError> {
        work::renew_readiness_lease(self.connection, claim, now, lease_duration_seconds)
    }
    /// Publish a claimed completion without an external artifact reference.
    pub fn complete(
        &mut self,
        claim: &ClaimedReadinessWork,
        completed_at: i64,
    ) -> Result<ArtifactPublishOutcome, ReadinessError> {
        work::complete_readiness_work(self.connection, claim, completed_at)
    }
    /// Publish a claimed completion with its durable artifact reference.
    pub fn complete_with_artifact_ref(
        &mut self,
        claim: &ClaimedReadinessWork,
        completed_at: i64,
        artifact_ref: &str,
    ) -> Result<ArtifactPublishOutcome, ReadinessError> {
        work::complete_readiness_work_with_artifact_ref(
            self.connection,
            claim,
            completed_at,
            artifact_ref,
        )
    }
    /// Record a classified failure for a current claim.
    pub fn fail(
        &mut self,
        claim: &ClaimedReadinessWork,
        classification: ReadinessFailureClassification,
        code: &str,
        reason: &str,
        failed_at: i64,
        retry_policy: ReadinessRetryPolicy,
    ) -> Result<ReadinessFailureOutcome, ReadinessError> {
        work::fail_readiness_work(
            self.connection,
            claim,
            classification,
            code,
            reason,
            failed_at,
            retry_policy,
        )
    }
    /// Return a current claim to pending work.
    pub fn release(
        &mut self,
        claim: &ClaimedReadinessWork,
        released_at: i64,
    ) -> Result<ReadinessWorkMutationOutcome, ReadinessError> {
        work::release_readiness_work(self.connection, claim, released_at)
    }
    /// Return a current claim to pending work with a cancellation diagnostic.
    pub fn cancel(
        &mut self,
        claim: &ClaimedReadinessWork,
        reason: &str,
        cancelled_at: i64,
    ) -> Result<ReadinessWorkMutationOutcome, ReadinessError> {
        work::cancel_readiness_work(self.connection, claim, reason, cancelled_at)
    }
    /// Invalidate an exact artifact only when its target remains current.
    pub fn invalidate_artifact(
        &mut self,
        target: &ReadinessTarget,
    ) -> Result<bool, ReadinessError> {
        persistence::invalidate_readiness_artifact(self.connection, target)
    }

    /// Invalidate source-owned analysis outputs and requeue their exact current
    /// readiness targets for an explicit asynchronous reanalysis request.
    pub fn requeue_source_analysis(
        &mut self,
        source_id: &str,
        requested_at: i64,
    ) -> Result<usize, ReadinessError> {
        persistence::requeue_source_analysis(self.connection, source_id, requested_at)
    }
    /// Load queue and lease-recovery telemetry for the current durable work set.
    pub fn work_stats(&mut self, now: i64) -> Result<ReadinessWorkStats, ReadinessError> {
        work::readiness_work_stats(self.connection, now)
    }

    /// Load bounded queue and lease-recovery telemetry for one source.
    pub fn source_work_stats(
        &mut self,
        source_id: &str,
        now: i64,
    ) -> Result<ReadinessWorkStats, ReadinessError> {
        work::readiness_work_stats_for_source(self.connection, source_id, now)
    }

    /// Publish an independently produced artifact if its target is still current.
    pub fn publish_artifact(
        &mut self,
        artifact: &ReadinessArtifact,
    ) -> Result<ArtifactPublishOutcome, ReadinessError> {
        persistence::publish_readiness_artifact(self.connection, artifact)
    }

    /// Whether the readiness schema required for source processing is present.
    pub fn schema_available(&mut self) -> Result<bool, ReadinessError> {
        self.connection.query_row(
            "SELECT COUNT(*) = 3 FROM sqlite_master WHERE type = 'table' AND name IN ('source_readiness_sources', 'source_readiness_targets', 'source_readiness_artifacts')",
            [], |row| row.get(0),
        ).map_err(Into::into)
    }

    /// Whether the source database has every readiness-owned table and work column.
    pub fn processing_schema_available(&mut self) -> Result<bool, ReadinessError> {
        for (table, columns) in [
            (
                "analysis_jobs",
                &[
                    "id",
                    "relative_path",
                    "job_type",
                    "created_at",
                    "status",
                    "readiness_managed",
                ][..],
            ),
            (
                "source_readiness_artifacts",
                &[
                    "source_id",
                    "scope_kind",
                    "scope_id",
                    "relative_path",
                    "stage",
                    "artifact_version",
                    "content_generation",
                    "artifact_ref",
                ][..],
            ),
            (
                "source_readiness_sources",
                &[
                    "source_id",
                    "source_generation",
                    "readiness_revision",
                    "availability",
                    "contract_version",
                    "membership_digest",
                    "membership_count",
                ][..],
            ),
        ] {
            let pragma = format!("PRAGMA table_info({table})");
            let mut statement = self.connection.prepare(&pragma)?;
            let found = statement
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<Result<BTreeSet<_>, _>>()?;
            if columns.iter().any(|column| !found.contains(*column)) {
                return Ok(false);
            }
        }
        self.schema_available()
    }

    /// Whether one source has durable readiness state.
    pub fn source_exists(&mut self, source_id: &str) -> Result<bool, ReadinessError> {
        self.connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM source_readiness_sources WHERE source_id = ?1)",
                [source_id],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    /// Whether an exact file generation is already known to be permanently unsupported.
    pub fn generation_is_known_unsupported(
        &mut self,
        source_id: &str,
        scope_id: &str,
        content_generation: &str,
    ) -> Result<bool, ReadinessError> {
        work::readiness_generation_is_known_unsupported(
            self.connection,
            source_id,
            scope_id,
            content_generation,
        )
    }

    /// Load the state required to decide whether a complete target publication is still current.
    pub fn source_state(
        &mut self,
        source_id: &str,
    ) -> Result<Option<ReadinessSourceState>, ReadinessError> {
        self.connection
            .query_row(
                "SELECT source_generation, readiness_revision, availability, contract_version,
                    membership_digest, membership_count
             FROM source_readiness_sources
             WHERE source_id = ?1",
                [source_id],
                |row| {
                    let availability = row.get::<_, String>(2)?;
                    let availability = SourceAvailability::from_stored(&availability)
                        .ok_or_else(|| rusqlite::Error::InvalidQuery)?;
                    let raw_digest = row.get::<_, Vec<u8>>(4)?;
                    let raw_count = row.get::<_, i64>(5)?;
                    let mut contract_version = row.get::<_, String>(3)?;
                    let digest = match raw_digest.try_into() {
                        Ok(digest) if raw_count >= 0 => digest,
                        _ => {
                            // A corrupt or legacy checkpoint must force the next probe through
                            // complete publication instead of making the source unrecoverable.
                            contract_version.clear();
                            [0; 32]
                        }
                    };
                    Ok(ReadinessSourceState {
                        source_generation: row.get(0)?,
                        readiness_revision: row.get(1)?,
                        availability,
                        contract_version,
                        membership: ReadinessMembership::from_parts(
                            digest,
                            raw_count.max(0) as u64,
                        ),
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    /// Mark a source offline without modifying its generation or target set.
    pub fn mark_temporarily_unavailable(
        &mut self,
        source_id: &str,
        now: i64,
    ) -> Result<(), ReadinessError> {
        self.connection.execute(
            "UPDATE source_readiness_sources SET availability = 'offline', readiness_revision = readiness_revision + 1, updated_at = ?2 WHERE source_id = ?1 AND availability != 'offline'",
            params![source_id, now],
        )?;
        Ok(())
    }

    /// Drop queued readiness work for files that are still being actively recorded.
    pub fn defer_active_recordings(
        &mut self,
        scope_ids: &BTreeSet<String>,
    ) -> Result<(), ReadinessError> {
        for scope_id in scope_ids {
            self.connection.execute(
                "DELETE FROM analysis_jobs WHERE readiness_managed = 1 AND readiness_scope_id = ?1 AND status != 'running'",
                [scope_id],
            )?;
        }
        Ok(())
    }

    /// Return file content generations that have durably failed as unsupported.
    pub fn unsupported_content_generations(
        &mut self,
        source_id: &str,
    ) -> Result<BTreeSet<(String, String)>, ReadinessError> {
        let mut statement = self.connection.prepare(
            "SELECT DISTINCT readiness_scope_id, content_generation FROM analysis_jobs WHERE source_id = ?1 AND readiness_managed = 1 AND readiness_scope_kind = 'file' AND status = 'failed' AND failure_kind = 'unsupported' AND readiness_scope_id IS NOT NULL AND content_generation IS NOT NULL",
        )?;
        statement
            .query_map([source_id], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<BTreeSet<_>, _>>()
            .map_err(Into::into)
    }

    /// Check whether a prerequisite stage is terminally unsupported for a file generation.
    pub fn stage_is_unsupported(
        &mut self,
        target: &ReadinessTarget,
        stage: ReadinessStage,
    ) -> Result<bool, ReadinessError> {
        ReadinessView::new(self.connection).stage_is_unsupported(target, stage)
    }

    /// Upgrade deterministic legacy decode failures to the terminal unsupported classification.
    pub fn reclassify_known_unsupported_failures(
        &mut self,
        is_unsupported: impl Fn(&str) -> bool,
    ) -> Result<usize, ReadinessError> {
        let failures = {
            let mut statement = self.connection.prepare(
                "SELECT id, COALESCE(last_error, '') FROM analysis_jobs WHERE readiness_managed = 1 AND status = 'failed' AND failure_code IS NULL AND failure_kind IN ('retryable', 'permanent')",
            )?;
            statement
                .query_map([], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut changed = 0;
        for id in failures
            .into_iter()
            .filter_map(|(id, error)| is_unsupported(&error).then_some(id))
        {
            changed += tx.execute(
                "UPDATE analysis_jobs SET failure_kind = 'unsupported', failure_code = 'legacy_decoder_unsupported', retry_at = NULL WHERE id = ?1 AND readiness_managed = 1 AND status = 'failed' AND failure_code IS NULL AND failure_kind IN ('retryable', 'permanent')", [id],
            )?;
        }
        changed += tx.execute(
            "UPDATE analysis_jobs AS dependent SET failure_kind = 'unsupported', failure_code = 'legacy_prerequisite_unsupported', retry_at = NULL WHERE dependent.readiness_managed = 1 AND dependent.status = 'failed' AND dependent.failure_code IS NULL AND dependent.failure_kind IN ('retryable', 'permanent') AND dependent.readiness_stage = 'embedding_aspects' AND dependent.last_error = 'embedding feature prerequisite is not durable yet' AND EXISTS (SELECT 1 FROM analysis_jobs AS prerequisite WHERE prerequisite.readiness_managed = 1 AND prerequisite.source_id = dependent.source_id AND prerequisite.readiness_scope_kind = dependent.readiness_scope_kind AND prerequisite.readiness_scope_id = dependent.readiness_scope_id AND prerequisite.readiness_stage = 'analysis_features' AND prerequisite.content_generation = dependent.content_generation AND prerequisite.status = 'failed' AND prerequisite.failure_kind = 'unsupported')", [],
        )?;
        tx.commit()?;
        Ok(changed)
    }

    /// Retire rows belonging to the removed playback-summary stage, atomically collecting cache refs.
    pub fn retire_legacy_playback(
        &mut self,
        source_id: &str,
    ) -> Result<ReadinessCompatibilityCleanup, ReadinessError> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let refs = {
            let mut statement = tx.prepare("SELECT artifact_ref FROM source_readiness_artifacts WHERE source_id = ?1 AND stage = 'playback_summary' AND artifact_ref IS NOT NULL AND length(trim(artifact_ref)) > 0")?;
            statement
                .query_map([source_id], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        let mut changed = tx.execute("DELETE FROM analysis_jobs WHERE source_id = ?1 AND readiness_managed = 1 AND readiness_stage = 'playback_summary'", [source_id])?;
        changed += tx.execute("DELETE FROM source_readiness_artifacts WHERE source_id = ?1 AND stage = 'playback_summary'", [source_id])?;
        changed += tx.execute("DELETE FROM source_readiness_targets WHERE source_id = ?1 AND stage = 'playback_summary'", [source_id])?;
        tx.commit()?;
        Ok(ReadinessCompatibilityCleanup {
            changed,
            retired_artifact_refs: refs,
        })
    }

    /// Disable a retired source and delete all readiness-owned work, retaining cache cleanup data.
    pub fn retire_source(
        &mut self,
        source_id: &str,
        now: i64,
    ) -> Result<ReadinessCompatibilityCleanup, ReadinessError> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let refs = {
            let mut statement = tx.prepare("SELECT artifact_ref FROM source_readiness_artifacts WHERE source_id = ?1 AND stage = 'playback_summary' AND artifact_ref IS NOT NULL AND length(trim(artifact_ref)) > 0")?;
            statement
                .query_map([source_id], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        tx.execute("UPDATE source_readiness_sources SET availability = 'disabled', readiness_revision = readiness_revision + 1, updated_at = ?2 WHERE source_id = ?1 AND availability != 'disabled'", params![source_id, now])?;
        let mut changed = tx.execute(
            "DELETE FROM analysis_jobs WHERE source_id = ?1 AND readiness_managed = 1",
            [source_id],
        )?;
        changed += tx.execute("DELETE FROM source_readiness_artifacts WHERE source_id = ?1 AND stage = 'playback_summary'", [source_id])?;
        changed += tx.execute("DELETE FROM source_readiness_targets WHERE source_id = ?1 AND stage = 'playback_summary'", [source_id])?;
        tx.commit()?;
        Ok(ReadinessCompatibilityCleanup {
            changed,
            retired_artifact_refs: refs,
        })
    }

    /// Return legacy playback cache references for bounded app-global cache collection.
    pub fn legacy_playback_artifact_refs(&mut self) -> Result<Vec<String>, ReadinessError> {
        if !self.schema_available()? {
            return Ok(Vec::new());
        }
        let mut statement = self.connection.prepare(
            "SELECT artifact_ref FROM source_readiness_artifacts WHERE stage = 'playback_summary' AND artifact_ref IS NOT NULL AND length(trim(artifact_ref)) > 0",
        )?;
        statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Check legacy playback cache ownership without exposing the artifact table to callers.
    pub fn legacy_playback_artifact_ref_is_owned(
        &mut self,
        artifact_ref: &str,
    ) -> Result<bool, ReadinessError> {
        if !self.schema_available()? {
            return Ok(false);
        }
        self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM source_readiness_artifacts WHERE stage = 'playback_summary' AND artifact_ref = ?1)",
            [artifact_ref], |row| row.get(0),
        ).map_err(Into::into)
    }
}

#[cfg(test)]
pub(crate) use persistence::{
    invalidate_readiness_artifact, persist_readiness_deficits, publish_readiness_artifact,
    replace_readiness_targets,
};
#[cfg(test)]
pub(crate) use reconcile::reconcile_readiness;
#[cfg(test)]
pub(crate) use reconcile::reconcile_readiness_with_hook;
#[cfg(test)]
pub(crate) use work::{
    cancel_readiness_work, claim_readiness_target, complete_readiness_work,
    complete_readiness_work_with_artifact_ref, fail_readiness_work, readiness_work_stats,
    release_readiness_work, renew_readiness_lease,
};
