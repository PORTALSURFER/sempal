use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

use super::schema;
use super::util::{
    EXACT_SUBTREE_PATH_PREDICATE, exact_subtree_path_bounds, map_sql_error,
    normalize_source_index_path, parse_source_index_path_from_db,
};
use super::{
    META_SOURCE_INDEX_REVISION, SourceDatabase, SourceDbError, SourceIndexClassification,
    SourceIndexDiagnostic, SourceIndexEntry, SourceIndexSnapshot, SourceWriteBatch,
};

const REQUIRED_COLUMNS: [&str; 7] = [
    "path",
    "classification",
    "file_size",
    "modified_ns",
    "file_identity",
    "diagnostic",
    "format_policy_version",
];

impl SourceDatabase {
    /// Read the complete index-only file set and its independent revision atomically.
    ///
    /// Legacy read-only databases without the table project revision zero and
    /// an empty set instead of requiring a migration on the reader.
    pub fn source_index_snapshot(&self) -> Result<SourceIndexSnapshot, SourceDbError> {
        if !source_index_schema_available(&self.connection)? {
            return Ok(SourceIndexSnapshot {
                revision: 0,
                entries: Vec::new(),
            });
        }
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(map_sql_error)?;
        let revision = read_index_revision(&transaction)?;
        let path_encoding = source_index_path_encoding_available(&transaction)?;
        let entries = collect_entries(
            &transaction,
            &source_index_select_sql(path_encoding, false),
            [],
        )?;
        transaction.rollback().map_err(map_sql_error)?;
        Ok(SourceIndexSnapshot { revision, entries })
    }

    /// Read all durable index-only entries in deterministic path order.
    pub fn list_source_index_entries(&self) -> Result<Vec<SourceIndexEntry>, SourceDbError> {
        Ok(self.source_index_snapshot()?.entries)
    }

    /// Read index-only entries at or below one source-relative path.
    pub fn list_source_index_entries_under_path(
        &self,
        relative_path: &Path,
    ) -> Result<Vec<SourceIndexEntry>, SourceDbError> {
        if !source_index_schema_available(&self.connection)? {
            return Ok(Vec::new());
        }
        let (normalized, _path_encoding) = normalize_source_index_path(relative_path)?;
        let (lower_bound, upper_bound) = exact_subtree_path_bounds(&normalized);
        let path_encoding = source_index_path_encoding_available(&self.connection)?;
        collect_entries(
            &self.connection,
            &source_index_select_sql(path_encoding, true),
            params![normalized, lower_bound, upper_bound],
        )
    }
}

impl SourceWriteBatch<'_> {
    /// Insert or update one index-only file without creating sample metadata.
    pub fn upsert_source_index_entry(
        &mut self,
        entry: &SourceIndexEntry,
    ) -> Result<(), SourceDbError> {
        validate_entry(entry)?;
        let (path, path_encoding) = normalize_source_index_path(&entry.relative_path)?;
        let live_manifest_row = self
            .tx
            .query_row(
                "SELECT 1 FROM wav_files WHERE path = ?1 AND missing = 0",
                [&path],
                |_| Ok(()),
            )
            .optional()
            .map_err(map_sql_error)?
            .is_some();
        if live_manifest_row {
            return Err(SourceDbError::Unexpected);
        }
        let changed = self
            .tx
            .execute(
                "INSERT INTO source_index_entries (
                    path, path_encoding, classification, file_size, modified_ns, file_identity,
                    diagnostic, format_policy_version
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(path) DO UPDATE SET
                    path_encoding = excluded.path_encoding,
                    classification = excluded.classification,
                    file_size = excluded.file_size,
                    modified_ns = excluded.modified_ns,
                    file_identity = excluded.file_identity,
                    diagnostic = excluded.diagnostic,
                    format_policy_version = excluded.format_policy_version
                 WHERE classification IS NOT excluded.classification
                    OR file_size IS NOT excluded.file_size
                    OR modified_ns IS NOT excluded.modified_ns
                    OR file_identity IS NOT excluded.file_identity
                    OR diagnostic IS NOT excluded.diagnostic
                    OR format_policy_version IS NOT excluded.format_policy_version",
                params![
                    path,
                    path_encoding,
                    entry.classification.token(),
                    entry.file_size.map(saturating_i64),
                    entry.modified_ns,
                    entry.file_identity,
                    entry.diagnostic.map(SourceIndexDiagnostic::token),
                    i64::from(entry.format_policy_version),
                ],
            )
            .map_err(map_sql_error)?;
        self.index_revision_dirty |= changed > 0;
        Ok(())
    }

    /// Remove one index-only row, including during promotion to the supported manifest.
    pub fn remove_source_index_entry(&mut self, relative_path: &Path) -> Result<(), SourceDbError> {
        let (path, _path_encoding) = normalize_source_index_path(relative_path)?;
        let changed = self
            .tx
            .execute("DELETE FROM source_index_entries WHERE path = ?1", [path])
            .map_err(map_sql_error)?;
        self.index_revision_dirty |= changed > 0;
        Ok(())
    }
}

fn source_index_schema_available(connection: &Connection) -> Result<bool, SourceDbError> {
    let columns = schema::table_columns(connection, "source_index_entries")?;
    Ok(REQUIRED_COLUMNS
        .iter()
        .all(|column| columns.contains(*column)))
}

fn source_index_path_encoding_available(connection: &Connection) -> Result<bool, SourceDbError> {
    Ok(schema::table_columns(connection, "source_index_entries")?.contains("path_encoding"))
}

fn source_index_select_sql(path_encoding: bool, under_path: bool) -> String {
    let encoding = if path_encoding {
        "path_encoding"
    } else {
        "0 AS path_encoding"
    };
    let predicate = if under_path {
        format!("WHERE {EXACT_SUBTREE_PATH_PREDICATE}")
    } else {
        String::new()
    };
    format!(
        "SELECT path, {encoding}, classification, file_size, modified_ns, file_identity,
                diagnostic, format_policy_version
         FROM source_index_entries
         {predicate}
         ORDER BY path ASC"
    )
}

fn read_index_revision(connection: &Connection) -> Result<u64, SourceDbError> {
    connection
        .query_row(
            "SELECT value FROM metadata WHERE key = ?1",
            [META_SOURCE_INDEX_REVISION],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(map_sql_error)?
        .map(|raw| raw.parse::<u64>().map_err(|_| SourceDbError::Unexpected))
        .transpose()
        .map(Option::unwrap_or_default)
}

fn collect_entries(
    connection: &Connection,
    sql: &str,
    query_params: impl rusqlite::Params,
) -> Result<Vec<SourceIndexEntry>, SourceDbError> {
    let mut statement = connection.prepare(sql).map_err(map_sql_error)?;
    let rows = statement
        .query_map(query_params, |row| {
            let raw_path: String = row.get(0)?;
            let path_encoding: i64 = row.get(1)?;
            let relative_path = match parse_source_index_path_from_db(&raw_path, path_encoding) {
                Ok(path) => path,
                Err(error) => {
                    tracing::warn!(
                        path = raw_path,
                        %error,
                        "Skipping source index row with invalid relative path"
                    );
                    return Ok(None);
                }
            };
            let raw_classification: String = row.get(2)?;
            let Some(classification) = SourceIndexClassification::from_token(&raw_classification)
            else {
                tracing::warn!(
                    classification = raw_classification,
                    "Skipping source index row with invalid classification"
                );
                return Ok(None);
            };
            let diagnostic = row
                .get::<_, Option<String>>(6)?
                .as_deref()
                .and_then(SourceIndexDiagnostic::from_token);
            Ok(Some(SourceIndexEntry {
                relative_path,
                classification,
                file_size: row
                    .get::<_, Option<i64>>(3)?
                    .map(|value| value.max(0) as u64),
                modified_ns: row.get(4)?,
                file_identity: row.get(5)?,
                diagnostic,
                format_policy_version: row.get::<_, i64>(7)?.clamp(0, i64::from(u32::MAX)) as u32,
            }))
        })
        .map_err(map_sql_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_sql_error)?;
    Ok(rows.into_iter().flatten().collect())
}

fn validate_entry(entry: &SourceIndexEntry) -> Result<(), SourceDbError> {
    let complete_facts = entry.file_size.is_some() && entry.modified_ns.is_some();
    let valid = match entry.classification {
        SourceIndexClassification::UnsupportedAudio
        | SourceIndexClassification::UnsupportedNonAudio => {
            complete_facts && entry.diagnostic.is_none()
        }
        SourceIndexClassification::Inaccessible => entry.diagnostic.is_some(),
        SourceIndexClassification::PracticallyUnsupportedAudio => {
            complete_facts && entry.diagnostic == Some(SourceIndexDiagnostic::PracticalSupportLimit)
        }
    };
    if valid {
        Ok(())
    } else {
        Err(SourceDbError::Unexpected)
    }
}

fn saturating_i64(value: u64) -> i64 {
    value.min(i64::MAX as u64) as i64
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::sample_sources::SOURCE_FORMAT_POLICY_VERSION;

    #[test]
    fn index_only_writes_advance_only_the_index_revision() {
        let directory = tempfile::tempdir().expect("source root");
        let database =
            SourceDatabase::open_for_source_write(directory.path()).expect("source database");
        let manifest_revision = database.get_revision().expect("manifest revision");
        let entry = SourceIndexEntry {
            relative_path: PathBuf::from("notes.txt"),
            classification: SourceIndexClassification::UnsupportedNonAudio,
            file_size: Some(5),
            modified_ns: Some(10),
            file_identity: None,
            diagnostic: None,
            format_policy_version: SOURCE_FORMAT_POLICY_VERSION,
        };
        let mut batch = database.write_batch().expect("index batch");
        batch
            .upsert_source_index_entry(&entry)
            .expect("upsert index entry");
        batch
            .commit_auxiliary_state()
            .expect("commit index-only state");

        let snapshot = database.source_index_snapshot().expect("index snapshot");
        assert_eq!(snapshot.revision, 1);
        assert_eq!(snapshot.entries, vec![entry]);
        assert_eq!(
            database.get_revision().expect("manifest revision"),
            manifest_revision
        );
    }

    #[test]
    fn live_manifest_and_index_only_rows_cannot_share_a_path() {
        let directory = tempfile::tempdir().expect("source root");
        let database =
            SourceDatabase::open_for_source_write(directory.path()).expect("source database");
        database
            .upsert_file(Path::new("sample.wav"), 5, 10)
            .expect("supported row");
        let entry = SourceIndexEntry {
            relative_path: PathBuf::from("sample.wav"),
            classification: SourceIndexClassification::Inaccessible,
            file_size: None,
            modified_ns: None,
            file_identity: None,
            diagnostic: Some(SourceIndexDiagnostic::OpenUnavailable),
            format_policy_version: SOURCE_FORMAT_POLICY_VERSION,
        };
        let mut batch = database.write_batch().expect("index batch");
        assert!(matches!(
            batch.upsert_source_index_entry(&entry),
            Err(SourceDbError::Unexpected)
        ));
    }

    #[test]
    fn practical_support_and_inaccessible_diagnostics_round_trip() {
        let directory = tempfile::tempdir().expect("source root");
        let database =
            SourceDatabase::open_for_source_write(directory.path()).expect("source database");
        let entries = [
            SourceIndexEntry {
                relative_path: PathBuf::from("too-long.wav"),
                classification: SourceIndexClassification::PracticallyUnsupportedAudio,
                file_size: Some(1_000),
                modified_ns: Some(20),
                file_identity: Some(String::from("file-1")),
                diagnostic: Some(SourceIndexDiagnostic::PracticalSupportLimit),
                format_policy_version: SOURCE_FORMAT_POLICY_VERSION,
            },
            SourceIndexEntry {
                relative_path: PathBuf::from("unknown.bin"),
                classification: SourceIndexClassification::Inaccessible,
                file_size: None,
                modified_ns: None,
                file_identity: None,
                diagnostic: Some(SourceIndexDiagnostic::EntryTypeUnavailable),
                format_policy_version: SOURCE_FORMAT_POLICY_VERSION,
            },
        ];
        let mut batch = database.write_batch().expect("index batch");
        for entry in &entries {
            batch
                .upsert_source_index_entry(entry)
                .expect("upsert index entry");
        }
        batch.commit_auxiliary_state().expect("commit index rows");

        assert_eq!(
            database
                .list_source_index_entries()
                .expect("read index rows"),
            entries
        );
    }

    #[test]
    fn subtree_reads_treat_sql_wildcards_as_literal_path_characters() {
        let directory = tempfile::tempdir().expect("source root");
        let database =
            SourceDatabase::open_for_source_write(directory.path()).expect("source database");
        let entries = [
            "literal%_!/inside.txt",
            "literal%_!/nested/deep.txt",
            "literalx_Yx!/outside.txt",
            "Literal%_!/case-distinct.txt",
        ]
        .map(|path| SourceIndexEntry {
            relative_path: PathBuf::from(path),
            classification: SourceIndexClassification::UnsupportedNonAudio,
            file_size: Some(5),
            modified_ns: Some(10),
            file_identity: None,
            diagnostic: None,
            format_policy_version: SOURCE_FORMAT_POLICY_VERSION,
        });
        let mut batch = database.write_batch().expect("index batch");
        for entry in &entries {
            batch
                .upsert_source_index_entry(entry)
                .expect("upsert index entry");
        }
        batch.commit_auxiliary_state().expect("commit index rows");

        assert_eq!(
            database
                .list_source_index_entries_under_path(Path::new("literal%_!"))
                .expect("read literal subtree"),
            vec![entries[0].clone(), entries[1].clone()]
        );
    }

    #[cfg(unix)]
    #[test]
    fn non_unicode_index_paths_and_diagnostics_round_trip_without_aliasing() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let directory = tempfile::tempdir().expect("source root");
        let database =
            SourceDatabase::open_for_source_write(directory.path()).expect("source database");
        let path = PathBuf::from_iter([
            OsString::from("folder"),
            OsString::from_vec(b"raw-\xFF.wav".to_vec()),
        ]);
        let entry = SourceIndexEntry {
            relative_path: path.clone(),
            classification: SourceIndexClassification::Inaccessible,
            file_size: Some(3),
            modified_ns: Some(10),
            file_identity: Some(String::from("file-raw")),
            diagnostic: Some(SourceIndexDiagnostic::NonUnicodePath),
            format_policy_version: SOURCE_FORMAT_POLICY_VERSION,
        };
        let mut batch = database.write_batch().expect("index batch");
        batch
            .upsert_source_index_entry(&entry)
            .expect("upsert raw index entry");
        batch.commit_auxiliary_state().expect("commit index row");

        assert_eq!(
            database.list_source_index_entries().unwrap(),
            vec![entry.clone()]
        );
        assert_eq!(
            database
                .list_source_index_entries_under_path(Path::new("folder"))
                .unwrap(),
            vec![entry]
        );
    }
}
