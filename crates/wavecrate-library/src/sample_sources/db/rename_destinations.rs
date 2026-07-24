use std::path::{Path, PathBuf};

use rusqlite::{OptionalExtension, params};

use super::util::{map_sql_error, normalize_relative_path, parse_relative_path_from_db};
use super::{SourceDatabase, SourceDbError, SourceWriteBatch};

const TARGETED_SCAN_GENERATION_KEY: &str = "targeted_scan_generation_v1";
const RENAME_CANDIDATE_LAST_SCAN_KIND_KEY: &str = "rename_candidate_last_scan_kind_v1";

#[derive(Clone, Copy)]
enum RenameCandidateScanKind {
    Targeted,
    Quick,
}

impl RenameCandidateScanKind {
    fn token(self) -> &'static str {
        match self {
            Self::Targeted => "targeted",
            Self::Quick => "quick",
        }
    }
}

impl SourceDatabase {
    /// List recent destinations plus candidates retained for unresolved content matches.
    pub fn list_pending_rename_destinations(&self) -> Result<Vec<PathBuf>, SourceDbError> {
        let generation = self
            .get_metadata(TARGETED_SCAN_GENERATION_KEY)?
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        let oldest = generation.saturating_sub(1);
        let mut statement = self
            .connection
            .prepare(
                "SELECT path
                 FROM pending_wav_rename_destinations
                 WHERE retained_hash IS NOT NULL OR scan_generation >= ?1
                 ORDER BY path ASC",
            )
            .map_err(map_sql_error)?;
        let rows = statement
            .query_map(params![oldest as i64], |row| row.get::<_, String>(0))
            .map_err(map_sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_sql_error)?;
        Ok(rows
            .into_iter()
            .filter_map(|path| match parse_relative_path_from_db(&path) {
                Ok(path) => Some(path),
                Err(error) => {
                    tracing::warn!(%error, "Skipping invalid pending rename destination path");
                    None
                }
            })
            .collect())
    }

    /// List only destinations retained because their content matches unresolved metadata.
    pub fn list_retained_rename_destinations(&self) -> Result<Vec<PathBuf>, SourceDbError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT path
                 FROM pending_wav_rename_destinations
                 WHERE retained_hash IS NOT NULL
                 ORDER BY path ASC",
            )
            .map_err(map_sql_error)?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(map_sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_sql_error)?;
        Ok(rows
            .into_iter()
            .filter_map(|path| match parse_relative_path_from_db(&path) {
                Ok(path) => Some(path),
                Err(error) => {
                    tracing::warn!(%error, "Skipping invalid retained rename destination path");
                    None
                }
            })
            .collect())
    }

    /// List up to two live retained destinations for one pending source path.
    /// Two live matches are sufficient to preserve the rename ambiguity rule;
    /// limiting the result keeps targeted recovery bounded.
    pub fn list_retained_rename_destinations_for_pending_path(
        &self,
        pending_path: &Path,
    ) -> Result<Vec<PathBuf>, SourceDbError> {
        let path = normalize_relative_path(pending_path)?;
        let mut statement = self
            .connection
            .prepare(
                "SELECT destination.path
                 FROM pending_wav_rename_destinations AS destination
                 JOIN pending_wav_renames AS pending
                   ON pending.content_hash = destination.retained_hash
                 JOIN wav_files AS live
                   ON live.path = destination.path AND live.missing = 0
                 WHERE pending.path = ?1
                   AND destination.retained_hash IS NOT NULL
                 ORDER BY destination.path ASC
                 LIMIT 2",
            )
            .map_err(map_sql_error)?;
        let rows = statement
            .query_map(params![path], |row| row.get::<_, String>(0))
            .map_err(map_sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_sql_error)?;
        Ok(rows
            .into_iter()
            .filter_map(|path| match parse_relative_path_from_db(&path) {
                Ok(path) => Some(path),
                Err(error) => {
                    tracing::warn!(%error, "Skipping invalid retained rename destination path");
                    None
                }
            })
            .collect())
    }

    /// List unretained destinations from the immediately preceding scan generation.
    /// The result contains at most `limit + 1` rows so callers can route overflow
    /// to authoritative recovery without materializing the complete carry-over set.
    pub fn list_recent_unretained_rename_destinations(
        &self,
        limit: usize,
    ) -> Result<(Vec<PathBuf>, bool), SourceDbError> {
        let generation = self
            .get_metadata(TARGETED_SCAN_GENERATION_KEY)?
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        let previous = generation.saturating_sub(1);
        let query_limit = i64::try_from(limit.saturating_add(1)).unwrap_or(i64::MAX);
        let mut statement = self
            .connection
            .prepare(
                "SELECT path
                 FROM pending_wav_rename_destinations
                 WHERE retained_hash IS NULL AND scan_generation = ?1
                 ORDER BY path ASC
                 LIMIT ?2",
            )
            .map_err(map_sql_error)?;
        let rows = statement
            .query_map(params![previous as i64, query_limit], |row| {
                row.get::<_, String>(0)
            })
            .map_err(map_sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_sql_error)?;
        let overflow = rows.len() > limit;
        Ok((
            rows.into_iter()
                .take(limit)
                .filter_map(|path| match parse_relative_path_from_db(&path) {
                    Ok(path) => Some(path),
                    Err(error) => {
                        tracing::warn!(%error, "Skipping invalid recent rename destination path");
                        None
                    }
                })
                .collect(),
            overflow,
        ))
    }
}

impl SourceWriteBatch<'_> {
    /// Begin a targeted watcher scan and expire candidates older than one prior batch.
    pub fn begin_targeted_scan_generation(&mut self) -> Result<u64, SourceDbError> {
        self.begin_rename_candidate_generation(RenameCandidateScanKind::Targeted)
    }

    /// Start a full quick scan while carrying destinations from one immediately prior scan.
    pub fn begin_quick_scan_rename_candidates(&mut self) -> Result<u64, SourceDbError> {
        self.begin_rename_candidate_generation(RenameCandidateScanKind::Quick)
    }

    fn begin_rename_candidate_generation(
        &mut self,
        kind: RenameCandidateScanKind,
    ) -> Result<u64, SourceDbError> {
        let current = self
            .tx
            .query_row(
                "SELECT value FROM metadata WHERE key = ?1",
                params![TARGETED_SCAN_GENERATION_KEY],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(map_sql_error)?
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        let last_kind = self
            .tx
            .query_row(
                "SELECT value FROM metadata WHERE key = ?1",
                params![RENAME_CANDIDATE_LAST_SCAN_KIND_KEY],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(map_sql_error)?;
        let generation = if matches!(kind, RenameCandidateScanKind::Targeted)
            && last_kind.as_deref() == Some(RenameCandidateScanKind::Quick.token())
        {
            current.max(1)
        } else {
            current.saturating_add(1)
        };
        self.set_metadata(TARGETED_SCAN_GENERATION_KEY, &generation.to_string())?;
        self.set_metadata(RENAME_CANDIDATE_LAST_SCAN_KIND_KEY, kind.token())?;
        self.tx
            .execute(
                "DELETE FROM pending_wav_rename_destinations
                 WHERE retained_hash IS NULL AND scan_generation < ?1",
                params![generation.saturating_sub(1) as i64],
            )
            .map_err(map_sql_error)?;
        Ok(generation)
    }

    /// Retain a newly discovered targeted path across the next watcher batch.
    pub fn stage_pending_rename_destination(
        &mut self,
        relative_path: &Path,
        generation: u64,
    ) -> Result<(), SourceDbError> {
        let path = normalize_relative_path(relative_path)?;
        self.tx
            .execute(
                "INSERT INTO pending_wav_rename_destinations (path, scan_generation)
                 VALUES (?1, ?2)
                 ON CONFLICT(path) DO UPDATE SET scan_generation = excluded.scan_generation",
                params![path, generation as i64],
            )
            .map_err(map_sql_error)?;
        Ok(())
    }

    /// Keep an eligible destination until the pending content match resolves or becomes invalid.
    pub fn retain_pending_rename_destination(
        &mut self,
        relative_path: &Path,
        content_hash: &str,
    ) -> Result<(), SourceDbError> {
        let path = normalize_relative_path(relative_path)?;
        self.tx
            .execute(
                "UPDATE pending_wav_rename_destinations
                 SET retained_hash = ?2
                 WHERE path = ?1",
                params![path, content_hash],
            )
            .map_err(map_sql_error)?;
        Ok(())
    }

    /// Remove a retained destination and report whether it belonged to unresolved recovery.
    pub fn clear_retained_pending_rename_destination(
        &mut self,
        relative_path: &Path,
    ) -> Result<bool, SourceDbError> {
        let path = normalize_relative_path(relative_path)?;
        self.tx
            .execute(
                "DELETE FROM pending_wav_rename_destinations
                 WHERE path = ?1 AND retained_hash IS NOT NULL",
                params![path],
            )
            .map(|removed| removed != 0)
            .map_err(map_sql_error)
    }

    /// Drop destinations that an authoritative manifest proves cannot recover pending metadata.
    pub fn prune_invalid_retained_rename_destinations(&mut self) -> Result<(), SourceDbError> {
        self.tx
            .execute(
                "DELETE FROM pending_wav_rename_destinations AS destination
                 WHERE NOT EXISTS (
                           SELECT 1
                           FROM wav_files AS present
                           WHERE present.path = destination.path
                             AND present.missing = 0
                       )
                    OR (
                        destination.retained_hash IS NULL
                        AND EXISTS (
                            SELECT 1
                            FROM wav_files AS present
                            WHERE present.path = destination.path
                              AND present.missing = 0
                              AND present.content_hash IS NOT NULL
                        )
                    )
                    OR (
                       destination.retained_hash IS NOT NULL
                       AND NOT EXISTS (
                       SELECT 1
                       FROM wav_files AS present
                       JOIN pending_wav_renames AS missing
                         ON missing.content_hash = destination.retained_hash
                       WHERE present.path = destination.path
                         AND present.missing = 0
                         AND present.content_hash = destination.retained_hash
                       )
                    )",
                [],
            )
            .map_err(map_sql_error)?;
        Ok(())
    }

    /// Remove transient destinations after an authoritative hard scan.
    pub fn clear_unretained_pending_rename_destinations(&mut self) -> Result<(), SourceDbError> {
        self.tx
            .execute(
                "DELETE FROM pending_wav_rename_destinations WHERE retained_hash IS NULL",
                [],
            )
            .map_err(map_sql_error)?;
        Ok(())
    }

    /// Forget a destination after it has been consumed by proven rename reconciliation.
    pub fn clear_pending_rename_destination(
        &mut self,
        relative_path: &Path,
    ) -> Result<(), SourceDbError> {
        let path = normalize_relative_path(relative_path)?;
        self.tx
            .execute(
                "DELETE FROM pending_wav_rename_destinations WHERE path = ?1",
                params![path],
            )
            .map_err(map_sql_error)?;
        Ok(())
    }

    /// Drop all retained destination candidates during authoritative rebuilds.
    pub fn clear_all_pending_rename_destinations(&mut self) -> Result<(), SourceDbError> {
        self.tx
            .execute("DELETE FROM pending_wav_rename_destinations", [])
            .map_err(map_sql_error)?;
        Ok(())
    }
}
