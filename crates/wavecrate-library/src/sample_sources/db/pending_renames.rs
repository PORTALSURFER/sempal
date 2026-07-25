use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{OptionalExtension, params};

use super::util::{map_sql_error, normalize_relative_path, parse_relative_path_from_db};
use super::{
    Rating, RenameMetadataSnapshot, SampleCollection, SampleSoundType, SourceDatabase,
    SourceDbError, SourceWriteBatch, WavEntry,
};

const DELETE_PENDING_RENAME_SQL: &str = "DELETE FROM pending_wav_renames WHERE path = ?1";
const PENDING_RENAME_AUTHORITATIVE_GENERATION_KEY: &str =
    "pending_rename_authoritative_generation_v1";
const PENDING_RENAME_RETENTION_GENERATIONS: u64 = 2;
const MAX_PENDING_RENAME_GENERATION: u64 = i64::MAX as u64;
const TAKE_PENDING_RENAME_BY_HASH_SQL: &str =
    "SELECT path, file_size, modified_ns, content_hash, tag, looped, sound_type, locked, last_played_at, last_curated_at, user_tag, normal_tags, collection, collections, tag_named, file_identity, staged_generation, staged_at
     FROM pending_wav_renames
     WHERE content_hash = ?1
     LIMIT 2";
const TAKE_PENDING_RENAME_BY_FILE_IDENTITY_SQL: &str =
    "SELECT path, file_size, modified_ns, content_hash, tag, looped, sound_type, locked, last_played_at, last_curated_at, user_tag, normal_tags, collection, collections, tag_named, file_identity, staged_generation, staged_at
     FROM pending_wav_renames
     WHERE file_identity = ?1 AND file_size = ?2 AND modified_ns = ?3
     LIMIT 2";
const FIND_PENDING_RENAME_BY_FILE_IDENTITY_SQL: &str =
    "SELECT path, file_size, modified_ns, content_hash, tag, looped, sound_type, locked, last_played_at, last_curated_at, user_tag, normal_tags, collection, collections, tag_named, file_identity, staged_generation, staged_at
     FROM pending_wav_renames
     WHERE file_identity = ?1
     LIMIT 2";
const TAKE_PENDING_RENAME_BY_FACTS_SQL: &str =
    "SELECT path, file_size, modified_ns, content_hash, tag, looped, sound_type, locked, last_played_at, last_curated_at, user_tag, normal_tags, collection, collections, tag_named, file_identity, staged_generation, staged_at
     FROM pending_wav_renames
     WHERE file_size = ?1 AND modified_ns = ?2
     LIMIT 2";

/// Metadata retained for a recently pruned sample row so later scans can
/// preserve user annotations when the file reappears at a new path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingRenameEntry {
    /// File path relative to the source root when the row was pruned.
    pub relative_path: PathBuf,
    /// File size at the time the row was pruned.
    pub file_size: u64,
    /// Last modified timestamp at the time the row was pruned.
    pub modified_ns: i64,
    /// Last known content hash, if one was computed before pruning.
    pub content_hash: Option<String>,
    /// Stable filesystem-object identity captured before pruning, when supported.
    pub file_identity: Option<String>,
    /// Completed authoritative generation this candidate was staged against.
    pub staged_generation: u64,
    /// Wall-clock staging time used only for diagnostics, never pruning eligibility.
    pub staged_at: Option<i64>,
    /// Complete user metadata restored when the rename is reconciled.
    pub metadata: RenameMetadataSnapshot,
}

/// Bounded aggregate diagnostics for retained rename metadata.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PendingRenameDiagnostics {
    /// Number of retained source candidates.
    pub candidate_count: usize,
    /// Latest successfully completed authoritative source enumeration.
    pub authoritative_generation: u64,
    /// Oldest candidate generation, when any candidates remain.
    pub oldest_staged_generation: Option<u64>,
    /// Oldest diagnostic staging timestamp, when recorded.
    pub oldest_staged_at: Option<i64>,
    /// Non-negative diagnostic age of the oldest timestamp at observation time.
    pub oldest_candidate_age_seconds: Option<u64>,
}

/// Result of atomically completing one authoritative retention generation.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PendingRenamePruneReport {
    /// Candidates removed by this completion.
    pub candidates_pruned: usize,
    /// Aggregate population after pruning.
    pub diagnostics: PendingRenameDiagnostics,
}

impl SourceDatabase {
    /// List pending rename candidates retained after immediate pruning.
    pub fn list_pending_renames(&self) -> Result<Vec<PendingRenameEntry>, SourceDbError> {
        let Some(query) = pending_rename_list_query(&self.connection)? else {
            return Ok(Vec::new());
        };
        let mut stmt = self.connection.prepare(&query).map_err(map_sql_error)?;
        let rows = stmt
            .query_map([], |row| {
                let stored_path: String = row.get(0)?;
                let Ok(relative_path) = parse_relative_path_from_db(&stored_path) else {
                    tracing::warn!(
                        "Skipping pending rename row with invalid relative path: {stored_path}"
                    );
                    return Ok(None);
                };
                pending_rename_from_row(row, relative_path).map(Some)
            })
            .map_err(map_sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_sql_error)?;
        Ok(rows.into_iter().flatten().collect())
    }

    /// Return whether any pending source candidate exists without loading candidate rows.
    pub fn has_pending_renames(&self) -> Result<bool, SourceDbError> {
        if super::schema::table_columns(&self.connection, "pending_wav_renames")?.is_empty() {
            return Ok(false);
        }
        self.connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pending_wav_renames LIMIT 1)",
                [],
                |row| row.get::<_, bool>(0),
            )
            .map_err(map_sql_error)
    }

    /// Read aggregate lifecycle diagnostics without materializing candidate rows.
    pub fn pending_rename_diagnostics(&self) -> Result<PendingRenameDiagnostics, SourceDbError> {
        let columns = super::schema::table_columns(&self.connection, "pending_wav_renames")?;
        if columns.is_empty() {
            return Ok(PendingRenameDiagnostics::default());
        }
        let staged_generation = if columns.contains("staged_generation") {
            "MIN(staged_generation)"
        } else {
            "CASE WHEN COUNT(*) = 0 THEN NULL ELSE 0 END"
        };
        let staged_at = if columns.contains("staged_at") {
            "MIN(staged_at)"
        } else {
            "NULL"
        };
        let (count, oldest_generation, oldest_staged_at) = self
            .connection
            .query_row(
                &format!(
                    "SELECT COUNT(*), {staged_generation}, {staged_at}
                     FROM pending_wav_renames"
                ),
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, Option<i64>>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                    ))
                },
            )
            .map_err(map_sql_error)?;
        let authoritative_generation =
            if super::schema::table_columns(&self.connection, "metadata")?.is_empty() {
                0
            } else {
                self.get_metadata(PENDING_RENAME_AUTHORITATIVE_GENERATION_KEY)?
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(0)
            };
        Ok(PendingRenameDiagnostics {
            candidate_count: usize::try_from(count).unwrap_or(usize::MAX),
            authoritative_generation,
            oldest_staged_generation: oldest_generation.and_then(|value| value.try_into().ok()),
            oldest_staged_at,
            oldest_candidate_age_seconds: pending_rename_age_seconds(oldest_staged_at),
        })
    }
}

fn pending_rename_list_query(
    connection: &rusqlite::Connection,
) -> Result<Option<String>, SourceDbError> {
    let columns = super::schema::table_columns(connection, "pending_wav_renames")?;
    if columns.is_empty() {
        return Ok(None);
    }
    let optional_column = |column: &'static str, fallback: &'static str| {
        if columns.contains(column) {
            column
        } else {
            fallback
        }
    };
    let sound_type = optional_column("sound_type", "NULL AS sound_type");
    let last_curated_at = optional_column("last_curated_at", "NULL AS last_curated_at");
    let user_tag = optional_column("user_tag", "NULL AS user_tag");
    let normal_tags = optional_column("normal_tags", "NULL AS normal_tags");
    let collection = optional_column("collection", "NULL AS collection");
    let collections = optional_column("collections", "NULL AS collections");
    let tag_named = optional_column("tag_named", "0 AS tag_named");
    let file_identity = optional_column("file_identity", "NULL AS file_identity");
    let staged_generation = optional_column("staged_generation", "0 AS staged_generation");
    let staged_at = optional_column("staged_at", "NULL AS staged_at");
    Ok(Some(format!(
        "SELECT path, file_size, modified_ns, content_hash, tag, looped, {sound_type}, locked, last_played_at, {last_curated_at}, {user_tag}, {normal_tags}, {collection}, {collections}, {tag_named}, {file_identity}, {staged_generation}, {staged_at}
         FROM pending_wav_renames
         ORDER BY path ASC"
    )))
}

impl<'conn> SourceWriteBatch<'conn> {
    /// Retain metadata for a pruned file row so later scans can recover tags on rename.
    ///
    /// Quick scans keep these rows around so a later deep-hash pass or follow-up
    /// quick scan can reconcile path changes without losing user metadata.
    pub fn stage_pending_rename(&mut self, entry: &WavEntry) -> Result<(), SourceDbError> {
        let path = normalize_relative_path(&entry.relative_path)?;
        let staged_generation = self.pending_rename_staging_generation()?;
        let staged_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .min(i64::MAX as u64) as i64;
        let metadata = self.snapshot_rename_metadata(&entry.relative_path)?;
        let normal_tags = encode_normal_tags(&metadata.normal_tags)?;
        let collections = encode_collections(&metadata.collections)?;
        let legacy_collection = metadata.collections.first().copied();
        let file_identity = self
            .tx
            .query_row(
                "SELECT file_identity FROM wav_files WHERE path = ?1",
                params![path.as_str()],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(map_sql_error)?
            .flatten();
        self.tx
            .prepare_cached(
                "INSERT INTO pending_wav_renames (
                     path, file_size, modified_ns, content_hash, tag, looped, sound_type, locked, last_played_at, last_curated_at, user_tag, normal_tags, collection, collections, tag_named, file_identity, staged_generation, staged_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
                 ON CONFLICT(path) DO UPDATE SET
                     file_size = excluded.file_size,
                     modified_ns = excluded.modified_ns,
                     content_hash = excluded.content_hash,
                     tag = excluded.tag,
                     looped = excluded.looped,
                     sound_type = excluded.sound_type,
                     locked = excluded.locked,
                     last_played_at = excluded.last_played_at,
                     last_curated_at = excluded.last_curated_at,
                     user_tag = excluded.user_tag,
                     normal_tags = excluded.normal_tags,
                     collection = excluded.collection,
                     collections = excluded.collections,
                     tag_named = excluded.tag_named,
                     file_identity = excluded.file_identity,
                     staged_generation = excluded.staged_generation,
                     staged_at = excluded.staged_at",
            )
            .map_err(map_sql_error)?
            .execute(params![
                path,
                entry.file_size as i64,
                entry.modified_ns,
                entry.content_hash.as_deref(),
                metadata.tag.as_i64(),
                metadata.looped as i64,
                metadata.sound_type.map(SampleSoundType::token),
                metadata.locked as i64,
                metadata.last_played_at,
                metadata.last_curated_at,
                metadata.user_tag.as_deref(),
                normal_tags.as_deref(),
                legacy_collection.map(SampleCollection::as_i64),
                collections.as_deref(),
                metadata.tag_named as i64,
                file_identity,
                staged_generation as i64,
                staged_at,
            ])
            .map_err(map_sql_error)?;
        Ok(())
    }

    fn pending_rename_staging_generation(&self) -> Result<u64, SourceDbError> {
        self.read_pending_rename_authoritative_generation()
            .map(next_pending_rename_generation)
    }

    fn read_pending_rename_authoritative_generation(&self) -> Result<u64, SourceDbError> {
        self.tx
            .query_row(
                "SELECT value FROM metadata WHERE key = ?1",
                params![PENDING_RENAME_AUTHORITATIVE_GENERATION_KEY],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(map_sql_error)
            .map(|value| value.and_then(|value| value.parse().ok()).unwrap_or(0))
    }

    /// Atomically advance authoritative retention state and prune safely expired candidates.
    ///
    /// Quick scans keep candidates through two later complete full-source enumerations. A hard
    /// scan may additionally drop unmatched candidates immediately because it has enumerated the
    /// complete source and already attempted current-destination reconciliation. Any unresolved
    /// destination defers quick-scan pruning so a crash between enumeration and deferred hashing
    /// cannot discard the only copy of user metadata.
    pub fn complete_pending_rename_authoritative_scan(
        &mut self,
        hard_scan: bool,
    ) -> Result<PendingRenamePruneReport, SourceDbError> {
        let generation =
            next_pending_rename_generation(self.read_pending_rename_authoritative_generation()?);
        let pruned = if hard_scan {
            self.tx
                .execute(
                    "DELETE FROM pending_wav_renames AS pending
                     WHERE (
                         pending.content_hash IS NULL
                         OR NOT EXISTS (
                             SELECT 1
                             FROM pending_wav_rename_destinations AS destination
                             WHERE destination.retained_hash = pending.content_hash
                         )
                     )
                       AND NOT EXISTS (
                           SELECT 1
                           FROM pending_wav_rename_destinations AS active
                           WHERE active.retained_hash IS NULL
                       )",
                    [],
                )
                .map_err(map_sql_error)?
        } else {
            let oldest_retained = generation.saturating_sub(PENDING_RENAME_RETENTION_GENERATIONS);
            self.tx
                .execute(
                    "DELETE FROM pending_wav_renames AS pending
                     WHERE ?2 > ?3
                       AND pending.staged_generation <= ?1
                       AND NOT EXISTS (
                           SELECT 1
                           FROM pending_wav_rename_destinations AS destination
                           WHERE destination.retained_hash = pending.content_hash
                       )
                       AND NOT EXISTS (
                           SELECT 1
                           FROM pending_wav_rename_destinations AS active
                           WHERE active.retained_hash IS NULL
                       )",
                    params![
                        oldest_retained as i64,
                        generation as i64,
                        PENDING_RENAME_RETENTION_GENERATIONS as i64,
                    ],
                )
                .map_err(map_sql_error)?
        };
        self.set_metadata(
            PENDING_RENAME_AUTHORITATIVE_GENERATION_KEY,
            &generation.to_string(),
        )?;
        let (count, oldest_generation, oldest_staged_at) = self
            .tx
            .query_row(
                "SELECT COUNT(*), MIN(staged_generation), MIN(staged_at)
                 FROM pending_wav_renames",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, Option<i64>>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                    ))
                },
            )
            .map_err(map_sql_error)?;
        Ok(PendingRenamePruneReport {
            candidates_pruned: pruned,
            diagnostics: PendingRenameDiagnostics {
                candidate_count: usize::try_from(count).unwrap_or(usize::MAX),
                authoritative_generation: generation,
                oldest_staged_generation: oldest_generation.and_then(|value| value.try_into().ok()),
                oldest_staged_at,
                oldest_candidate_age_seconds: pending_rename_age_seconds(oldest_staged_at),
            },
        })
    }

    /// Remove one retained rename candidate by its original relative path.
    pub fn clear_pending_rename(&mut self, relative_path: &Path) -> Result<(), SourceDbError> {
        let path = normalize_relative_path(relative_path)?;
        self.tx
            .prepare_cached(DELETE_PENDING_RENAME_SQL)
            .map_err(map_sql_error)?
            .execute(params![path])
            .map_err(map_sql_error)?;
        Ok(())
    }

    /// Remove any retained rename candidate that would conflict with a live path upsert.
    pub(crate) fn clear_pending_rename_for_live_path(
        &mut self,
        relative_path: &Path,
    ) -> Result<(), SourceDbError> {
        self.clear_pending_rename(relative_path)
    }

    /// Drop every retained rename candidate.
    ///
    /// Hard rescans use this to treat the current disk walk as authoritative and
    /// prune any unmatched quick-scan rename rows that are still hanging around.
    pub fn clear_all_pending_renames(&mut self) -> Result<(), SourceDbError> {
        self.tx
            .execute("DELETE FROM pending_wav_renames", [])
            .map_err(map_sql_error)?;
        Ok(())
    }

    /// Claim one unique retained rename candidate by content hash.
    pub fn take_pending_rename_by_hash(
        &mut self,
        hash: &str,
    ) -> Result<Option<PendingRenameEntry>, SourceDbError> {
        let entry = self.unique_pending_rename_by_hash(hash)?;
        if let Some(entry) = entry.as_ref() {
            self.clear_pending_rename(&entry.relative_path)?;
        }
        Ok(entry)
    }

    /// Find one globally unique retained candidate by content hash without consuming it.
    pub fn unique_pending_rename_by_hash(
        &mut self,
        hash: &str,
    ) -> Result<Option<PendingRenameEntry>, SourceDbError> {
        self.find_unique_pending_rename(TAKE_PENDING_RENAME_BY_HASH_SQL, params![hash])
    }

    /// Claim one unique retained rename candidate by stable filesystem identity.
    pub fn take_pending_rename_by_file_identity(
        &mut self,
        file_identity: &str,
        file_size: u64,
        modified_ns: i64,
    ) -> Result<Option<PendingRenameEntry>, SourceDbError> {
        let entry =
            self.unique_pending_rename_by_file_identity(file_identity, file_size, modified_ns)?;
        if let Some(entry) = entry.as_ref() {
            self.clear_pending_rename(&entry.relative_path)?;
        }
        Ok(entry)
    }

    /// Find one unique retained candidate by stable filesystem identity without consuming it.
    pub fn unique_pending_rename_by_file_identity(
        &mut self,
        file_identity: &str,
        file_size: u64,
        modified_ns: i64,
    ) -> Result<Option<PendingRenameEntry>, SourceDbError> {
        self.find_unique_pending_rename(
            TAKE_PENDING_RENAME_BY_FILE_IDENTITY_SQL,
            params![file_identity, file_size as i64, modified_ns],
        )
    }

    /// Find one globally unique candidate by stable filesystem identity without consuming it.
    pub fn unique_pending_rename_by_file_identity_only(
        &mut self,
        file_identity: &str,
    ) -> Result<Option<PendingRenameEntry>, SourceDbError> {
        self.find_unique_pending_rename(
            FIND_PENDING_RENAME_BY_FILE_IDENTITY_SQL,
            params![file_identity],
        )
    }

    /// Claim one unique retained rename candidate by file facts.
    pub fn take_pending_rename_by_facts(
        &mut self,
        file_size: u64,
        modified_ns: i64,
    ) -> Result<Option<PendingRenameEntry>, SourceDbError> {
        let entry = self.find_unique_pending_rename(
            TAKE_PENDING_RENAME_BY_FACTS_SQL,
            params![file_size as i64, modified_ns],
        )?;
        if let Some(entry) = entry.as_ref() {
            self.clear_pending_rename(&entry.relative_path)?;
        }
        Ok(entry)
    }

    fn find_unique_pending_rename(
        &mut self,
        sql: &str,
        params: impl rusqlite::Params,
    ) -> Result<Option<PendingRenameEntry>, SourceDbError> {
        let mut stmt = self.tx.prepare_cached(sql).map_err(map_sql_error)?;
        let rows = stmt
            .query_map(params, |row| {
                let stored_path: String = row.get(0)?;
                let relative_path = parse_relative_path_from_db(&stored_path).map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(err),
                    )
                })?;
                pending_rename_from_row(row, relative_path)
            })
            .map_err(map_sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_sql_error)?;
        drop(stmt);
        if rows.len() != 1 {
            return Ok(None);
        }
        Ok(rows.into_iter().next())
    }
}

fn pending_rename_from_row(
    row: &rusqlite::Row<'_>,
    relative_path: PathBuf,
) -> rusqlite::Result<PendingRenameEntry> {
    let file_size: i64 = row.get(1)?;
    let file_size = u64::try_from(file_size).map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            1,
            rusqlite::types::Type::Integer,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "pending rename file_size out of range",
            )),
        )
    })?;
    let staged_generation = row.get::<_, i64>(16)?;
    let staged_generation = u64::try_from(staged_generation).map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            16,
            rusqlite::types::Type::Integer,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "pending rename staged_generation out of range",
            )),
        )
    })?;
    Ok(PendingRenameEntry {
        relative_path,
        file_size,
        modified_ns: row.get(2)?,
        content_hash: row.get(3)?,
        file_identity: row.get(15)?,
        staged_generation,
        staged_at: row.get(17)?,
        metadata: metadata_from_row(row)?,
    })
}

fn next_pending_rename_generation(current: u64) -> u64 {
    current.saturating_add(1).min(MAX_PENDING_RENAME_GENERATION)
}

fn pending_rename_age_seconds(staged_at: Option<i64>) -> Option<u64> {
    let staged_at = staged_at?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .min(i64::MAX as u64) as i64;
    Some(now.saturating_sub(staged_at).max(0) as u64)
}

fn metadata_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RenameMetadataSnapshot> {
    Ok(RenameMetadataSnapshot {
        tag: Rating::from_i64(row.get::<_, i64>(4)?),
        looped: row.get::<_, i64>(5)? != 0,
        sound_type: row
            .get::<_, Option<String>>(6)?
            .as_deref()
            .and_then(SampleSoundType::from_token),
        locked: row.get::<_, i64>(7)? != 0,
        last_played_at: row.get(8)?,
        last_curated_at: row.get(9)?,
        user_tag: row.get(10)?,
        normal_tags: decode_normal_tags(row.get(11)?),
        collections: decode_collections(row.get(13)?, row.get(12)?),
        tag_named: row.get::<_, i64>(14)? != 0,
    })
}

fn encode_normal_tags(labels: &[String]) -> Result<Option<String>, SourceDbError> {
    if labels.is_empty() {
        return Ok(None);
    }
    serde_json::to_string(labels)
        .map(Some)
        .map_err(|_| SourceDbError::Unexpected)
}

fn decode_normal_tags(value: Option<String>) -> Vec<String> {
    value
        .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
        .unwrap_or_default()
}

fn encode_collections(collections: &[SampleCollection]) -> Result<Option<String>, SourceDbError> {
    let values = collections
        .iter()
        .copied()
        .map(SampleCollection::as_i64)
        .collect::<Vec<_>>();
    serde_json::to_string(&values)
        .map(Some)
        .map_err(|_| SourceDbError::Unexpected)
}

fn decode_collections(
    value: Option<String>,
    legacy_collection: Option<i64>,
) -> Vec<SampleCollection> {
    let Some(values) = value.and_then(|raw| serde_json::from_str::<Vec<i64>>(&raw).ok()) else {
        return legacy_collection
            .and_then(SampleCollection::from_i64)
            .into_iter()
            .collect();
    };
    let mut collections = values
        .into_iter()
        .filter_map(SampleCollection::from_i64)
        .collect::<Vec<_>>();
    collections.sort_by_key(|collection| collection.index());
    collections.dedup();
    collections
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn pending_reconciliation_lookups_use_bounded_indices() {
        let directory = tempdir().unwrap();
        let database =
            SourceDatabase::open_for_test_fixture_source_write(directory.path()).unwrap();
        for (sql, parameters, expected_index) in [
            (
                TAKE_PENDING_RENAME_BY_HASH_SQL,
                vec![rusqlite::types::Value::Text(String::from("hash"))],
                "idx_pending_wav_renames_hash",
            ),
            (
                TAKE_PENDING_RENAME_BY_FILE_IDENTITY_SQL,
                vec![
                    rusqlite::types::Value::Text(String::from("unix:1:2:3")),
                    rusqlite::types::Value::Integer(1),
                    rusqlite::types::Value::Integer(2),
                ],
                "idx_pending_wav_renames_identity_facts",
            ),
            (
                TAKE_PENDING_RENAME_BY_FACTS_SQL,
                vec![
                    rusqlite::types::Value::Integer(1),
                    rusqlite::types::Value::Integer(2),
                ],
                "idx_pending_wav_renames_facts",
            ),
        ] {
            let mut statement = database
                .connection
                .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                .unwrap();
            let details = statement
                .query_map(rusqlite::params_from_iter(parameters), |row| {
                    row.get::<_, String>(3)
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
                .join("\n");
            assert!(
                details.contains(expected_index),
                "expected {expected_index} in query plan:\n{details}"
            );
            assert!(!details.contains("SCAN pending_wav_renames"));
        }
    }

    #[test]
    fn unresolved_destination_defers_quick_and_hard_pruning() {
        let directory = tempdir().unwrap();
        std::fs::write(directory.path().join("old.wav"), b"same").unwrap();
        let database =
            SourceDatabase::open_for_test_fixture_source_write(directory.path()).unwrap();
        let mut batch = database.write_batch().unwrap();
        batch
            .upsert_file_with_hash(Path::new("old.wav"), 4, 1, "same-hash")
            .unwrap();
        batch.commit().unwrap();
        let entry = database
            .entry_for_path(Path::new("old.wav"))
            .unwrap()
            .unwrap();
        let mut batch = database.write_batch().unwrap();
        batch.stage_pending_rename(&entry).unwrap();
        batch.remove_file(Path::new("old.wav")).unwrap();
        batch
            .stage_pending_rename_destination(Path::new("new.wav"), 1)
            .unwrap();
        batch.commit().unwrap();

        for _ in 0..4 {
            let mut batch = database.write_batch().unwrap();
            let report = batch
                .complete_pending_rename_authoritative_scan(false)
                .unwrap();
            assert_eq!(report.candidates_pruned, 0);
            batch.commit().unwrap();
        }
        assert_eq!(
            database
                .pending_rename_diagnostics()
                .unwrap()
                .candidate_count,
            1
        );

        let mut batch = database.write_batch().unwrap();
        let report = batch
            .complete_pending_rename_authoritative_scan(true)
            .unwrap();
        assert_eq!(report.candidates_pruned, 0);
        batch.commit_auxiliary_state().unwrap();
        assert_eq!(
            database
                .pending_rename_diagnostics()
                .unwrap()
                .candidate_count,
            1,
            "hard completion must retain metadata while a destination hash is unresolved"
        );
    }

    #[test]
    fn interrupted_pruning_rolls_back_candidate_and_generation() {
        let directory = tempdir().unwrap();
        let database =
            SourceDatabase::open_for_test_fixture_source_write(directory.path()).unwrap();
        let mut batch = database.write_batch().unwrap();
        batch
            .upsert_file_with_hash(Path::new("old.wav"), 4, 1, "same-hash")
            .unwrap();
        batch.commit().unwrap();
        let entry = database
            .entry_for_path(Path::new("old.wav"))
            .unwrap()
            .unwrap();
        let mut batch = database.write_batch().unwrap();
        batch.stage_pending_rename(&entry).unwrap();
        batch.remove_file(Path::new("old.wav")).unwrap();
        batch.commit().unwrap();

        for _ in 0..2 {
            let mut batch = database.write_batch().unwrap();
            batch
                .complete_pending_rename_authoritative_scan(false)
                .unwrap();
            batch.commit_auxiliary_state().unwrap();
        }
        let mut interrupted = database.write_batch().unwrap();
        let report = interrupted
            .complete_pending_rename_authoritative_scan(false)
            .unwrap();
        assert_eq!(report.candidates_pruned, 1);
        drop(interrupted);

        let diagnostics = database.pending_rename_diagnostics().unwrap();
        assert_eq!(diagnostics.authoritative_generation, 2);
        assert_eq!(diagnostics.candidate_count, 1);
    }
}
