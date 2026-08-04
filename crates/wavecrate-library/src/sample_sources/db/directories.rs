use std::collections::HashSet;
use std::path::Path;

use rusqlite::{OptionalExtension, Transaction, TransactionBehavior};

use super::schema;
use super::util::{
    map_sql_error, normalize_source_index_path, parse_canonical_directory_path_from_db,
};
use super::{
    SOURCE_DIRECTORY_TRUTH_BATCH_LIMIT, SOURCE_DIRECTORY_TRUTH_CLEANUP_LIMIT,
    SOURCE_DIRECTORY_TRUTH_MAX_IDENTITY_BYTES, SourceDatabase, SourceDbError, SourceDirectoryEntry,
    SourceDirectoryTruthCleanup, SourceDirectoryTruthCursor, SourceDirectoryTruthError,
    SourceDirectoryTruthPage, SourceDirectoryTruthPublication, SourceDirectoryTruthState,
    SourceDirectoryTruthUnavailableReason,
};

const GENERATION_COLUMNS: [&str; 7] = [
    "generation",
    "status",
    "expected_entry_count",
    "staged_entry_count",
    "complete",
    "published_source_revision",
    "created_at",
];
const ENTRY_COLUMNS: [&str; 4] = ["generation", "path", "path_encoding", "directory_identity"];
const CLEANUP_ENTRY_LIMIT: usize = SOURCE_DIRECTORY_TRUTH_BATCH_LIMIT;

#[derive(Clone, Copy, PartialEq, Eq)]
enum DirectoryTruthInspection {
    FullIntegrity,
    BoundedPage,
}

/// Encode one source-relative directory path using the lossless source-index representation.
pub(crate) fn encode_directory_path(path: &Path) -> Result<(String, i64), SourceDbError> {
    normalize_source_index_path(path).map_err(|_| {
        SourceDirectoryTruthError::InvalidPath {
            path: path.to_path_buf(),
        }
        .into()
    })
}

/// Validate the stable identity recorded for one directory.
pub(crate) fn validate_directory_identity(identity: &str) -> Result<(), SourceDbError> {
    if identity.trim().is_empty()
        || identity.len() > SOURCE_DIRECTORY_TRUTH_MAX_IDENTITY_BYTES
        || identity.chars().any(char::is_control)
    {
        return Err(SourceDirectoryTruthError::InvalidDirectoryIdentity.into());
    }
    Ok(())
}

fn directory_truth_schema_available(
    connection: &rusqlite::Connection,
) -> Result<bool, SourceDbError> {
    let generation_columns = schema::table_columns(connection, "source_directory_generations")?;
    let entry_columns = schema::table_columns(connection, "source_directory_entries")?;
    Ok(GENERATION_COLUMNS
        .iter()
        .all(|column| generation_columns.contains(*column))
        && ENTRY_COLUMNS
            .iter()
            .all(|column| entry_columns.contains(*column)))
}

fn schema_unavailable() -> SourceDbError {
    SourceDirectoryTruthError::SchemaUnavailable.into()
}

fn requires_audit(reason: SourceDirectoryTruthUnavailableReason) -> SourceDbError {
    SourceDirectoryTruthError::RequiresAudit { reason }.into()
}

fn source_revision(connection: &rusqlite::Connection) -> Result<u64, SourceDbError> {
    connection
        .query_row(
            "SELECT value FROM metadata WHERE key = 'revision'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(map_sql_error)?
        .map(|value| value.parse::<u64>().map_err(|_| SourceDbError::Unexpected))
        .transpose()
        .map(|value| value.unwrap_or_default())
}

fn inspect_directory_truth_state(
    connection: &rusqlite::Connection,
    inspection: DirectoryTruthInspection,
) -> Result<SourceDirectoryTruthState, SourceDbError> {
    let current_source_revision = source_revision(connection)?;
    let active_query = match inspection {
        DirectoryTruthInspection::FullIntegrity => {
            "SELECT generation, expected_entry_count, staged_entry_count, complete,
                    published_source_revision
             FROM source_directory_generations
             WHERE status = 'active'
             ORDER BY generation ASC"
        }
        DirectoryTruthInspection::BoundedPage => {
            "SELECT generation, expected_entry_count, staged_entry_count, complete,
                    published_source_revision
             FROM source_directory_generations
             WHERE status = 'active'
             ORDER BY generation ASC
             LIMIT 2"
        }
    };
    let active_rows = {
        let mut statement = connection.prepare(active_query).map_err(map_sql_error)?;
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
    if active_rows.len() > 1 {
        return Ok(SourceDirectoryTruthState::Unavailable {
            reason: SourceDirectoryTruthUnavailableReason::Malformed,
        });
    }

    let Some((generation, expected, staged, complete, published_revision)) =
        active_rows.first().copied()
    else {
        let has_inactive = match inspection {
            DirectoryTruthInspection::FullIntegrity => connection
                .query_row(
                    "SELECT COUNT(*) FROM source_directory_generations WHERE status = 'inactive'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(map_sql_error)?
                != 0,
            DirectoryTruthInspection::BoundedPage => {
                connection
                    .query_row(
                        "SELECT EXISTS(
                        SELECT 1
                        FROM source_directory_generations
                        WHERE status = 'inactive'
                    )",
                        [],
                        |row| row.get::<_, i64>(0),
                    )
                    .map_err(map_sql_error)?
                    != 0
            }
        };
        if has_inactive {
            return Ok(SourceDirectoryTruthState::Unavailable {
                reason: SourceDirectoryTruthUnavailableReason::Malformed,
            });
        }
        return Ok(SourceDirectoryTruthState::Uninitialized);
    };

    let Some(published_revision) = published_revision.filter(|revision| *revision > 0) else {
        return Ok(SourceDirectoryTruthState::Unavailable {
            reason: SourceDirectoryTruthUnavailableReason::Malformed,
        });
    };
    if generation <= 0
        || expected < 0
        || staged < 0
        || complete != 1
        || expected != staged
        || u64::try_from(published_revision).unwrap_or_default() > current_source_revision
    {
        return Ok(SourceDirectoryTruthState::Unavailable {
            reason: SourceDirectoryTruthUnavailableReason::Malformed,
        });
    }

    if inspection == DirectoryTruthInspection::FullIntegrity {
        {
            let mut statement = connection
                .prepare(
                    "SELECT path, path_encoding, directory_identity
                     FROM source_directory_entries
                     WHERE generation = ?1",
                )
                .map_err(map_sql_error)?;
            let mut rows = statement.query([generation]).map_err(map_sql_error)?;
            while let Some(row) = rows.next().map_err(map_sql_error)? {
                let raw_path = row.get::<_, String>(0).map_err(map_sql_error)?;
                let path_encoding = row.get::<_, i64>(1).map_err(map_sql_error)?;
                let directory_identity = row.get::<_, String>(2).map_err(map_sql_error)?;
                if active_page_entry(raw_path, path_encoding, directory_identity).is_err() {
                    return Ok(SourceDirectoryTruthState::Unavailable {
                        reason: SourceDirectoryTruthUnavailableReason::AuditRequired,
                    });
                }
            }
        }

        let (entry_count, distinct_paths, distinct_identities): (i64, i64, i64) = connection
            .query_row(
                "SELECT COUNT(*), COUNT(DISTINCT path), COUNT(DISTINCT directory_identity)
                 FROM source_directory_entries
                 WHERE generation = ?1",
                [generation],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(map_sql_error)?;
        if entry_count != expected
            || entry_count != distinct_paths
            || entry_count != distinct_identities
        {
            return Ok(SourceDirectoryTruthState::Unavailable {
                reason: SourceDirectoryTruthUnavailableReason::Malformed,
            });
        }
    }

    Ok(SourceDirectoryTruthState::Active {
        generation: u64::try_from(generation).map_err(|_| SourceDbError::Unexpected)?,
        published_source_revision: u64::try_from(published_revision)
            .map_err(|_| SourceDbError::Unexpected)?,
    })
}

fn active_page_entry(
    raw_path: String,
    path_encoding: i64,
    directory_identity: String,
) -> Result<SourceDirectoryEntry, SourceDbError> {
    let relative_path = parse_canonical_directory_path_from_db(&raw_path, path_encoding)
        .map_err(|_| requires_audit(SourceDirectoryTruthUnavailableReason::AuditRequired))?;
    validate_directory_identity(&directory_identity)
        .map_err(|_| requires_audit(SourceDirectoryTruthUnavailableReason::AuditRequired))?;
    Ok(SourceDirectoryEntry {
        relative_path,
        directory_identity,
    })
}

impl SourceDatabase {
    /// Start a revision-neutral source-directory generation.
    pub fn begin_source_directory_truth_generation(
        &self,
        generation: u64,
        expected_entry_count: u64,
    ) -> Result<(), SourceDbError> {
        let mut batch = self.write_batch()?;
        batch.begin_source_directory_truth_generation(generation, expected_entry_count)?;
        batch.commit_auxiliary_state()
    }

    /// Stage one bounded batch of validated descendant directories.
    pub fn stage_source_directory_truth_entries(
        &self,
        generation: u64,
        entries: &[SourceDirectoryEntry],
    ) -> Result<(), SourceDbError> {
        let mut batch = self.write_batch()?;
        batch.stage_source_directory_truth_entries(generation, entries)?;
        batch.commit_auxiliary_state()
    }

    /// Atomically publish one complete directory generation at the expected source revision.
    pub fn finalize_source_directory_truth_generation(
        &self,
        generation: u64,
        expected_source_revision: u64,
    ) -> Result<SourceDirectoryTruthPublication, SourceDbError> {
        let batch = self.write_batch()?;
        batch.finalize_source_directory_truth_generation(generation, expected_source_revision)
    }

    /// Read the active directory truth without migrating or creating a legacy reader schema.
    pub fn source_directory_truth_state(&self) -> Result<SourceDirectoryTruthState, SourceDbError> {
        if !directory_truth_schema_available(&self.connection)? {
            return Ok(SourceDirectoryTruthState::Unavailable {
                reason: SourceDirectoryTruthUnavailableReason::SchemaUnavailable,
            });
        }
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(map_sql_error)?;
        let state =
            inspect_directory_truth_state(&transaction, DirectoryTruthInspection::FullIntegrity)?;
        transaction.rollback().map_err(map_sql_error)?;
        Ok(state)
    }

    /// Read one bounded, revision-fenced page of the active directory generation.
    pub fn source_directory_truth_page(
        &self,
        after_cursor: Option<&SourceDirectoryTruthCursor>,
        requested_limit: usize,
    ) -> Result<SourceDirectoryTruthPage, SourceDbError> {
        if !directory_truth_schema_available(&self.connection)? {
            return Ok(SourceDirectoryTruthPage {
                state: SourceDirectoryTruthState::Unavailable {
                    reason: SourceDirectoryTruthUnavailableReason::SchemaUnavailable,
                },
                entries: Vec::new(),
                next_cursor: None,
            });
        }
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(map_sql_error)?;
        let state =
            inspect_directory_truth_state(&transaction, DirectoryTruthInspection::BoundedPage)?;
        let SourceDirectoryTruthState::Active {
            generation,
            published_source_revision,
        } = state
        else {
            transaction.rollback().map_err(map_sql_error)?;
            return Ok(SourceDirectoryTruthPage {
                state,
                entries: Vec::new(),
                next_cursor: None,
            });
        };

        if let Some(cursor) = after_cursor
            && (cursor.generation != generation
                || cursor.published_source_revision != published_source_revision)
        {
            return Err(SourceDirectoryTruthError::StaleCursor.into());
        }

        let limit = requested_limit.clamp(1, SOURCE_DIRECTORY_TRUTH_BATCH_LIMIT);
        let fetch_limit = i64::try_from(limit + 1).map_err(|_| SourceDbError::Unexpected)?;
        let raw_rows: Vec<(String, i64, String)> = if let Some(cursor) = after_cursor {
            let mut statement = transaction
                .prepare(
                    "SELECT path, path_encoding, directory_identity
                     FROM source_directory_entries
                     WHERE generation = ?1 AND path > ?2 COLLATE BINARY
                     ORDER BY path ASC
                     LIMIT ?3",
                )
                .map_err(map_sql_error)?;
            statement
                .query_map(
                    rusqlite::params![generation as i64, cursor.raw_path.as_str(), fetch_limit],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .map_err(map_sql_error)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sql_error)?
        } else {
            let mut statement = transaction
                .prepare(
                    "SELECT path, path_encoding, directory_identity
                     FROM source_directory_entries
                     WHERE generation = ?1
                     ORDER BY path ASC
                     LIMIT ?2",
                )
                .map_err(map_sql_error)?;
            statement
                .query_map(rusqlite::params![generation as i64, fetch_limit], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                })
                .map_err(map_sql_error)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sql_error)?
        };
        let has_more = raw_rows.len() > limit;
        let visible_rows = raw_rows.into_iter().take(limit).collect::<Vec<_>>();
        let mut identities = HashSet::with_capacity(visible_rows.len());
        let mut entries = Vec::with_capacity(visible_rows.len());
        for (raw_path, path_encoding, identity) in visible_rows.iter().cloned() {
            if !identities.insert(identity.clone()) {
                return Err(requires_audit(
                    SourceDirectoryTruthUnavailableReason::AuditRequired,
                ));
            }
            entries.push(active_page_entry(raw_path, path_encoding, identity)?);
        }
        let next_cursor = has_more
            .then(|| {
                visible_rows.last().map(|(raw_path, _, _)| {
                    SourceDirectoryTruthCursor::from_parts(
                        generation,
                        published_source_revision,
                        raw_path.clone(),
                    )
                })
            })
            .flatten();
        transaction.rollback().map_err(map_sql_error)?;
        Ok(SourceDirectoryTruthPage {
            state,
            entries,
            next_cursor,
        })
    }

    /// Remove a bounded number of oldest inactive generations without advancing source revision.
    pub fn cleanup_source_directory_truth(
        &self,
        requested_limit: usize,
    ) -> Result<SourceDirectoryTruthCleanup, SourceDbError> {
        if !directory_truth_schema_available(&self.connection)? {
            return Err(schema_unavailable());
        }
        let limit = requested_limit.min(SOURCE_DIRECTORY_TRUTH_CLEANUP_LIMIT);
        if limit == 0 {
            return Ok(SourceDirectoryTruthCleanup {
                deleted_entries: 0,
                deleted_generations: 0,
                more_work: self.has_inactive_directory_generations()?,
            });
        }
        let transaction =
            Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)
                .map_err(map_sql_error)?;
        let candidates = {
            let mut statement = transaction
                .prepare(
                    "SELECT generation
                     FROM source_directory_generations
                     WHERE status = 'inactive'
                     ORDER BY generation ASC
                     LIMIT ?1",
                )
                .map_err(map_sql_error)?;
            statement
                .query_map(
                    [i64::try_from(limit).map_err(|_| SourceDbError::Unexpected)?],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(map_sql_error)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sql_error)?
        };
        let mut deleted_entries = 0usize;
        let mut deleted_generations = 0usize;
        let mut remaining_entry_budget = CLEANUP_ENTRY_LIMIT;
        for generation in candidates {
            let deleted = if remaining_entry_budget == 0 {
                0
            } else {
                transaction
                    .execute(
                        "DELETE FROM source_directory_entries
                         WHERE generation = ?1
                           AND path IN (
                               SELECT path
                               FROM source_directory_entries
                               WHERE generation = ?1
                               ORDER BY path ASC
                               LIMIT ?2
                           )",
                        rusqlite::params![
                            generation,
                            i64::try_from(remaining_entry_budget)
                                .map_err(|_| SourceDbError::Unexpected)?
                        ],
                    )
                    .map_err(map_sql_error)?
            };
            deleted_entries = deleted_entries.saturating_add(deleted);
            remaining_entry_budget = remaining_entry_budget.saturating_sub(deleted);

            let exhausted = transaction
                .query_row(
                    "SELECT 1
                     FROM source_directory_entries
                     WHERE generation = ?1
                     ORDER BY path ASC
                     LIMIT 1",
                    [generation],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .map_err(map_sql_error)?
                .is_none();
            if exhausted {
                let removed_generation = transaction
                    .execute(
                        "DELETE FROM source_directory_generations
                         WHERE generation = ?1 AND status = 'inactive'",
                        [generation],
                    )
                    .map_err(map_sql_error)?;
                if removed_generation == 1 {
                    deleted_generations = deleted_generations.saturating_add(1);
                }
            }
        }
        let more_work = transaction
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM source_directory_generations WHERE status = 'inactive'
                 )",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(map_sql_error)?
            != 0;
        let db_path = self.db_path.clone();
        let telemetry_label = self.telemetry_label;
        transaction.commit().map_err(map_sql_error)?;
        crate::sqlite_wal::maybe_checkpoint_database_file(&db_path, "source_db", telemetry_label);
        Ok(SourceDirectoryTruthCleanup {
            deleted_entries,
            deleted_generations,
            more_work,
        })
    }

    fn has_inactive_directory_generations(&self) -> Result<bool, SourceDbError> {
        Ok(self
            .connection
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM source_directory_generations WHERE status = 'inactive'
                 )",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(map_sql_error)?
            != 0)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::super::{
        SourceDatabase, SourceDbError, SourceDirectoryEntry, SourceDirectoryTruthError,
        SourceDirectoryTruthState, SourceDirectoryTruthUnavailableReason,
    };

    fn directory(path: &str, identity: &str) -> SourceDirectoryEntry {
        SourceDirectoryEntry {
            relative_path: Path::new(path).to_path_buf(),
            directory_identity: identity.to_owned(),
        }
    }

    fn directory_error(error: SourceDbError) -> SourceDirectoryTruthError {
        match error {
            SourceDbError::DirectoryTruth(error) => error,
            other => panic!("expected directory-truth error, got {other:?}"),
        }
    }

    #[test]
    fn stages_and_publishes_directory_truth_once_at_one_revision() {
        let root = tempfile::tempdir().unwrap();
        let db = SourceDatabase::open_for_source_write(root.path()).unwrap();
        db.begin_source_directory_truth_generation(1, 2).unwrap();
        db.stage_source_directory_truth_entries(
            1,
            &[
                directory("drums", "dir-1"),
                directory("drums/kicks", "dir-2"),
            ],
        )
        .unwrap();

        assert_eq!(db.get_revision().unwrap(), 0);
        assert_eq!(
            db.source_directory_truth_state().unwrap(),
            SourceDirectoryTruthState::Uninitialized
        );

        let publication = db.finalize_source_directory_truth_generation(1, 0).unwrap();
        assert_eq!(publication.generation, 1);
        assert_eq!(publication.source_revision, 1);
        assert!(!publication.idempotent);
        assert_eq!(db.get_revision().unwrap(), 1);

        let retry = db.finalize_source_directory_truth_generation(1, 0).unwrap();
        assert!(retry.idempotent);
        assert_eq!(retry.source_revision, 1);
        assert_eq!(db.get_revision().unwrap(), 1);

        let first = db.source_directory_truth_page(None, 1).unwrap();
        assert_eq!(first.entries.len(), 1);
        assert_eq!(first.entries[0].relative_path, Path::new("drums"));
        let second = db
            .source_directory_truth_page(first.next_cursor.as_ref(), 1)
            .unwrap();
        assert_eq!(second.entries.len(), 1);
        assert!(second.next_cursor.is_none());
    }

    #[test]
    fn unicode_directory_path_round_trips_through_storage() {
        let root = tempfile::tempdir().unwrap();
        let db = SourceDatabase::open_for_source_write(root.path()).unwrap();
        let expected = directory("samples/東京/🎛️", "dir-unicode");

        db.begin_source_directory_truth_generation(1, 1).unwrap();
        db.stage_source_directory_truth_entries(1, std::slice::from_ref(&expected))
            .unwrap();
        db.finalize_source_directory_truth_generation(1, 0).unwrap();

        let page = db.source_directory_truth_page(None, 1).unwrap();
        assert_eq!(page.entries, vec![expected]);
        assert!(page.next_cursor.is_none());
    }

    #[test]
    fn reserved_prefix_unicode_directory_path_round_trips_through_storage() {
        let root = tempfile::tempdir().unwrap();
        let db = SourceDatabase::open_for_source_write(root.path()).unwrap();
        let expected = directory(
            "~wavecrate-nu~café/~wavecrate-escaped~東京",
            "dir-reserved-unicode",
        );

        db.begin_source_directory_truth_generation(1, 1).unwrap();
        db.stage_source_directory_truth_entries(1, std::slice::from_ref(&expected))
            .unwrap();
        db.finalize_source_directory_truth_generation(1, 0).unwrap();

        let page = db.source_directory_truth_page(None, 1).unwrap();
        assert_eq!(page.entries, vec![expected]);
        assert!(page.next_cursor.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn unix_non_unicode_directory_path_round_trips_through_storage() {
        use std::ffi::OsString;
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        use std::path::PathBuf;

        let root = tempfile::tempdir().unwrap();
        let db = SourceDatabase::open_for_source_write(root.path()).unwrap();
        let mut path = PathBuf::from("samples");
        path.push(OsString::from_vec(b"raw-\xFF".to_vec()));
        let expected = SourceDirectoryEntry {
            relative_path: path.clone(),
            directory_identity: "dir-non-unicode".to_owned(),
        };

        db.begin_source_directory_truth_generation(1, 1).unwrap();
        db.stage_source_directory_truth_entries(1, std::slice::from_ref(&expected))
            .unwrap();
        db.finalize_source_directory_truth_generation(1, 0).unwrap();

        let page = db.source_directory_truth_page(None, 1).unwrap();
        assert_eq!(page.entries.len(), 1);
        assert_eq!(
            page.entries[0].directory_identity,
            expected.directory_identity
        );
        assert_eq!(
            page.entries[0].relative_path.as_os_str().as_bytes(),
            expected.relative_path.as_os_str().as_bytes()
        );
    }

    #[cfg(unix)]
    #[test]
    fn unix_unicode_literal_backslash_directory_path_round_trips_through_storage() {
        use std::ffi::OsString;
        use std::path::PathBuf;

        let root = tempfile::tempdir().unwrap();
        let db = SourceDatabase::open_for_source_write(root.path()).unwrap();
        let mut path = PathBuf::from("samples");
        path.push(OsString::from(r"kick\raw"));
        let expected = SourceDirectoryEntry {
            relative_path: path,
            directory_identity: "dir-unicode-backslash".to_owned(),
        };

        db.begin_source_directory_truth_generation(1, 1).unwrap();
        db.stage_source_directory_truth_entries(1, std::slice::from_ref(&expected))
            .unwrap();
        db.finalize_source_directory_truth_generation(1, 0).unwrap();

        let page = db.source_directory_truth_page(None, 1).unwrap();
        assert_eq!(page.entries, vec![expected]);
        assert!(page.next_cursor.is_none());
    }

    #[test]
    fn malformed_persisted_lossless_directory_paths_require_audit() {
        for (path, expected_description) in [
            ("valid/~wavecrate-escaped~2f", "encoded slash/root"),
            (
                "valid/~wavecrate-escaped~612f62",
                "encoded slash inside component",
            ),
            ("valid/~wavecrate-escaped~2e2e", "encoded parent"),
            (
                "valid/~wavecrate-escaped~616c696173",
                "escaped ordinary plain path alias",
            ),
        ] {
            let root = tempfile::tempdir().unwrap();
            let db = SourceDatabase::open_for_source_write(root.path()).unwrap();
            db.begin_source_directory_truth_generation(1, 1).unwrap();
            db.stage_source_directory_truth_entries(1, &[directory("valid", "dir-valid")])
                .unwrap();
            db.finalize_source_directory_truth_generation(1, 0).unwrap();

            db.connection
                .execute(
                    "UPDATE source_directory_entries
                     SET path = ?1, path_encoding = 1
                     WHERE generation = 1",
                    [path],
                )
                .unwrap_or_else(|error| panic!("persist {expected_description}: {error}"));

            assert_eq!(
                db.source_directory_truth_state().unwrap(),
                SourceDirectoryTruthState::Unavailable {
                    reason: SourceDirectoryTruthUnavailableReason::AuditRequired,
                }
            );

            let error = db.source_directory_truth_page(None, 1).unwrap_err();
            assert_eq!(
                directory_error(error),
                SourceDirectoryTruthError::RequiresAudit {
                    reason: SourceDirectoryTruthUnavailableReason::AuditRequired,
                }
            );

            let error = db
                .finalize_source_directory_truth_generation(1, 0)
                .unwrap_err();
            assert_eq!(
                directory_error(error),
                SourceDirectoryTruthError::RequiresAudit {
                    reason: SourceDirectoryTruthUnavailableReason::AuditRequired,
                }
            );
        }
    }

    #[test]
    fn finalization_rejects_corrupt_staging_rows_before_publication() {
        for (path, path_encoding, directory_identity) in [
            ("valid/~wavecrate-escaped~2f", 1, "dir-valid"),
            ("valid/~wavecrate-escaped~616c696173", 1, "dir-valid"),
            ("valid", 0, "\u{1}"),
        ] {
            let root = tempfile::tempdir().unwrap();
            let db = SourceDatabase::open_for_source_write(root.path()).unwrap();
            db.begin_source_directory_truth_generation(1, 1).unwrap();
            db.stage_source_directory_truth_entries(1, &[directory("valid", "dir-valid")])
                .unwrap();

            db.connection
                .execute(
                    "UPDATE source_directory_entries
                     SET path = ?1, path_encoding = ?2, directory_identity = ?3
                     WHERE generation = 1",
                    rusqlite::params![path, path_encoding, directory_identity],
                )
                .unwrap();

            let error = db
                .finalize_source_directory_truth_generation(1, 0)
                .unwrap_err();
            assert_eq!(
                directory_error(error),
                SourceDirectoryTruthError::RequiresAudit {
                    reason: SourceDirectoryTruthUnavailableReason::AuditRequired,
                }
            );
            assert_eq!(db.get_revision().unwrap(), 0);
            assert_eq!(
                db.source_directory_truth_state().unwrap(),
                SourceDirectoryTruthState::Uninitialized
            );
            let active_generation_count: i64 = db
                .connection
                .query_row(
                    "SELECT COUNT(*)
                     FROM source_directory_generations
                     WHERE status = 'active'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(active_generation_count, 0);
        }
    }

    #[test]
    fn idempotent_finalization_rejects_active_revision_ahead_of_metadata() {
        let root = tempfile::tempdir().unwrap();
        let db = SourceDatabase::open_for_source_write(root.path()).unwrap();
        db.begin_source_directory_truth_generation(1, 1).unwrap();
        db.stage_source_directory_truth_entries(1, &[directory("valid", "dir-valid")])
            .unwrap();
        db.finalize_source_directory_truth_generation(1, 0).unwrap();

        db.connection
            .execute(
                "UPDATE source_directory_generations
                 SET published_source_revision = 2
                 WHERE generation = 1",
                [],
            )
            .unwrap();

        let revision_before = db.get_revision().unwrap();
        let error = db
            .finalize_source_directory_truth_generation(1, 0)
            .unwrap_err();
        assert_eq!(
            directory_error(error),
            SourceDirectoryTruthError::RequiresAudit {
                reason: SourceDirectoryTruthUnavailableReason::Malformed,
            }
        );
        assert_eq!(db.get_revision().unwrap(), revision_before);
        assert_eq!(
            db.source_directory_truth_state().unwrap(),
            SourceDirectoryTruthState::Unavailable {
                reason: SourceDirectoryTruthUnavailableReason::Malformed,
            }
        );
    }

    #[test]
    fn reopening_source_database_preserves_active_state_and_pages() {
        let root = tempfile::tempdir().unwrap();
        let expected = vec![
            directory("samples/kicks", "dir-kicks"),
            directory("samples/snare", "dir-snare"),
        ];

        {
            let db = SourceDatabase::open_for_source_write(root.path()).unwrap();
            db.begin_source_directory_truth_generation(1, expected.len() as u64)
                .unwrap();
            db.stage_source_directory_truth_entries(1, &expected)
                .unwrap();
            db.finalize_source_directory_truth_generation(1, 0).unwrap();
        }

        let reopened = SourceDatabase::open_for_source_write(root.path()).unwrap();
        assert_eq!(
            reopened.source_directory_truth_state().unwrap(),
            SourceDirectoryTruthState::Active {
                generation: 1,
                published_source_revision: 1,
            }
        );
        let first = reopened.source_directory_truth_page(None, 1).unwrap();
        assert_eq!(first.entries, vec![expected[0].clone()]);
        let second = reopened
            .source_directory_truth_page(first.next_cursor.as_ref(), 1)
            .unwrap();
        assert_eq!(second.entries, vec![expected[1].clone()]);
        assert!(second.next_cursor.is_none());
    }

    #[test]
    fn finalization_flips_active_generation_and_cleanup_is_revision_neutral() {
        let root = tempfile::tempdir().unwrap();
        let db = SourceDatabase::open_for_source_write(root.path()).unwrap();
        db.begin_source_directory_truth_generation(1, 1).unwrap();
        db.stage_source_directory_truth_entries(1, &[directory("one", "dir-1")])
            .unwrap();
        db.finalize_source_directory_truth_generation(1, 0).unwrap();

        db.begin_source_directory_truth_generation(2, 1).unwrap();
        db.stage_source_directory_truth_entries(2, &[directory("two", "dir-2")])
            .unwrap();
        db.finalize_source_directory_truth_generation(2, 1).unwrap();
        assert_eq!(db.get_revision().unwrap(), 2);
        assert_eq!(
            db.source_directory_truth_state().unwrap(),
            SourceDirectoryTruthState::Active {
                generation: 2,
                published_source_revision: 2,
            }
        );

        let cleanup = db.cleanup_source_directory_truth(1).unwrap();
        assert_eq!(cleanup.deleted_generations, 1);
        assert_eq!(cleanup.deleted_entries, 1);
        assert!(!cleanup.more_work);
        assert_eq!(db.get_revision().unwrap(), 2);
    }

    #[test]
    fn staging_does_not_scan_large_generation() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        const STAGING_QUERY_WORK_BUDGET: usize = 2_000;

        let root = tempfile::tempdir().unwrap();
        let db = SourceDatabase::open_for_source_write(root.path()).unwrap();
        let entry_count = super::super::SOURCE_DIRECTORY_TRUTH_BATCH_LIMIT * 8;
        let existing_entries = (0..entry_count - 1)
            .map(|index| directory(&format!("large/{index:04}"), &format!("dir-{index}")))
            .collect::<Vec<_>>();

        db.begin_source_directory_truth_generation(1, entry_count as u64)
            .unwrap();
        for batch in existing_entries.chunks(super::super::SOURCE_DIRECTORY_TRUTH_BATCH_LIMIT) {
            db.stage_source_directory_truth_entries(1, batch).unwrap();
        }

        let query_work = Arc::new(AtomicUsize::new(0));
        db.connection.progress_handler(
            1,
            Some({
                let query_work = Arc::clone(&query_work);
                move || query_work.fetch_add(1, Ordering::Relaxed) >= STAGING_QUERY_WORK_BUDGET
            }),
        );
        let stage_result = db.stage_source_directory_truth_entries(
            1,
            &[directory(
                &format!("large/{:04}", entry_count - 1),
                &format!("dir-{}", entry_count - 1),
            )],
        );
        db.connection.progress_handler(0, None::<fn() -> bool>);

        stage_result.expect("staging one bounded batch must not scan the generation");
        assert!(
            query_work.load(Ordering::Relaxed) < STAGING_QUERY_WORK_BUDGET,
            "bounded staging exceeded query-work budget"
        );
        db.finalize_source_directory_truth_generation(1, 0).unwrap();
    }

    #[test]
    fn cleanup_bounds_large_inactive_generation_without_advancing_revision() {
        const CLEANUP_QUERY_WORK_BUDGET: usize = 8_000;

        let root = tempfile::tempdir().unwrap();
        let db = SourceDatabase::open_for_source_write(root.path()).unwrap();
        let entry_count = super::CLEANUP_ENTRY_LIMIT * 32;
        let entries = (0..entry_count)
            .map(|index| directory(&format!("large/{index:04}"), &format!("dir-{index}")))
            .collect::<Vec<_>>();

        db.begin_source_directory_truth_generation(1, entry_count as u64)
            .unwrap();
        for batch in entries.chunks(super::super::SOURCE_DIRECTORY_TRUTH_BATCH_LIMIT) {
            db.stage_source_directory_truth_entries(1, batch).unwrap();
        }
        db.finalize_source_directory_truth_generation(1, 0).unwrap();

        db.begin_source_directory_truth_generation(2, 1).unwrap();
        db.stage_source_directory_truth_entries(2, &[directory("current", "dir-current")])
            .unwrap();
        db.finalize_source_directory_truth_generation(2, 1).unwrap();
        let revision_before_cleanup = db.get_revision().unwrap();

        let query_work = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        db.connection.progress_handler(
            1,
            Some({
                let query_work = std::sync::Arc::clone(&query_work);
                move || {
                    query_work.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                        >= CLEANUP_QUERY_WORK_BUDGET
                }
            }),
        );
        let cleanup_result = db.cleanup_source_directory_truth(1);
        db.connection.progress_handler(0, None::<fn() -> bool>);

        let cleanup = cleanup_result.expect("cleanup must stay bounded for a large generation");
        assert_eq!(cleanup.deleted_entries, super::CLEANUP_ENTRY_LIMIT);
        assert_eq!(cleanup.deleted_generations, 0);
        assert!(cleanup.more_work);
        assert_eq!(db.get_revision().unwrap(), revision_before_cleanup);
        assert!(
            query_work.load(std::sync::atomic::Ordering::Relaxed) < CLEANUP_QUERY_WORK_BUDGET,
            "bounded cleanup exceeded query-work budget"
        );
    }

    #[test]
    fn cleanup_probes_empty_candidates_after_entry_budget_is_exhausted() {
        let root = tempfile::tempdir().unwrap();
        let db = SourceDatabase::open_for_source_write(root.path()).unwrap();
        let entry_count = super::CLEANUP_ENTRY_LIMIT + 1;
        let entries = (0..entry_count)
            .map(|index| directory(&format!("large/{index:03}"), &format!("dir-{index}")))
            .collect::<Vec<_>>();

        db.begin_source_directory_truth_generation(1, entry_count as u64)
            .unwrap();
        for batch in entries.chunks(super::super::SOURCE_DIRECTORY_TRUTH_BATCH_LIMIT) {
            db.stage_source_directory_truth_entries(1, batch).unwrap();
        }
        db.finalize_source_directory_truth_generation(1, 0).unwrap();

        db.begin_source_directory_truth_generation(2, 0).unwrap();
        db.finalize_source_directory_truth_generation(2, 1).unwrap();

        db.begin_source_directory_truth_generation(3, 1).unwrap();
        db.stage_source_directory_truth_entries(3, &[directory("current", "dir-current")])
            .unwrap();
        db.finalize_source_directory_truth_generation(3, 2).unwrap();

        let cleanup = db.cleanup_source_directory_truth(2).unwrap();
        assert_eq!(cleanup.deleted_entries, super::CLEANUP_ENTRY_LIMIT);
        assert_eq!(cleanup.deleted_generations, 1);
        assert!(cleanup.more_work);
        let generation_two_exists: bool = db
            .connection
            .query_row(
                "SELECT EXISTS(
                    SELECT 1
                    FROM source_directory_generations
                    WHERE generation = 2
                )",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap()
            != 0;
        assert!(!generation_two_exists);
    }

    #[test]
    fn small_page_does_not_scan_large_active_generation() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        const PAGE_QUERY_WORK_BUDGET: usize = 2_000;

        let root = tempfile::tempdir().unwrap();
        let db = SourceDatabase::open_for_source_write(root.path()).unwrap();
        let entry_count = super::super::SOURCE_DIRECTORY_TRUTH_BATCH_LIMIT * 8;
        db.begin_source_directory_truth_generation(1, entry_count as u64)
            .unwrap();
        for start in (0..entry_count).step_by(super::super::SOURCE_DIRECTORY_TRUTH_BATCH_LIMIT) {
            let entries = (start..start + super::super::SOURCE_DIRECTORY_TRUTH_BATCH_LIMIT)
                .map(|index| directory(&format!("large/{index:04}"), &format!("dir-{index}")))
                .collect::<Vec<_>>();
            db.stage_source_directory_truth_entries(1, &entries)
                .unwrap();
        }
        db.finalize_source_directory_truth_generation(1, 0).unwrap();

        let full_query_work = Arc::new(AtomicUsize::new(0));
        db.connection.progress_handler(
            1,
            Some({
                let full_query_work = Arc::clone(&full_query_work);
                move || {
                    full_query_work.fetch_add(1, Ordering::Relaxed);
                    false
                }
            }),
        );
        db.source_directory_truth_state().unwrap();
        db.connection.progress_handler(0, None::<fn() -> bool>);
        assert!(
            full_query_work.load(Ordering::Relaxed) > PAGE_QUERY_WORK_BUDGET,
            "the full integrity inspection should exceed the bounded page budget"
        );

        let query_work = Arc::new(AtomicUsize::new(0));
        db.connection.progress_handler(
            1,
            Some({
                let query_work = Arc::clone(&query_work);
                move || query_work.fetch_add(1, Ordering::Relaxed) >= PAGE_QUERY_WORK_BUDGET
            }),
        );
        let page_result = db.source_directory_truth_page(None, 1);
        db.connection.progress_handler(0, None::<fn() -> bool>);

        let page = page_result.expect("a small page must stay within its query-work budget");
        assert_eq!(page.entries.len(), 1);
        assert!(page.next_cursor.is_some());
        assert!(
            query_work.load(Ordering::Relaxed) < PAGE_QUERY_WORK_BUDGET,
            "bounded page exceeded query-work budget"
        );
    }

    #[test]
    fn stale_incomplete_duplicate_collision_and_invalid_inputs_fail_closed() {
        let root = tempfile::tempdir().unwrap();
        let db = SourceDatabase::open_for_source_write(root.path()).unwrap();

        db.begin_source_directory_truth_generation(1, 2).unwrap();
        let error = db
            .stage_source_directory_truth_entries(
                1,
                &[directory("same", "dir-1"), directory("same", "dir-2")],
            )
            .unwrap_err();
        assert!(matches!(
            directory_error(error),
            SourceDirectoryTruthError::DuplicatePath { .. }
        ));
        let error = db
            .stage_source_directory_truth_entries(
                1,
                &[directory("one", "dir-1"), directory("two", "dir-1")],
            )
            .unwrap_err();
        assert!(matches!(
            directory_error(error),
            SourceDirectoryTruthError::DuplicateDirectoryIdentity { .. }
        ));
        let error = db
            .stage_source_directory_truth_entries(1, &[directory("../escape", "dir-1")])
            .unwrap_err();
        assert!(matches!(
            directory_error(error),
            SourceDirectoryTruthError::InvalidPath { .. }
        ));
        let error = db
            .stage_source_directory_truth_entries(1, &[directory("one", " ")])
            .unwrap_err();
        assert!(matches!(
            directory_error(error),
            SourceDirectoryTruthError::InvalidDirectoryIdentity
        ));

        let error = db
            .finalize_source_directory_truth_generation(1, 0)
            .unwrap_err();
        assert!(matches!(
            directory_error(error),
            SourceDirectoryTruthError::Incomplete { .. }
        ));
        let error = db
            .finalize_source_directory_truth_generation(99, 0)
            .unwrap_err();
        assert!(matches!(
            directory_error(error),
            SourceDirectoryTruthError::GenerationMissing { .. }
        ));

        db.stage_source_directory_truth_entries(
            1,
            &[directory("one", "dir-1"), directory("two", "dir-2")],
        )
        .unwrap();
        db.finalize_source_directory_truth_generation(1, 0).unwrap();
        let error = db.finalize_source_directory_truth_generation(1, 0).unwrap();
        assert!(error.idempotent);

        db.begin_source_directory_truth_generation(2, 1).unwrap();
        db.stage_source_directory_truth_entries(2, &[directory("two", "dir-2")])
            .unwrap();
        let error = db
            .finalize_source_directory_truth_generation(2, 0)
            .unwrap_err();
        assert!(matches!(
            directory_error(error),
            SourceDirectoryTruthError::StaleRevision { .. }
        ));
        let error = db
            .begin_source_directory_truth_generation(1, 1)
            .unwrap_err();
        assert!(matches!(
            directory_error(error),
            SourceDirectoryTruthError::GenerationCollision { .. }
        ));
    }

    #[test]
    fn missing_directory_truth_schema_is_read_compatible_without_creation() {
        let root = tempfile::tempdir().unwrap();
        let db_path = root.path().join(super::super::DB_FILE_NAME);
        let connection = rusqlite::Connection::open(&db_path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                 CREATE TABLE wav_files (
                     path TEXT PRIMARY KEY, file_size INTEGER NOT NULL, modified_ns INTEGER NOT NULL
                 );
                 PRAGMA user_version = 11;",
            )
            .unwrap();
        drop(connection);

        let db = SourceDatabase::open_for_ui_read(root.path()).unwrap();
        assert_eq!(
            db.source_directory_truth_state().unwrap(),
            SourceDirectoryTruthState::Unavailable {
                reason: super::super::SourceDirectoryTruthUnavailableReason::SchemaUnavailable,
            }
        );
        let page = db.source_directory_truth_page(None, 32).unwrap();
        assert!(page.entries.is_empty());
        let table_count: i64 = db
            .connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name LIKE 'source_directory_%'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 0);
    }
}
