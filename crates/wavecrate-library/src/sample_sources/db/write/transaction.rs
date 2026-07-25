use std::path::PathBuf;

use rusqlite::OptionalExtension;

use super::super::util::{map_sql_error, normalize_relative_path};
use super::super::{
    META_SOURCE_TRAVERSAL_POLICY, SourceDatabase, SourceDbError, SourceManifestEntry,
    SourceTraversalPolicy, SourceWriteBatch,
};

/// Manifest state published by a committed source-database write batch.
pub struct ManifestCommitResult {
    /// Revision assigned to the committed manifest state.
    pub revision: u64,
    /// Manifest rows for paths touched by this batch when the cached revision was current.
    pub touched_path_changes: Vec<(PathBuf, Option<SourceManifestEntry>)>,
    /// Complete manifest captured in the committing transaction when the cached revision was stale.
    pub authoritative_snapshot: Option<Vec<SourceManifestEntry>>,
}

impl SourceWriteBatch<'_> {
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
        })
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
