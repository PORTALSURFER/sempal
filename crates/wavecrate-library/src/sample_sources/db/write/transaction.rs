use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::OptionalExtension;

use super::super::util::{map_sql_error, normalize_relative_path};
use super::super::{
    META_SOURCE_TRAVERSAL_POLICY, SourceDatabase, SourceDbError, SourceDirectoryEntry,
    SourceDirectoryTruthError, SourceDirectoryTruthPublication, SourceIndexEntry,
    SourceManifestEntry, SourceTraversalPolicy, SourceWriteBatch,
};
use crate::sample_sources::reconciliation::{RootIdentity, SourceAuditCommit, SourceAuditRequest};

/// Manifest state published by a committed source-database write batch.
pub struct ManifestCommitResult {
    /// Revision assigned to the committed manifest state.
    pub revision: u64,
    /// Manifest rows for paths touched by this batch when the cached revision was current.
    pub touched_path_changes: Vec<(PathBuf, Option<SourceManifestEntry>)>,
    /// Complete manifest captured in the committing transaction when the cached revision was stale.
    pub authoritative_snapshot: Option<Vec<SourceManifestEntry>>,
    /// Index-only facts committed by this same transaction, when any changed.
    pub source_index_commit: Option<SourceIndexCommitResult>,
}

/// Typed index-only facts proven by one committed source-database write.
pub struct SourceIndexCommitResult {
    /// Generic source revision assigned by the committing transaction.
    pub source_revision: u64,
    /// Index-only revision assigned by the committing transaction.
    pub index_revision: u64,
    /// Final typed index rows upserted by the transaction.
    pub upserted_entries: Vec<SourceIndexEntry>,
    /// Index rows removed by the transaction.
    pub removed_paths: Vec<PathBuf>,
}

impl SourceWriteBatch<'_> {
    pub(crate) fn begin_source_directory_truth_generation(
        &mut self,
        generation: u64,
        expected_entry_count: u64,
    ) -> Result<(), SourceDbError> {
        let generation = directory_generation_sql_value(generation)?;
        let expected_entry_count = directory_entry_count_sql_value(expected_entry_count)?;
        let existing = self
            .tx
            .query_row(
                "SELECT status, expected_entry_count
                 FROM source_directory_generations
                 WHERE generation = ?1",
                [generation],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()
            .map_err(map_sql_error)?;

        if let Some((status, existing_expected)) = existing {
            if status == "staging" && existing_expected == expected_entry_count {
                return Ok(());
            }
            return Err(SourceDirectoryTruthError::GenerationCollision {
                generation: u64::try_from(generation).unwrap_or_default(),
            }
            .into());
        }

        let complete = i64::from(expected_entry_count == 0);
        self.tx
            .execute(
                "INSERT INTO source_directory_generations (
                    generation, status, expected_entry_count, staged_entry_count, complete,
                    published_source_revision, created_at
                 ) VALUES (?1, 'staging', ?2, 0, ?3, NULL, ?4)",
                rusqlite::params![generation, expected_entry_count, complete, unix_timestamp()],
            )
            .map_err(map_sql_error)?;
        Ok(())
    }

    pub(crate) fn stage_source_directory_truth_entries(
        &mut self,
        generation: u64,
        entries: &[SourceDirectoryEntry],
    ) -> Result<(), SourceDbError> {
        if entries.len() > super::super::SOURCE_DIRECTORY_TRUTH_BATCH_LIMIT {
            return Err(SourceDirectoryTruthError::BatchTooLarge.into());
        }
        let generation_sql = directory_generation_sql_value(generation)?;
        let Some((status, expected_entry_count, staged_entry_count, complete)) = self
            .tx
            .query_row(
                "SELECT status, expected_entry_count, staged_entry_count, complete
                 FROM source_directory_generations
                 WHERE generation = ?1",
                [generation_sql],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(map_sql_error)?
        else {
            return Err(SourceDirectoryTruthError::GenerationMissing { generation }.into());
        };
        if status != "staging" {
            return Err(SourceDirectoryTruthError::GenerationCollision { generation }.into());
        }
        if expected_entry_count < 0 || staged_entry_count < 0 || complete < 0 {
            return Err(directory_requires_audit());
        }
        if staged_entry_count > expected_entry_count {
            return Err(SourceDirectoryTruthError::EntryCountExceeded { generation }.into());
        }
        let actual_entry_count = self
            .tx
            .query_row(
                "SELECT COUNT(*)
                 FROM source_directory_entries
                 WHERE generation = ?1",
                [generation_sql],
                |row| row.get::<_, i64>(0),
            )
            .map_err(map_sql_error)?;
        if actual_entry_count != staged_entry_count {
            return Err(directory_requires_audit());
        }

        let mut encoded_paths = HashSet::with_capacity(entries.len());
        let mut identities = HashSet::with_capacity(entries.len());
        let mut encoded_entries = Vec::with_capacity(entries.len());
        for entry in entries {
            let (path, path_encoding) =
                super::super::directories::encode_directory_path(&entry.relative_path)?;
            super::super::directories::validate_directory_identity(&entry.directory_identity)?;
            if !encoded_paths.insert(path.clone()) {
                return Err(SourceDirectoryTruthError::DuplicatePath {
                    path: entry.relative_path.clone(),
                }
                .into());
            }
            if !identities.insert(entry.directory_identity.clone()) {
                return Err(SourceDirectoryTruthError::DuplicateDirectoryIdentity {
                    identity: entry.directory_identity.clone(),
                }
                .into());
            }
            encoded_entries.push((path, path_encoding, entry.directory_identity.clone()));
        }

        let incoming_count = i64::try_from(encoded_entries.len()).map_err(|_| {
            SourceDirectoryTruthError::InvalidEntryCount {
                count: encoded_entries.len() as u64,
            }
        })?;
        let new_staged_entry_count = staged_entry_count.checked_add(incoming_count).ok_or(
            SourceDirectoryTruthError::InvalidEntryCount {
                count: u64::try_from(staged_entry_count).unwrap_or_default(),
            },
        )?;
        if new_staged_entry_count > expected_entry_count {
            return Err(SourceDirectoryTruthError::EntryCountExceeded { generation }.into());
        }

        for (path, path_encoding, identity) in encoded_entries {
            if self
                .tx
                .query_row(
                    "SELECT 1
                     FROM source_directory_entries
                     WHERE generation = ?1 AND path = ?2 COLLATE BINARY",
                    rusqlite::params![generation_sql, path],
                    |_| Ok(()),
                )
                .optional()
                .map_err(map_sql_error)?
                .is_some()
            {
                return Err(SourceDirectoryTruthError::ExistingPath {
                    path: PathBuf::from(path),
                }
                .into());
            }
            if self
                .tx
                .query_row(
                    "SELECT 1
                     FROM source_directory_entries
                     WHERE generation = ?1 AND directory_identity = ?2",
                    rusqlite::params![generation_sql, identity],
                    |_| Ok(()),
                )
                .optional()
                .map_err(map_sql_error)?
                .is_some()
            {
                return Err(
                    SourceDirectoryTruthError::DuplicateDirectoryIdentity { identity }.into(),
                );
            }
            self.tx
                .execute(
                    "INSERT INTO source_directory_entries (
                        generation, path, path_encoding, directory_identity
                     ) VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![generation_sql, path, path_encoding, identity],
                )
                .map_err(map_sql_error)?;
        }

        self.tx
            .execute(
                "UPDATE source_directory_generations
                 SET staged_entry_count = ?1, complete = ?2
                 WHERE generation = ?3 AND status = 'staging'",
                rusqlite::params![
                    new_staged_entry_count,
                    i64::from(new_staged_entry_count == expected_entry_count),
                    generation_sql
                ],
            )
            .map_err(map_sql_error)?;
        Ok(())
    }

    pub(crate) fn finalize_source_directory_truth_generation(
        self,
        generation: u64,
        expected_source_revision: u64,
    ) -> Result<SourceDirectoryTruthPublication, SourceDbError> {
        let generation_sql = directory_generation_sql_value(generation)?;
        let Some((status, expected_entry_count, staged_entry_count, complete, published_revision)) =
            self.tx
                .query_row(
                    "SELECT status, expected_entry_count, staged_entry_count, complete,
                            published_source_revision
                     FROM source_directory_generations
                     WHERE generation = ?1",
                    [generation_sql],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, i64>(3)?,
                            row.get::<_, Option<i64>>(4)?,
                        ))
                    },
                )
                .optional()
                .map_err(map_sql_error)?
        else {
            return Err(SourceDirectoryTruthError::GenerationMissing { generation }.into());
        };

        if status == "active" {
            let published_revision = valid_published_revision(published_revision)?;
            validate_generation_counts(
                &self.tx,
                generation_sql,
                expected_entry_count,
                staged_entry_count,
                complete,
            )?;
            let db_path = self.db_path.clone();
            let telemetry_label = self.telemetry_label;
            self.tx.commit().map_err(map_sql_error)?;
            checkpoint_source_database(&db_path, telemetry_label);
            return Ok(SourceDirectoryTruthPublication {
                generation,
                source_revision: published_revision,
                idempotent: true,
            });
        }
        if status != "staging" {
            return Err(SourceDirectoryTruthError::GenerationCollision { generation }.into());
        }

        let actual_source_revision = manifest_revision(&self.tx)?;
        if actual_source_revision != expected_source_revision {
            return Err(SourceDirectoryTruthError::StaleRevision {
                expected: expected_source_revision,
                actual: actual_source_revision,
            }
            .into());
        }
        validate_generation_counts(
            &self.tx,
            generation_sql,
            expected_entry_count,
            staged_entry_count,
            complete,
        )?;
        let active_generations: Vec<(i64, i64, i64, i64, Option<i64>)> = {
            let mut statement = self
                .tx
                .prepare(
                    "SELECT generation, expected_entry_count, staged_entry_count, complete,
                            published_source_revision
                     FROM source_directory_generations
                     WHERE status = 'active'",
                )
                .map_err(map_sql_error)?;
            statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, Option<i64>>(4)?,
                    ))
                })
                .map_err(map_sql_error)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sql_error)?
        };
        if active_generations.len() > 1 {
            return Err(directory_requires_audit());
        }
        if let Some((
            active_generation,
            active_expected_entry_count,
            active_staged_entry_count,
            active_complete,
            published_revision,
        )) = active_generations.first().copied()
        {
            if valid_published_revision(published_revision).is_err() {
                return Err(directory_requires_audit());
            }
            validate_generation_counts(
                &self.tx,
                active_generation,
                active_expected_entry_count,
                active_staged_entry_count,
                active_complete,
            )?;
            if active_generation == generation_sql {
                return Err(directory_requires_audit());
            }
        }

        let next_source_revision = expected_source_revision
            .checked_add(1)
            .ok_or(SourceDbError::Unexpected)?;
        SourceDatabase::bump_revision(&self.tx)?;
        let committed_source_revision = manifest_revision(&self.tx)?;
        if committed_source_revision != next_source_revision {
            return Err(SourceDbError::Unexpected);
        }
        self.tx
            .execute(
                "UPDATE source_directory_generations
                 SET status = 'inactive'
                 WHERE status = 'active'",
                [],
            )
            .map_err(map_sql_error)?;
        let changed = self
            .tx
            .execute(
                "UPDATE source_directory_generations
                 SET status = 'active', complete = 1, published_source_revision = ?1
                 WHERE generation = ?2 AND status = 'staging'",
                rusqlite::params![committed_source_revision as i64, generation_sql],
            )
            .map_err(map_sql_error)?;
        if changed != 1 {
            return Err(SourceDirectoryTruthError::GenerationMissing { generation }.into());
        }
        let db_path = self.db_path.clone();
        let telemetry_label = self.telemetry_label;
        self.tx.commit().map_err(map_sql_error)?;
        checkpoint_source_database(&db_path, telemetry_label);
        Ok(SourceDirectoryTruthPublication {
            generation,
            source_revision: committed_source_revision,
            idempotent: false,
        })
    }

    /// Read a metadata value from the active write transaction.
    ///
    /// The value is read from the same snapshot and writer reservation that will commit the
    /// batch, so callers can make a revision-fenced decision without opening a second
    /// connection or observing a newer transaction after the decision.
    pub fn get_metadata(&self, key: &str) -> Result<Option<String>, SourceDbError> {
        self.tx
            .query_row("SELECT value FROM metadata WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()
            .map_err(map_sql_error)
    }

    /// Read the current manifest revision from the active write transaction.
    pub fn get_revision(&self) -> Result<u64, SourceDbError> {
        manifest_revision(&self.tx)
    }

    /// Return whether the source traversal policy still matches the scan snapshot.
    pub fn matches_source_traversal_policy(
        &self,
        expected: SourceTraversalPolicy,
    ) -> Result<bool, SourceDbError> {
        let value = self
            .tx
            .query_row(
                "SELECT value FROM metadata WHERE key = ?1",
                [META_SOURCE_TRAVERSAL_POLICY],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(map_sql_error)?;
        let actual = value
            .as_deref()
            .map(SourceTraversalPolicy::from_stored)
            .unwrap_or_default();
        Ok(actual == expected)
    }

    /// Return whether this write transaction began at `expected_revision`.
    ///
    /// The batch owns SQLite's immediate writer reservation, so a successful match remains valid
    /// until this batch commits or rolls back. Callers can use this to discard work derived from
    /// an older read snapshot without overwriting newer metadata.
    pub fn matches_revision(&self, expected_revision: u64) -> Result<bool, SourceDbError> {
        Ok(manifest_revision(&self.tx)? == expected_revision)
    }

    /// Commit all batched operations atomically.
    pub fn commit(self) -> Result<(), SourceDbError> {
        self.prepare_commit()?;
        self.tx.commit().map_err(map_sql_error)?;
        crate::sqlite_wal::maybe_checkpoint_database_file(
            &self.db_path,
            "source_db",
            self.telemetry_label,
        );
        Ok(())
    }

    /// Commit source-local coordination metadata without advancing the manifest revision.
    ///
    /// This is restricted to batches that did not touch `wav_files` or the ordered path set.
    /// Callers use it for transactionally coherent auxiliary lifecycle state whose publication
    /// must not impersonate a new source-manifest generation.
    pub fn commit_auxiliary_state(self) -> Result<(), SourceDbError> {
        if self.paths_revision_dirty
            || self.identities_revision_dirty
            || !self.manifest_touched_paths.is_empty()
        {
            return Err(SourceDbError::Unexpected);
        }
        if self.index_revision_dirty {
            SourceDatabase::bump_source_index_revision(&self.tx)?;
        }
        self.tx.commit().map_err(map_sql_error)?;
        crate::sqlite_wal::maybe_checkpoint_database_file(
            &self.db_path,
            "source_db",
            self.telemetry_label,
        );
        Ok(())
    }

    /// Commit the batch and return the manifest snapshot owned by that exact revision.
    ///
    /// The snapshot is read from the active write transaction after its revision bump and before
    /// `COMMIT`. A later writer therefore cannot advance the returned revision or alter the
    /// returned manifest between the authoritative mutation and delta publication.
    pub fn commit_with_manifest_snapshot(
        self,
    ) -> Result<(u64, Vec<SourceManifestEntry>), SourceDbError> {
        self.prepare_commit()?;
        let snapshot = manifest_snapshot(&self.tx)?;
        self.tx.commit().map_err(map_sql_error)?;
        crate::sqlite_wal::maybe_checkpoint_database_file(
            &self.db_path,
            "source_db",
            self.telemetry_label,
        );
        Ok(snapshot)
    }

    /// Commit complete manifest-audit coverage and return its opaque source-audit authority.
    ///
    /// The authority exists only when [`SourceWriteBatch::complete_manifest_audit`] was applied
    /// to this same transaction. Callers must revalidate their held source-root capability before
    /// invoking this method; the returned token is the only input accepted by the receipt boundary
    /// for a clearing source-audit acknowledgement.
    pub fn commit_with_manifest_snapshot_and_audit(
        self,
        committed_root_identity: RootIdentity,
        audit_request: Option<&SourceAuditRequest>,
    ) -> Result<(SourceAuditCommit, Vec<SourceManifestEntry>), SourceDbError> {
        if !self.manifest_audit_completed {
            return Err(SourceDbError::Unexpected);
        }
        self.prepare_commit()?;
        let (revision, snapshot) = manifest_snapshot(&self.tx)?;
        self.tx.commit().map_err(map_sql_error)?;
        crate::sqlite_wal::maybe_checkpoint_database_file(
            &self.db_path,
            "source_db",
            self.telemetry_label,
        );
        Ok((
            SourceAuditCommit::new(revision, committed_root_identity, audit_request.cloned()),
            snapshot,
        ))
    }

    /// Commit the batch and return its exact revision plus manifest state owned by that revision.
    ///
    /// When the caller's cached revision is current, `touched_path_changes` contains only touched
    /// paths and `authoritative_snapshot` is `None`, keeping chunked scans linear. When another
    /// writer has advanced the manifest, `touched_path_changes` is empty and
    /// `authoritative_snapshot` contains the full manifest captured inside this committing
    /// transaction before the write lock is released.
    pub fn commit_with_manifest_changes(
        self,
        expected_previous_revision: u64,
    ) -> Result<ManifestCommitResult, SourceDbError> {
        self.prepare_commit()?;
        let revision = manifest_revision(&self.tx)?;
        let source_index_commit = self.source_index_commit(revision)?;
        let (changes, snapshot) = if revision == expected_previous_revision.saturating_add(1) {
            let changes = self
                .manifest_touched_paths
                .iter()
                .map(|path| {
                    let normalized = PathBuf::from(normalize_relative_path(path)?);
                    let entry = manifest_entry_for_path(&self.tx, &normalized)?;
                    Ok((normalized, entry))
                })
                .collect::<Result<Vec<_>, SourceDbError>>()?;
            (changes, None)
        } else {
            let (_, snapshot) = manifest_snapshot(&self.tx)?;
            (Vec::new(), Some(snapshot))
        };
        self.tx.commit().map_err(map_sql_error)?;
        crate::sqlite_wal::maybe_checkpoint_database_file(
            &self.db_path,
            "source_db",
            self.telemetry_label,
        );
        Ok(ManifestCommitResult {
            revision,
            touched_path_changes: changes,
            authoritative_snapshot: snapshot,
            source_index_commit,
        })
    }

    /// Commit a revision-fenced batch and return only the manifest rows touched by that batch.
    ///
    /// Unlike [`Self::commit_with_manifest_changes`], this method never falls back to loading the
    /// complete manifest. It is intended for bounded work whose caller has already selected a
    /// small path set and requires an exact revision match before publishing that path delta.
    pub fn commit_with_bounded_manifest_changes(
        self,
        expected_previous_revision: u64,
    ) -> Result<ManifestCommitResult, SourceDbError> {
        if manifest_revision(&self.tx)? != expected_previous_revision {
            return Err(SourceDbError::Unexpected);
        }
        self.prepare_commit()?;
        let revision = manifest_revision(&self.tx)?;
        let source_index_commit = self.source_index_commit(revision)?;
        let changes = self
            .manifest_touched_paths
            .iter()
            .map(|path| {
                let normalized = PathBuf::from(normalize_relative_path(path)?);
                let entry = manifest_entry_for_path(&self.tx, &normalized)?;
                Ok((normalized, entry))
            })
            .collect::<Result<Vec<_>, SourceDbError>>()?;
        self.tx.commit().map_err(map_sql_error)?;
        crate::sqlite_wal::maybe_checkpoint_database_file(
            &self.db_path,
            "source_db",
            self.telemetry_label,
        );
        Ok(ManifestCommitResult {
            revision,
            touched_path_changes: changes,
            authoritative_snapshot: None,
            source_index_commit,
        })
    }

    fn source_index_commit(
        &self,
        source_revision: u64,
    ) -> Result<Option<SourceIndexCommitResult>, SourceDbError> {
        if self.source_index_changes.is_empty() {
            return Ok(None);
        }
        let index_revision = self
            .tx
            .query_row(
                "SELECT value FROM metadata WHERE key = 'source_index_revision_v1'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(map_sql_error)?
            .map(|raw| raw.parse::<u64>().map_err(|_| SourceDbError::Unexpected))
            .transpose()?
            .unwrap_or_default();
        let mut upserted_entries = Vec::new();
        let mut removed_paths = Vec::new();
        for (path, entry) in &self.source_index_changes {
            match entry {
                Some(entry) => upserted_entries.push(entry.clone()),
                None => removed_paths.push(path.clone()),
            }
        }
        Ok(Some(SourceIndexCommitResult {
            source_revision,
            index_revision,
            upserted_entries,
            removed_paths,
        }))
    }

    fn prepare_commit(&self) -> Result<(), SourceDbError> {
        SourceDatabase::bump_revision(&self.tx)?;
        if self.paths_revision_dirty {
            SourceDatabase::bump_wav_paths_revision(&self.tx)?;
        }
        if self.identities_revision_dirty {
            SourceDatabase::bump_wav_identities_revision(&self.tx)?;
        }
        if self.index_revision_dirty {
            SourceDatabase::bump_source_index_revision(&self.tx)?;
        }
        Ok(())
    }
}

fn directory_generation_sql_value(generation: u64) -> Result<i64, SourceDbError> {
    if generation == 0 {
        return Err(SourceDirectoryTruthError::InvalidGeneration { generation }.into());
    }
    i64::try_from(generation)
        .map_err(|_| SourceDirectoryTruthError::InvalidGeneration { generation }.into())
}

fn directory_entry_count_sql_value(count: u64) -> Result<i64, SourceDbError> {
    i64::try_from(count).map_err(|_| SourceDirectoryTruthError::InvalidEntryCount { count }.into())
}

fn unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or_default()
}

fn directory_requires_audit() -> SourceDbError {
    SourceDirectoryTruthError::RequiresAudit {
        reason: super::super::SourceDirectoryTruthUnavailableReason::Malformed,
    }
    .into()
}

fn valid_published_revision(value: Option<i64>) -> Result<u64, SourceDbError> {
    let Some(value) = value.filter(|value| *value > 0) else {
        return Err(directory_requires_audit());
    };
    u64::try_from(value).map_err(|_| directory_requires_audit())
}

fn validate_generation_counts(
    connection: &rusqlite::Transaction<'_>,
    generation: i64,
    expected_entry_count: i64,
    staged_entry_count: i64,
    complete: i64,
) -> Result<(), SourceDbError> {
    if expected_entry_count < 0 || staged_entry_count < 0 || complete < 0 {
        return Err(directory_requires_audit());
    }
    let actual_entry_count = connection
        .query_row(
            "SELECT COUNT(*), COUNT(DISTINCT path), COUNT(DISTINCT directory_identity)
             FROM source_directory_entries
             WHERE generation = ?1",
            [generation],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .map_err(map_sql_error)?;
    if actual_entry_count.0 != actual_entry_count.1 || actual_entry_count.0 != actual_entry_count.2
    {
        return Err(directory_requires_audit());
    }
    if complete != 1 || expected_entry_count != staged_entry_count {
        return Err(SourceDirectoryTruthError::Incomplete {
            generation: u64::try_from(generation).unwrap_or_default(),
            expected: u64::try_from(expected_entry_count).unwrap_or_default(),
            staged: u64::try_from(actual_entry_count.0).unwrap_or_default(),
        }
        .into());
    }
    if actual_entry_count.0 != staged_entry_count {
        return Err(SourceDirectoryTruthError::Incomplete {
            generation: u64::try_from(generation).unwrap_or_default(),
            expected: u64::try_from(expected_entry_count).unwrap_or_default(),
            staged: u64::try_from(actual_entry_count.0).unwrap_or_default(),
        }
        .into());
    }
    Ok(())
}

fn checkpoint_source_database(db_path: &Path, telemetry_label: &'static str) {
    crate::sqlite_wal::maybe_checkpoint_database_file(db_path, "source_db", telemetry_label);
}

fn manifest_snapshot(
    connection: &rusqlite::Connection,
) -> Result<(u64, Vec<SourceManifestEntry>), SourceDbError> {
    let revision = manifest_revision(connection)?;
    let filter = crate::sample_sources::supported_audio_where_clause();
    let sql = format!(
        "SELECT path, file_identity, content_hash, file_size, modified_ns
         FROM wav_files
         WHERE {filter} AND missing = 0
         ORDER BY path ASC"
    );
    let raw_entries = {
        let mut statement = connection.prepare(&sql).map_err(map_sql_error)?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get::<_, i64>(3)?,
                    row.get(4)?,
                ))
            })
            .map_err(map_sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_sql_error)?
    };
    let entries = raw_entries
        .into_iter()
        .filter_map(
            |(raw_path, file_identity, content_hash, file_size, modified_ns)| {
                let normalized = match normalize_relative_path(std::path::Path::new(&raw_path)) {
                    Ok(normalized) => normalized,
                    Err(error) => {
                        tracing::warn!(
                            path = raw_path,
                            %error,
                            "Skipping source manifest row with invalid relative path"
                        );
                        return None;
                    }
                };
                Some(SourceManifestEntry {
                    relative_path: PathBuf::from(normalized),
                    file_identity,
                    content_hash,
                    file_size: file_size.max(0) as u64,
                    modified_ns,
                })
            },
        )
        .collect();
    Ok((revision, entries))
}

fn manifest_revision(connection: &rusqlite::Connection) -> Result<u64, SourceDbError> {
    connection
        .query_row(
            "SELECT value FROM metadata WHERE key = 'revision'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(map_sql_error)?
        .map(|raw| raw.parse::<u64>().map_err(|_| SourceDbError::Unexpected))
        .transpose()
        .map(|revision| revision.unwrap_or_default())
}

fn manifest_entry_for_path(
    connection: &rusqlite::Connection,
    relative_path: &std::path::Path,
) -> Result<Option<SourceManifestEntry>, SourceDbError> {
    let raw_path = relative_path.to_string_lossy();
    let row = connection
        .query_row(
            "SELECT path, file_identity, content_hash, file_size, modified_ns
             FROM wav_files
             WHERE path = ?1 AND missing = 0",
            [raw_path.as_ref()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get::<_, i64>(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()
        .map_err(map_sql_error)?;
    let Some((raw_path, file_identity, content_hash, file_size, modified_ns)) = row else {
        return Ok(None);
    };
    let normalized = normalize_relative_path(std::path::Path::new(&raw_path))?;
    Ok(Some(SourceManifestEntry {
        relative_path: PathBuf::from(normalized),
        file_identity,
        content_hash,
        file_size: file_size.max(0) as u64,
        modified_ns,
    }))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn auxiliary_commit_rejects_manifest_mutations() {
        let directory = tempfile::tempdir().expect("source root");
        let database =
            SourceDatabase::open_for_source_write(directory.path()).expect("source database");
        let mut batch = database.write_batch().expect("write batch");
        batch
            .upsert_file(Path::new("sample.wav"), 1, 1)
            .expect("stage manifest mutation");

        assert!(matches!(
            batch.commit_auxiliary_state(),
            Err(SourceDbError::Unexpected)
        ));
        assert!(
            database
                .entry_for_path(Path::new("sample.wav"))
                .expect("read rolled-back manifest")
                .is_none()
        );
    }

    #[test]
    fn commit_snapshot_stays_bound_to_its_own_revision() {
        let directory = tempfile::tempdir().expect("source root");
        let database =
            SourceDatabase::open_for_source_write(directory.path()).expect("source database");
        let mut first = database.write_batch().expect("first batch");
        first
            .upsert_file_with_hash(Path::new("first.wav"), 5, 10, "first-hash")
            .expect("insert first file");
        let (committed_revision, committed_manifest) = first
            .commit_with_manifest_snapshot()
            .expect("commit first manifest");

        let mut second = database.write_batch().expect("second batch");
        second
            .upsert_file_with_hash(Path::new("second.wav"), 6, 20, "second-hash")
            .expect("insert second file");
        second.commit().expect("commit second manifest");

        assert_eq!(committed_manifest.len(), 1);
        assert_eq!(committed_manifest[0].relative_path, Path::new("first.wav"));
        assert!(database.get_revision().expect("current revision") > committed_revision);
    }

    #[test]
    fn commit_manifest_changes_reports_only_touched_live_rows() {
        let directory = tempfile::tempdir().expect("source root");
        let database =
            SourceDatabase::open_for_source_write(directory.path()).expect("source database");
        database
            .upsert_file(Path::new("removed.wav"), 5, 10)
            .expect("insert removed file");
        database
            .upsert_file(Path::new("untouched.wav"), 6, 20)
            .expect("insert untouched file");

        let expected_previous_revision = database.get_revision().expect("previous revision");
        let mut batch = database.write_batch().expect("manifest batch");
        batch
            .set_missing(Path::new("removed.wav"), true)
            .expect("mark file missing");
        batch
            .upsert_file_with_hash(Path::new("created.wav"), 7, 30, "created-hash")
            .expect("insert created file");
        let result = batch
            .commit_with_manifest_changes(expected_previous_revision)
            .expect("commit manifest changes");

        assert_eq!(
            result.revision,
            database.get_revision().expect("current revision")
        );
        assert!(result.authoritative_snapshot.is_none());
        assert_eq!(result.touched_path_changes.len(), 2);
        assert_eq!(
            result.touched_path_changes[0],
            (
                PathBuf::from("created.wav"),
                Some(SourceManifestEntry {
                    relative_path: PathBuf::from("created.wav"),
                    file_identity: None,
                    content_hash: Some(String::from("created-hash")),
                    file_size: 7,
                    modified_ns: 30,
                })
            )
        );
        assert_eq!(
            result.touched_path_changes[1],
            (PathBuf::from("removed.wav"), None)
        );
    }

    #[test]
    fn commit_manifest_changes_returns_authoritative_snapshot_when_revision_advanced() {
        let directory = tempfile::tempdir().expect("source root");
        let database =
            SourceDatabase::open_for_source_write(directory.path()).expect("source database");
        database
            .upsert_file(Path::new("existing.wav"), 5, 10)
            .expect("insert existing file");

        let mut batch = database.write_batch().expect("manifest batch");
        batch
            .upsert_file_with_hash(Path::new("created.wav"), 7, 30, "created-hash")
            .expect("insert created file");
        let result = batch
            .commit_with_manifest_changes(0)
            .expect("commit manifest changes");

        assert_eq!(
            result.revision,
            database.get_revision().expect("current revision")
        );
        assert!(result.touched_path_changes.is_empty());
        let snapshot = result
            .authoritative_snapshot
            .expect("authoritative manifest snapshot");
        assert_eq!(
            snapshot
                .into_iter()
                .map(|entry| entry.relative_path)
                .collect::<Vec<_>>(),
            vec![PathBuf::from("created.wav"), PathBuf::from("existing.wav")]
        );
    }

    #[test]
    fn commit_manifest_changes_normalizes_windows_separator_paths() {
        let directory = tempfile::tempdir().expect("source root");
        let database =
            SourceDatabase::open_for_source_write(directory.path()).expect("source database");
        let expected_previous_revision = database.get_revision().expect("previous revision");
        let mut batch = database.write_batch().expect("manifest batch");
        batch
            .upsert_file_with_hash(Path::new(r"nested\kick.wav"), 7, 30, "kick-hash")
            .expect("insert nested file");

        let result = batch
            .commit_with_manifest_changes(expected_previous_revision)
            .expect("commit manifest changes");

        assert!(result.authoritative_snapshot.is_none());
        assert_eq!(result.touched_path_changes.len(), 1);
        assert_eq!(
            result.touched_path_changes[0].0,
            Path::new("nested/kick.wav")
        );
        assert_eq!(
            result.touched_path_changes[0]
                .1
                .as_ref()
                .map(|entry| entry.relative_path.as_path()),
            Some(Path::new("nested/kick.wav"))
        );
    }
}
