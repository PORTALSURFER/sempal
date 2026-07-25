use std::{collections::HashMap, path::Path};

use rusqlite::OptionalExtension;

use super::super::util::map_sql_error;
use super::super::{
    BrowserFileMetadata, BrowserMetadataCursor, BrowserMetadataPage, BrowserMetadataSnapshot,
    META_WAV_PATHS_REVISION, Rating, SampleCollection, SampleSoundType, SourceDatabase,
    SourceDbError,
};
use super::decode::{
    decode_path_row, decode_relative_path, table_has_columns, wav_file_has_column,
};

struct BrowserMetadataCapabilities {
    wav_columns: std::collections::HashSet<String>,
    collection_memberships: bool,
    metadata_revision: bool,
}

fn optional_browser_column<'a>(
    columns: &std::collections::HashSet<String>,
    column: &'a str,
    fallback: &'a str,
) -> &'a str {
    if columns.contains(column) {
        column
    } else {
        fallback
    }
}

fn browser_metadata_snapshot_with_observer(
    db: &SourceDatabase,
    mut statement_started: impl FnMut(),
) -> Result<BrowserMetadataSnapshot, SourceDbError> {
    let transaction = db
        .connection
        .unchecked_transaction()
        .map_err(map_sql_error)?;

    statement_started();
    let wav_columns = super::super::schema::table_columns(&transaction, "wav_files")?;
    statement_started();
    let collection_columns =
        super::super::schema::table_columns(&transaction, "wav_file_collections")?;
    statement_started();
    let metadata_columns = super::super::schema::table_columns(&transaction, "metadata")?;
    let capabilities = BrowserMetadataCapabilities {
        wav_columns,
        collection_memberships: collection_columns.contains("path")
            && collection_columns.contains("collection"),
        metadata_revision: metadata_columns.contains("key") && metadata_columns.contains("value"),
    };

    let revision = if capabilities.metadata_revision {
        statement_started();
        transaction
            .query_row(
                "SELECT value FROM metadata WHERE key = 'revision'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(map_sql_error)?
            .map(|raw| raw.parse::<u64>().map_err(|_| SourceDbError::Unexpected))
            .transpose()?
            .unwrap_or_default()
    } else {
        0
    };

    let supported_filter = if capabilities.wav_columns.contains("extension") {
        crate::sample_sources::supported_audio_where_clause()
    } else {
        String::from("lower(path) GLOB '*.wav' AND path NOT GLOB '._*' AND path NOT GLOB '*/._*'")
    };
    let legacy_collection = optional_browser_column(
        &capabilities.wav_columns,
        "collection",
        "NULL AS collection",
    );
    let sql = format!(
        "SELECT path, {}, {}, {}, {}, {legacy_collection}, {}, {}, {}
         FROM wav_files
         WHERE {supported_filter}
         ORDER BY path ASC",
        optional_browser_column(&capabilities.wav_columns, "tag", "0 AS tag"),
        optional_browser_column(&capabilities.wav_columns, "locked", "0 AS locked"),
        optional_browser_column(
            &capabilities.wav_columns,
            "last_played_at",
            "NULL AS last_played_at"
        ),
        optional_browser_column(
            &capabilities.wav_columns,
            "last_curated_at",
            "NULL AS last_curated_at"
        ),
        optional_browser_column(&capabilities.wav_columns, "file_size", "0 AS file_size"),
        optional_browser_column(&capabilities.wav_columns, "modified_ns", "0 AS modified_ns"),
        optional_browser_column(&capabilities.wav_columns, "missing", "0 AS missing"),
    );
    statement_started();
    let mut files = {
        let mut statement = transaction.prepare(&sql).map_err(map_sql_error)?;
        statement
            .query_map([], |row| {
                let Some(relative_path) = decode_path_row(
                    row,
                    "Skipping browser metadata row with invalid relative path",
                )?
                else {
                    return Ok(None);
                };
                let legacy_collection = if capabilities.collection_memberships {
                    Vec::new()
                } else {
                    row.get::<_, Option<i64>>(5)?
                        .and_then(SampleCollection::from_i64)
                        .into_iter()
                        .collect()
                };
                Ok(Some(BrowserFileMetadata {
                    relative_path,
                    file_size: u64::try_from(row.get::<_, i64>(6)?).unwrap_or_default(),
                    modified_ns: row.get(7)?,
                    missing: row.get::<_, i64>(8)? != 0,
                    rating: Rating::from_i64(row.get(1)?),
                    locked: row.get::<_, i64>(2)? != 0,
                    collections: legacy_collection,
                    last_played_at: row.get(3)?,
                    last_curated_at: row.get(4)?,
                }))
            })
            .map_err(map_sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_sql_error)?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
    };

    if capabilities.collection_memberships {
        statement_started();
        let memberships = {
            let mut statement = transaction
                .prepare(
                    "SELECT path, collection
                     FROM wav_file_collections
                     ORDER BY path ASC, collection ASC",
                )
                .map_err(map_sql_error)?;
            statement
                .query_map([], |row| {
                    let path = row.get::<_, String>(0)?;
                    Ok((
                        decode_relative_path(
                            path,
                            "Skipping browser collection row with invalid relative path",
                        )?,
                        row.get::<_, i64>(1)?,
                    ))
                })
                .map_err(map_sql_error)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sql_error)?
        };
        let by_path = files
            .iter_mut()
            .map(|file| (file.relative_path.clone(), file))
            .collect::<HashMap<_, _>>();
        let mut by_path = by_path;
        for (path, collection) in memberships {
            let Some(path) = path else {
                continue;
            };
            if let (Some(file), Some(collection)) = (
                by_path.get_mut(&path),
                SampleCollection::from_i64(collection),
            ) {
                file.collections.push(collection);
            }
        }
    }

    transaction.rollback().map_err(map_sql_error)?;
    Ok(BrowserMetadataSnapshot { revision, files })
}

#[cfg(test)]
pub(super) fn browser_metadata_snapshot_statement_count(
    db: &SourceDatabase,
) -> Result<(BrowserMetadataSnapshot, usize), SourceDbError> {
    let mut count = 0;
    let snapshot = browser_metadata_snapshot_with_observer(db, || count += 1)?;
    Ok((snapshot, count))
}

fn normalize_supported_audio_path(path: &Path) -> Result<Option<String>, SourceDbError> {
    if !crate::sample_sources::is_supported_audio(path) {
        return Ok(None);
    }
    super::super::normalize_relative_path(path).map(Some)
}

fn query_flag_for_path(
    db: &SourceDatabase,
    column: &str,
    path_str: &str,
) -> Result<Option<bool>, SourceDbError> {
    let sql = format!("SELECT {column} FROM wav_files WHERE path = ?1");
    let value: Option<i64> = db
        .connection
        .query_row(&sql, rusqlite::params![path_str], |row| row.get(0))
        .optional()
        .map_err(map_sql_error)?;
    Ok(value.map(|flag| flag != 0))
}

impl SourceDatabase {
    /// Fetch all browser metadata from one committed read snapshot.
    ///
    /// Schema capabilities and data are read with a fixed statement budget that is independent
    /// of the number of tracked files.
    pub fn browser_metadata_snapshot(&self) -> Result<BrowserMetadataSnapshot, SourceDbError> {
        browser_metadata_snapshot_with_observer(self, || {})
    }

    /// Fetch one bounded browser metadata page at an exact source revision.
    ///
    /// `after_cursor` is an exclusive raw-SQL keyset cursor. A revision mismatch fails closed so pages
    /// cannot be combined across committed source states.
    pub fn browser_metadata_page(
        &self,
        expected_revision: u64,
        after_cursor: Option<&BrowserMetadataCursor>,
        requested_limit: usize,
    ) -> Result<BrowserMetadataPage, SourceDbError> {
        const MAX_PAGE_SIZE: usize = 64;
        let limit = requested_limit.clamp(1, MAX_PAGE_SIZE);
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(map_sql_error)?;
        let revision = transaction
            .query_row(
                "SELECT value FROM metadata WHERE key = 'revision'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(map_sql_error)?
            .map(|raw| raw.parse::<u64>().map_err(|_| SourceDbError::Unexpected))
            .transpose()?
            .unwrap_or_default();
        if revision != expected_revision {
            return Err(SourceDbError::Unexpected);
        }
        let wav_columns = super::super::schema::table_columns(&transaction, "wav_files")?;
        let collection_columns =
            super::super::schema::table_columns(&transaction, "wav_file_collections")?;
        let memberships_available =
            collection_columns.contains("path") && collection_columns.contains("collection");
        let supported_filter = if wav_columns.contains("extension") {
            crate::sample_sources::supported_audio_where_clause()
        } else {
            String::from(
                "lower(path) GLOB '*.wav' AND path NOT GLOB '._*' AND path NOT GLOB '*/._*'",
            )
        };
        let legacy_collection =
            optional_browser_column(&wav_columns, "collection", "NULL AS collection");
        let sql = format!(
            "SELECT path, {}, {}, {}, {}, {legacy_collection}, {}, {}, {}
             FROM wav_files
             WHERE {supported_filter} AND (?1 IS NULL OR path > ?1)
             ORDER BY path ASC LIMIT ?2",
            optional_browser_column(&wav_columns, "tag", "0 AS tag"),
            optional_browser_column(&wav_columns, "locked", "0 AS locked"),
            optional_browser_column(&wav_columns, "last_played_at", "NULL AS last_played_at"),
            optional_browser_column(&wav_columns, "last_curated_at", "NULL AS last_curated_at"),
            optional_browser_column(&wav_columns, "file_size", "0 AS file_size"),
            optional_browser_column(&wav_columns, "modified_ns", "0 AS modified_ns"),
            optional_browser_column(&wav_columns, "missing", "0 AS missing"),
        );
        let raw_rows = {
            let mut statement = transaction.prepare(&sql).map_err(map_sql_error)?;
            statement
                .query_map(
                    rusqlite::params![
                        after_cursor.map(BrowserMetadataCursor::as_str),
                        limit as i64
                    ],
                    |row| {
                        let raw_path: String = row.get(0)?;
                        let Some(relative_path) = decode_relative_path(
                            raw_path.clone(),
                            "Skipping browser metadata row with invalid relative path",
                        )?
                        else {
                            return Ok((raw_path, None));
                        };
                        let legacy_collection = if memberships_available {
                            Vec::new()
                        } else {
                            row.get::<_, Option<i64>>(5)?
                                .and_then(SampleCollection::from_i64)
                                .into_iter()
                                .collect()
                        };
                        Ok((
                            raw_path,
                            Some(BrowserFileMetadata {
                                relative_path,
                                file_size: u64::try_from(row.get::<_, i64>(6)?).unwrap_or_default(),
                                modified_ns: row.get(7)?,
                                missing: row.get::<_, i64>(8)? != 0,
                                rating: Rating::from_i64(row.get(1)?),
                                locked: row.get::<_, i64>(2)? != 0,
                                collections: legacy_collection,
                                last_played_at: row.get(3)?,
                                last_curated_at: row.get(4)?,
                            }),
                        ))
                    },
                )
                .map_err(map_sql_error)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sql_error)?
        };
        let next_cursor = (raw_rows.len() == limit)
            .then(|| {
                raw_rows
                    .last()
                    .map(|(raw_path, _)| BrowserMetadataCursor::from_raw(raw_path.clone()))
            })
            .flatten();
        let mut files = raw_rows
            .into_iter()
            .filter_map(|(_, file)| file)
            .collect::<Vec<_>>();
        if memberships_available && !files.is_empty() {
            let first = super::super::normalize_relative_path(&files[0].relative_path)?;
            let last = super::super::normalize_relative_path(
                &files[files.len().saturating_sub(1)].relative_path,
            )?;
            let mut statement = transaction
                .prepare(
                    "SELECT path, collection FROM wav_file_collections
                     WHERE path >= ?1 AND path <= ?2 ORDER BY path ASC, collection ASC",
                )
                .map_err(map_sql_error)?;
            let memberships = statement
                .query_map(rusqlite::params![first, last], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })
                .map_err(map_sql_error)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sql_error)?;
            for (path, collection) in memberships {
                if let (Some(file), Some(collection)) = (
                    files.iter_mut().find(|file| {
                        super::super::normalize_relative_path(&file.relative_path)
                            .is_ok_and(|candidate| candidate == path)
                    }),
                    SampleCollection::from_i64(collection),
                ) {
                    file.collections.push(collection);
                }
            }
        }
        transaction.rollback().map_err(map_sql_error)?;
        Ok(BrowserMetadataPage {
            revision,
            files,
            next_cursor,
        })
    }
    /// Return the numeric metadata value for `key` (0 if missing).
    fn get_numeric_metadata(&self, key: &str) -> Result<u64, SourceDbError> {
        let value = self.get_metadata(key)?;
        match value {
            Some(raw) => raw.parse::<u64>().map_err(|_| SourceDbError::Unexpected),
            None => Ok(0),
        }
    }

    /// Find the sorted index for a tracked wav path.
    pub fn index_for_path(&self, path: &Path) -> Result<Option<usize>, SourceDbError> {
        let Some(path_str) = normalize_supported_audio_path(path)? else {
            return Ok(None);
        };
        let filter = crate::sample_sources::supported_audio_where_clause();
        let sql = format!(
            "SELECT
                (SELECT COUNT(*) FROM wav_files WHERE {filter} AND path < ?1) AS offset,
                EXISTS(SELECT 1 FROM wav_files WHERE {filter} AND path = ?1) AS path_exists"
        );
        let (offset, exists): (i64, i64) = self
            .connection
            .query_row(&sql, rusqlite::params![path_str.as_str()], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .map_err(map_sql_error)?;
        if exists == 0 {
            return Ok(None);
        }
        Ok(Some(offset.max(0) as usize))
    }

    /// Fetch the tag for a specific wav path.
    pub fn tag_for_path(&self, path: &Path) -> Result<Option<Rating>, SourceDbError> {
        let Some(path_str) = normalize_supported_audio_path(path)? else {
            return Ok(None);
        };
        let value: Option<i64> = self
            .connection
            .query_row(
                "SELECT tag FROM wav_files WHERE path = ?1",
                rusqlite::params![path_str.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(map_sql_error)?;
        Ok(value.map(Rating::from_i64))
    }

    /// Fetch the loop marker state for a specific wav path.
    pub fn looped_for_path(&self, path: &Path) -> Result<Option<bool>, SourceDbError> {
        let Some(path_str) = normalize_supported_audio_path(path)? else {
            return Ok(None);
        };
        query_flag_for_path(self, "looped", path_str.as_str())
    }

    /// Fetch the keep-lock state for a specific wav path.
    pub fn locked_for_path(&self, path: &Path) -> Result<Option<bool>, SourceDbError> {
        let Some(path_str) = normalize_supported_audio_path(path)? else {
            return Ok(None);
        };
        query_flag_for_path(self, "locked", path_str.as_str())
    }

    /// Fetch whether a wav path is known to have a tag-derived filename.
    pub fn tag_named_for_path(&self, path: &Path) -> Result<Option<bool>, SourceDbError> {
        let Some(path_str) = normalize_supported_audio_path(path)? else {
            return Ok(None);
        };
        query_flag_for_path(self, "tag_named", path_str.as_str())
    }

    /// Fetch the canonical sound classification for a specific wav path.
    pub fn sound_type_for_path(
        &self,
        path: &Path,
    ) -> Result<Option<SampleSoundType>, SourceDbError> {
        let Some(path_str) = normalize_supported_audio_path(path)? else {
            return Ok(None);
        };
        let value: Option<String> = self
            .connection
            .query_row(
                "SELECT sound_type FROM wav_files WHERE path = ?1",
                rusqlite::params![path_str.as_str()],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(map_sql_error)?
            .flatten();
        Ok(value.as_deref().and_then(SampleSoundType::from_token))
    }

    /// Fetch the custom user tag for a specific wav path.
    pub fn user_tag_for_path(&self, path: &Path) -> Result<Option<String>, SourceDbError> {
        let Some(path_str) = normalize_supported_audio_path(path)? else {
            return Ok(None);
        };
        self.connection
            .query_row(
                "SELECT user_tag FROM wav_files WHERE path = ?1",
                rusqlite::params![path_str.as_str()],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(map_sql_error)
            .map(|value| value.flatten())
    }

    /// Fetch the last played timestamp for a specific wav path.
    pub fn last_played_at_for_path(&self, path: &Path) -> Result<Option<i64>, SourceDbError> {
        let Some(path_str) = normalize_supported_audio_path(path)? else {
            return Ok(None);
        };
        let value: Option<i64> = self
            .connection
            .query_row(
                "SELECT last_played_at FROM wav_files WHERE path = ?1",
                rusqlite::params![path_str.as_str()],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()
            .map_err(map_sql_error)?
            .flatten();
        Ok(value)
    }

    /// Fetch the last curation timestamp for a specific wav path.
    pub fn last_curated_at_for_path(&self, path: &Path) -> Result<Option<i64>, SourceDbError> {
        let Some(path_str) = normalize_supported_audio_path(path)? else {
            return Ok(None);
        };
        if !wav_file_has_column(self, "last_curated_at")? {
            return Ok(None);
        }
        let value: Option<i64> = self
            .connection
            .query_row(
                "SELECT last_curated_at FROM wav_files WHERE path = ?1",
                rusqlite::params![path_str.as_str()],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()
            .map_err(map_sql_error)?
            .flatten();
        Ok(value)
    }

    /// Fetch the fixed collection slot for a specific wav path.
    pub fn collection_for_path(
        &self,
        path: &Path,
    ) -> Result<Option<SampleCollection>, SourceDbError> {
        Ok(self.collections_for_path(path)?.into_iter().next())
    }

    /// Fetch all fixed collection slots assigned to a specific wav path.
    pub fn collections_for_path(
        &self,
        path: &Path,
    ) -> Result<Vec<SampleCollection>, SourceDbError> {
        let Some(path_str) = normalize_supported_audio_path(path)? else {
            return Ok(Vec::new());
        };
        if table_has_columns(self, "wav_file_collections", &["path", "collection"])? {
            let mut stmt = self
                .connection
                .prepare(
                    "SELECT collection
                     FROM wav_file_collections
                     WHERE path = ?1
                     ORDER BY collection ASC",
                )
                .map_err(map_sql_error)?;
            return stmt
                .query_map(rusqlite::params![path_str.as_str()], |row| {
                    row.get::<_, i64>(0)
                })
                .map_err(map_sql_error)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sql_error)
                .map(|collections| {
                    collections
                        .into_iter()
                        .filter_map(SampleCollection::from_i64)
                        .collect()
                });
        }
        if !wav_file_has_column(self, "collection")? {
            return Ok(Vec::new());
        }
        let collection = self
            .connection
            .query_row(
                "SELECT collection FROM wav_files WHERE path = ?1",
                rusqlite::params![path_str.as_str()],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()
            .map_err(map_sql_error)?
            .flatten()
            .and_then(SampleCollection::from_i64);
        Ok(collection.into_iter().collect())
    }

    /// Read a metadata value by key from the database.
    pub fn get_metadata(&self, key: &str) -> Result<Option<String>, SourceDbError> {
        let value: Option<String> = self
            .connection
            .query_row(
                "SELECT value FROM metadata WHERE key = ?1",
                rusqlite::params![key],
                |row| row.get(0),
            )
            .optional()
            .map_err(map_sql_error)?;
        Ok(value)
    }

    /// Return the numeric metadata revision (0 if missing).
    pub fn get_revision(&self) -> Result<u64, SourceDbError> {
        self.get_numeric_metadata("revision")
    }

    /// Return the numeric revision for ordered wav-path changes (0 if missing).
    pub fn get_wav_paths_revision(&self) -> Result<u64, SourceDbError> {
        self.get_numeric_metadata(META_WAV_PATHS_REVISION)
    }
}
