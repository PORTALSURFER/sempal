use std::sync::atomic::AtomicBool;

use rusqlite::{Connection, params};

use super::{ReadinessError, reconcile, work};
use crate::sample_sources::readiness::{
    ReadinessSnapshot, ReadinessStage, ReadinessTarget, ReadinessWorkStats,
};

/// Read-only half of the typed readiness persistence boundary.
pub struct ReadinessView<'connection> {
    connection: &'connection Connection,
}

impl<'connection> ReadinessView<'connection> {
    /// Bind readiness queries to one caller-selected source database connection.
    pub fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    /// Classify durable readiness for one source at a point in time.
    pub fn reconcile(
        &self,
        source_id: &str,
        now: i64,
    ) -> Result<ReadinessSnapshot, ReadinessError> {
        reconcile::reconcile_readiness_inner(self.connection, source_id, now, None, &mut |_| {})
    }

    /// Reconcile with cancellation-safe progress reporting.
    pub fn reconcile_with_cancel_and_progress(
        &self,
        source_id: &str,
        now: i64,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(crate::sample_sources::readiness::ReadinessProgress),
    ) -> Result<ReadinessSnapshot, ReadinessError> {
        reconcile::reconcile_readiness_inner(
            self.connection,
            source_id,
            now,
            Some(cancel),
            progress,
        )
    }

    /// Read queue and lease-recovery telemetry.
    pub fn work_stats(&self, now: i64) -> Result<ReadinessWorkStats, ReadinessError> {
        work::readiness_work_stats(self.connection, now)
    }

    /// Read bounded queue and lease-recovery telemetry for one source.
    pub fn source_work_stats(
        &self,
        source_id: &str,
        now: i64,
    ) -> Result<ReadinessWorkStats, ReadinessError> {
        work::readiness_work_stats_for_source(self.connection, source_id, now)
    }

    /// Check whether a prerequisite stage is terminally unsupported.
    pub fn stage_is_unsupported(
        &self,
        target: &ReadinessTarget,
        stage: ReadinessStage,
    ) -> Result<bool, ReadinessError> {
        self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM analysis_jobs WHERE readiness_managed = 1 AND source_id = ?1 AND readiness_scope_kind = 'file' AND readiness_scope_id = ?2 AND readiness_stage = ?3 AND content_generation = ?4 AND status = 'failed' AND failure_kind = 'unsupported')",
            params![target.source_id, target.scope_id, stage.as_str(), target.content_generation],
            |row| row.get(0),
        ).map_err(Into::into)
    }

    /// Check the exact source-level similarity publication fence.
    pub fn similarity_publication_is_current(
        &self,
        source_id: &str,
        source_generation: i64,
        artifact_version: &str,
        membership_generation: &str,
        manifest_generation: i64,
    ) -> Result<bool, ReadinessError> {
        self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM source_readiness_sources AS source JOIN source_readiness_targets AS target ON target.source_id = source.source_id WHERE source.source_id = ?1 AND source.source_generation = ?2 AND source.availability = 'active' AND target.scope_kind = 'source' AND target.scope_id = ?1 AND target.stage = 'similarity_layout' AND target.required_version = ?3 AND target.source_generation = ?2 AND target.content_generation = ?4 AND target.eligibility = 'eligible' AND ?5 = ?2)",
            params![source_id, source_generation, artifact_version, membership_generation, manifest_generation],
            |row| row.get(0),
        ).map_err(Into::into)
    }
}
