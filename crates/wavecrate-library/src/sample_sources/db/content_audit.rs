use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};

use super::util::{map_sql_error, normalize_relative_path, parse_relative_path_from_db};
use super::{SourceDatabase, SourceDbError, SourceManifestEntry, SourceWriteBatch};

const LEGACY_CURSOR_KEY: &str = "source_content_audit_cursor_v1";

/// Durable checkpoint for one complete content-verification rotation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentAuditCheckpoint {
    /// Monotonic identifier for the active rotation.
    pub rotation_id: i64,
    /// Epoch second when the active rotation began.
    pub rotation_started_at: i64,
    /// Last forward-progress path committed by the rotation.
    pub cursor: String,
    /// Last retry path attempted, used for durable round-robin retry fairness.
    pub retry_cursor: String,
    /// Whether the next mixed-work slice should begin with a due retry.
    pub retry_next: bool,
    /// Source manifest revision observed by the latest checkpoint.
    pub checkpoint_revision: u64,
}

/// Durable outcome for one manifest entry in the current or a previous rotation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentAuditEntryState {
    /// Rotation in which the current snapshot was verified.
    pub verified_rotation: Option<i64>,
    /// Epoch second of the successful verification.
    pub verified_at: Option<i64>,
    /// Size of the successfully verified snapshot.
    pub verified_file_size: Option<u64>,
    /// Modification timestamp of the successfully verified snapshot.
    pub verified_modified_ns: Option<i64>,
    /// Filesystem identity of the successfully verified snapshot.
    pub verified_file_identity: Option<String>,
    /// Epoch second of the latest attempt.
    pub last_attempt_at: Option<i64>,
    /// Earliest epoch second for retrying a skipped entry.
    pub retry_at: Option<i64>,
    /// Stable reason token when the entry remains due.
    pub skip_reason: Option<String>,
    /// Total durable attempt count for the path.
    pub attempts: u32,
    /// Total bytes read while attempting this path.
    pub bytes_read: u64,
}

/// One path in a bounded forward cursor window plus its current audit disposition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentAuditForwardCandidate {
    /// Current live manifest entry.
    pub entry: SourceManifestEntry,
    /// Whether the current snapshot still requires a forward verification attempt.
    pub needs_verification: bool,
    /// Whether independently scheduled retry state owns the next attempt for this path.
    pub retry_pending: bool,
}

/// One row in the bounded due-retry frontier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentAuditRetryCandidate {
    /// Durable path used for retry rotation and stale-state cleanup.
    pub relative_path: PathBuf,
    /// Current live supported manifest entry, or `None` when the retry state is stale.
    pub entry: Option<SourceManifestEntry>,
}

impl ContentAuditEntryState {
    /// Return whether this state verifies the entry's current snapshot in `rotation_id`.
    pub fn verifies(&self, entry: &SourceManifestEntry, rotation_id: i64) -> bool {
        self.verified_rotation == Some(rotation_id)
            && self.verified_file_size == Some(entry.file_size)
            && self.verified_modified_ns == Some(entry.modified_ns)
            && self.verified_file_identity == entry.file_identity
            && self.skip_reason.is_none()
    }

    /// Return whether a skipped entry is eligible for bounded retry.
    pub fn retry_is_due(&self, now: i64) -> bool {
        self.skip_reason.is_some() && self.retry_at.is_none_or(|retry_at| retry_at <= now)
    }
}

/// Stable reason tokens for entries that remain due after a bounded attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentAuditSkipReason {
    /// The path disappeared or could not be opened.
    Unavailable,
    /// The path no longer classifies as supported regular audio.
    Unsupported,
    /// File facts changed between the pre-hash and post-hash snapshots.
    ChangedDuringHash,
    /// Hashing failed for a reason other than cancellation.
    HashFailed,
}

impl ContentAuditSkipReason {
    /// Return the stable database and telemetry token.
    pub const fn token(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::Unsupported => "unsupported",
            Self::ChangedDuringHash => "changed_during_hash",
            Self::HashFailed => "hash_failed",
        }
    }
}

/// Measurable per-source content-verification coverage.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContentAuditReport {
    /// Active durable rotation identifier.
    pub rotation_id: i64,
    /// Epoch second when the active rotation began.
    pub rotation_started_at: i64,
    /// Source manifest revision observed by the latest checkpoint.
    pub checkpoint_revision: u64,
    /// Last committed forward-progress path.
    pub cursor: String,
    /// Last committed retry path.
    pub retry_cursor: String,
    /// Whether the next mixed-work slice begins with retry work.
    pub retry_next: bool,
    /// Current supported manifest entry count.
    pub total_entries: usize,
    /// Current supported manifest byte count.
    pub total_bytes: u64,
    /// Entries verified in the active rotation.
    pub verified_entries: usize,
    /// Current bytes represented by verified entries.
    pub verified_bytes: u64,
    /// Entries still due in the active rotation.
    pub remaining_entries: usize,
    /// Current bytes represented by entries still due.
    pub remaining_bytes: u64,
    /// Due entries carrying a visible retry reason.
    pub skipped_retry_entries: usize,
    /// Bytes read by committed attempts in the active rotation.
    pub bytes_read: u64,
    /// Age in seconds of the oldest still-unverified rotation scope.
    pub oldest_unverified_age_seconds: Option<i64>,
    /// Projected full-rotation time at measured progress.
    pub estimated_rotation_seconds: Option<i64>,
    /// Epoch second when the previous complete rotation finished.
    pub last_rotation_completed_at: Option<i64>,
    /// Measured duration of the previous complete rotation.
    pub last_rotation_seconds: Option<i64>,
}

impl SourceDatabase {
    /// Begin a content rotation or resume its durable cursor. The legacy v1 cursor is imported
    /// once so upgrades preserve forward progress without treating it as verification evidence.
    pub fn begin_or_resume_content_audit(
        &self,
        now: i64,
        manifest_revision: u64,
    ) -> Result<ContentAuditCheckpoint, SourceDbError> {
        let transaction =
            Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)
                .map_err(map_sql_error)?;
        let legacy_cursor = transaction
            .query_row(
                "SELECT value FROM metadata WHERE key = ?1",
                [LEGACY_CURSOR_KEY],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(map_sql_error)?
            .unwrap_or_default();
        transaction
            .execute(
                "INSERT OR IGNORE INTO source_content_audit_state (
                    singleton, rotation_id, rotation_started_at, cursor, checkpoint_revision
                 ) VALUES (1, 1, ?1, ?2, ?3)",
                params![now, legacy_cursor, manifest_revision],
            )
            .map_err(map_sql_error)?;
        transaction
            .execute(
                "UPDATE source_content_audit_state
                 SET checkpoint_revision = ?1
                 WHERE singleton = 1",
                [manifest_revision],
            )
            .map_err(map_sql_error)?;
        transaction
            .execute("DELETE FROM metadata WHERE key = ?1", [LEGACY_CURSOR_KEY])
            .map_err(map_sql_error)?;
        let checkpoint = transaction
            .query_row(
                "SELECT rotation_id, rotation_started_at, cursor, retry_cursor, retry_next,
                        checkpoint_revision
                 FROM source_content_audit_state
                 WHERE singleton = 1",
                [],
                |row| {
                    Ok(ContentAuditCheckpoint {
                        rotation_id: row.get(0)?,
                        rotation_started_at: row.get(1)?,
                        cursor: row.get(2)?,
                        retry_cursor: row.get(3)?,
                        retry_next: row.get::<_, i64>(4)? != 0,
                        checkpoint_revision: row.get::<_, i64>(5)?.max(0) as u64,
                    })
                },
            )
            .map_err(map_sql_error)?;
        transaction.commit().map_err(map_sql_error)?;
        Ok(checkpoint)
    }

    /// Read durable per-entry content-verification outcomes keyed by relative path.
    pub fn content_audit_entry_states(
        &self,
    ) -> Result<BTreeMap<PathBuf, ContentAuditEntryState>, SourceDbError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT path, verified_rotation, verified_at, verified_file_size,
                        verified_modified_ns, verified_file_identity, last_attempt_at, retry_at,
                        skip_reason, attempts, bytes_read
                 FROM source_content_audit_entries
                 ORDER BY path",
            )
            .map_err(map_sql_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    ContentAuditEntryState {
                        verified_rotation: row.get(1)?,
                        verified_at: row.get(2)?,
                        verified_file_size: row
                            .get::<_, Option<i64>>(3)?
                            .map(|value| value.max(0) as u64),
                        verified_modified_ns: row.get(4)?,
                        verified_file_identity: row.get(5)?,
                        last_attempt_at: row.get(6)?,
                        retry_at: row.get(7)?,
                        skip_reason: row.get(8)?,
                        attempts: row.get::<_, i64>(9)?.max(0) as u32,
                        bytes_read: row.get::<_, i64>(10)?.max(0) as u64,
                    },
                ))
            })
            .map_err(map_sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_sql_error)?;
        Ok(rows
            .into_iter()
            .filter_map(|(path, state)| {
                parse_relative_path_from_db(&path)
                    .ok()
                    .map(|path| (path, state))
            })
            .collect())
    }

    /// Select a bounded forward window after the durable cursor, wrapping once at the end.
    pub fn content_audit_forward_candidates(
        &self,
        rotation_id: i64,
        cursor: &str,
        limit: usize,
    ) -> Result<Vec<ContentAuditForwardCandidate>, SourceDbError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut candidates = self.content_audit_forward_window(rotation_id, cursor, limit, true)?;
        if candidates.len() < limit {
            let remaining = limit - candidates.len();
            candidates.extend(self.content_audit_forward_window(
                rotation_id,
                cursor,
                remaining,
                false,
            )?);
        }
        Ok(candidates)
    }

    /// Select a bounded due-retry frontier and rotate it after the durable retry cursor.
    pub fn content_audit_retry_candidates(
        &self,
        retry_cursor: &str,
        now: i64,
        limit: usize,
    ) -> Result<Vec<ContentAuditRetryCandidate>, SourceDbError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let pool_limit = limit.saturating_mul(2);
        let sql = "SELECT wav.path, wav.file_identity, wav.content_hash,
                    wav.file_size, wav.modified_ns, wav.extension, wav.missing
             FROM source_content_audit_entries AS audit
                  INDEXED BY idx_source_content_audit_retry_due
             LEFT JOIN wav_files AS wav ON wav.path = audit.path
             WHERE audit.skip_reason IS NOT NULL
               AND COALESCE(audit.retry_at, 0) <= ?1
             ORDER BY COALESCE(audit.retry_at, 0) ASC, audit.path ASC
             LIMIT ?2"
            .to_string();
        let mut statement = self.connection.prepare(&sql).map_err(map_sql_error)?;
        let rows = statement
            .query_map(
                params![now, i64::try_from(pool_limit).unwrap_or(i64::MAX)],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                        row.get::<_, Option<i64>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, Option<i64>>(6)?,
                    ))
                },
            )
            .map_err(map_sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_sql_error)?;
        let mut candidates = rows
            .into_iter()
            .filter_map(
                |(
                    path,
                    file_identity,
                    content_hash,
                    file_size,
                    modified_ns,
                    extension,
                    missing,
                )| {
                    let relative_path = parse_relative_path_from_db(&path).ok()?;
                    let entry = (missing == Some(0)
                        && extension
                            .as_deref()
                            .is_some_and(|extension| extension.eq_ignore_ascii_case("wav"))
                        && crate::sample_sources::is_supported_audio(&relative_path))
                    .then(|| SourceManifestEntry {
                        relative_path: relative_path.clone(),
                        file_identity,
                        content_hash,
                        file_size: file_size.unwrap_or_default().max(0) as u64,
                        modified_ns: modified_ns.unwrap_or_default(),
                    });
                    Some(ContentAuditRetryCandidate {
                        relative_path,
                        entry,
                    })
                },
            )
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        let start = candidates
            .iter()
            .position(|entry| entry.relative_path.to_string_lossy().as_ref() > retry_cursor)
            .unwrap_or(0);
        candidates.rotate_left(start);
        candidates.truncate(limit);
        Ok(candidates)
    }

    fn content_audit_forward_window(
        &self,
        rotation_id: i64,
        cursor: &str,
        limit: usize,
        after_cursor: bool,
    ) -> Result<Vec<ContentAuditForwardCandidate>, SourceDbError> {
        let comparison = if after_cursor { ">" } else { "<=" };
        let filter = qualified_supported_audio_filter("wav");
        let sql = format!(
            "SELECT wav.path, wav.file_identity, wav.content_hash,
                    wav.file_size, wav.modified_ns,
                    CASE WHEN audit.verified_rotation IS ?2
                              AND audit.verified_file_size IS wav.file_size
                              AND audit.verified_modified_ns IS wav.modified_ns
                              AND audit.verified_file_identity IS wav.file_identity
                              AND audit.skip_reason IS NULL
                         THEN 1 ELSE 0 END AS verified,
                    CASE WHEN audit.skip_reason IS NOT NULL THEN 1 ELSE 0 END AS retry_pending
             FROM wav_files AS wav
                  INDEXED BY idx_source_content_audit_forward_path
             LEFT JOIN source_content_audit_entries AS audit
                    ON audit.path = wav.path
             WHERE wav.path {comparison} ?1
               AND {filter}
               AND wav.missing = 0
             ORDER BY wav.path ASC
             LIMIT ?3"
        );
        let mut statement = self.connection.prepare(&sql).map_err(map_sql_error)?;
        let rows = statement
            .query_map(
                params![
                    cursor,
                    rotation_id,
                    i64::try_from(limit).unwrap_or(i64::MAX)
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get::<_, i64>(3)?.max(0) as u64,
                        row.get(4)?,
                        row.get::<_, i64>(5)? != 0,
                        row.get::<_, i64>(6)? != 0,
                    ))
                },
            )
            .map_err(map_sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_sql_error)?;
        Ok(rows
            .into_iter()
            .filter_map(
                |(
                    path,
                    file_identity,
                    content_hash,
                    file_size,
                    modified_ns,
                    verified,
                    retry_pending,
                )| {
                    parse_relative_path_from_db(&path)
                        .ok()
                        .map(|relative_path| ContentAuditForwardCandidate {
                            entry: SourceManifestEntry {
                                relative_path,
                                file_identity,
                                content_hash,
                                file_size,
                                modified_ns,
                            },
                            needs_verification: !verified && !retry_pending,
                            retry_pending,
                        })
                },
            )
            .collect())
    }

    /// Measure content-verification coverage against the current manifest.
    pub fn content_audit_report(&self, now: i64) -> Result<ContentAuditReport, SourceDbError> {
        let revision = self.get_revision()?;
        let checkpoint = self.begin_or_resume_content_audit(now, revision)?;
        let filter = qualified_supported_audio_filter("wav");
        let coverage = self
            .connection
            .query_row(
                &format!(
                    "WITH coverage AS (
                         SELECT wav.file_size,
                                audit.skip_reason,
                                CASE WHEN audit.verified_rotation IS ?1
                                          AND audit.verified_file_size IS wav.file_size
                                          AND audit.verified_modified_ns IS wav.modified_ns
                                          AND audit.verified_file_identity IS wav.file_identity
                                          AND audit.skip_reason IS NULL
                                     THEN 1 ELSE 0 END AS verified
                         FROM wav_files AS wav
                         LEFT JOIN source_content_audit_entries AS audit
                                ON audit.path = wav.path
                         WHERE {filter} AND wav.missing = 0
                     )
                     SELECT COUNT(*),
                            COALESCE(SUM(file_size), 0),
                            COALESCE(SUM(verified), 0),
                            COALESCE(SUM(CASE WHEN verified = 1 THEN file_size ELSE 0 END), 0),
                            COALESCE(SUM(CASE WHEN verified = 0 THEN 1 ELSE 0 END), 0),
                            COALESCE(SUM(CASE WHEN verified = 0 THEN file_size ELSE 0 END), 0),
                            COALESCE(SUM(
                                CASE WHEN verified = 0 AND skip_reason IS NOT NULL
                                     THEN 1 ELSE 0 END
                            ), 0)
                     FROM coverage"
                ),
                [checkpoint.rotation_id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?.max(0) as usize,
                        row.get::<_, i64>(1)?.max(0) as u64,
                        row.get::<_, i64>(2)?.max(0) as usize,
                        row.get::<_, i64>(3)?.max(0) as u64,
                        row.get::<_, i64>(4)?.max(0) as usize,
                        row.get::<_, i64>(5)?.max(0) as u64,
                        row.get::<_, i64>(6)?.max(0) as usize,
                    ))
                },
            )
            .map_err(map_sql_error)?;
        let mut report = ContentAuditReport {
            rotation_id: checkpoint.rotation_id,
            rotation_started_at: checkpoint.rotation_started_at,
            checkpoint_revision: checkpoint.checkpoint_revision,
            cursor: checkpoint.cursor,
            retry_cursor: checkpoint.retry_cursor,
            retry_next: checkpoint.retry_next,
            total_entries: coverage.0,
            total_bytes: coverage.1,
            verified_entries: coverage.2,
            verified_bytes: coverage.3,
            remaining_entries: coverage.4,
            remaining_bytes: coverage.5,
            skipped_retry_entries: coverage.6,
            ..ContentAuditReport::default()
        };
        let persisted = self
            .connection
            .query_row(
                "SELECT bytes_read, last_rotation_completed_at, last_rotation_seconds
                 FROM source_content_audit_state WHERE singleton = 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?.max(0) as u64,
                        row.get::<_, Option<i64>>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                    ))
                },
            )
            .map_err(map_sql_error)?;
        report.bytes_read = persisted.0;
        report.last_rotation_completed_at = persisted.1;
        report.last_rotation_seconds = persisted.2;
        if report.remaining_entries > 0 {
            report.oldest_unverified_age_seconds =
                Some(now.saturating_sub(checkpoint.rotation_started_at).max(0));
        }
        let elapsed = now.saturating_sub(checkpoint.rotation_started_at).max(1);
        if report.verified_bytes > 0 && report.total_bytes > 0 {
            let scaled = (elapsed as i128)
                .saturating_mul(report.total_bytes as i128)
                .saturating_div(report.verified_bytes as i128);
            report.estimated_rotation_seconds = Some(scaled.min(i64::MAX as i128) as i64);
        } else if report.verified_entries > 0 && report.total_entries > 0 {
            report.estimated_rotation_seconds = Some(
                elapsed
                    .saturating_mul(report.total_entries as i64)
                    .saturating_div(report.verified_entries as i64),
            );
        }
        Ok(report)
    }
}

fn qualified_supported_audio_filter(alias: &str) -> String {
    crate::sample_sources::supported_audio_where_clause()
        .replace("extension", &format!("{alias}.extension"))
        .replace("path", &format!("{alias}.path"))
}

impl SourceWriteBatch<'_> {
    /// Record one revalidated file snapshot as verified in the active rotation.
    pub fn record_content_audit_verified(
        &mut self,
        path: &Path,
        rotation_id: i64,
        verified_at: i64,
        file_size: u64,
        modified_ns: i64,
        file_identity: Option<&str>,
    ) -> Result<(), SourceDbError> {
        let path = normalize_relative_path(path)?;
        self.tx
            .execute(
                "INSERT INTO source_content_audit_entries (
                    path, verified_rotation, verified_at, verified_file_size,
                    verified_modified_ns, verified_file_identity, last_attempt_at, retry_at,
                    skip_reason, attempts, bytes_read
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?3, NULL, NULL, 1, ?4)
                 ON CONFLICT(path) DO UPDATE SET
                    verified_rotation = excluded.verified_rotation,
                    verified_at = excluded.verified_at,
                    verified_file_size = excluded.verified_file_size,
                    verified_modified_ns = excluded.verified_modified_ns,
                    verified_file_identity = excluded.verified_file_identity,
                    last_attempt_at = excluded.last_attempt_at,
                    retry_at = NULL,
                    skip_reason = NULL,
                    attempts = source_content_audit_entries.attempts + 1,
                    bytes_read = source_content_audit_entries.bytes_read + excluded.bytes_read",
                params![
                    path,
                    rotation_id,
                    verified_at,
                    i64::try_from(file_size).unwrap_or(i64::MAX),
                    modified_ns,
                    file_identity
                ],
            )
            .map_err(map_sql_error)?;
        Ok(())
    }

    /// Keep one attempted path due with a stable reason and bounded retry deadline.
    pub fn record_content_audit_skipped(
        &mut self,
        path: &Path,
        attempted_at: i64,
        retry_at: i64,
        reason: ContentAuditSkipReason,
        bytes_read: u64,
    ) -> Result<(), SourceDbError> {
        let path = normalize_relative_path(path)?;
        self.tx
            .execute(
                "INSERT INTO source_content_audit_entries (
                    path, last_attempt_at, retry_at, skip_reason, attempts, bytes_read
                 ) VALUES (?1, ?2, ?3, ?4, 1, ?5)
                 ON CONFLICT(path) DO UPDATE SET
                    last_attempt_at = excluded.last_attempt_at,
                    retry_at = excluded.retry_at,
                    skip_reason = excluded.skip_reason,
                    attempts = source_content_audit_entries.attempts + 1,
                    bytes_read = source_content_audit_entries.bytes_read + excluded.bytes_read",
                params![
                    path,
                    attempted_at,
                    retry_at,
                    reason.token(),
                    i64::try_from(bytes_read).unwrap_or(i64::MAX)
                ],
            )
            .map_err(map_sql_error)?;
        Ok(())
    }

    /// Remove retry state whose path is no longer a live supported manifest entry.
    pub fn remove_stale_content_audit_entry(&mut self, path: &Path) -> Result<(), SourceDbError> {
        let path = normalize_relative_path(path)?;
        self.tx
            .execute(
                "DELETE FROM source_content_audit_entries WHERE path = ?1",
                [path],
            )
            .map_err(map_sql_error)?;
        Ok(())
    }

    /// Advance durable content-audit progress without conflating it with manifest completion.
    pub fn checkpoint_content_audit(
        &mut self,
        cursor: Option<&Path>,
        retry_cursor: Option<&Path>,
        retry_next: bool,
        checkpoint_revision: u64,
        bytes_read: u64,
        batch_at: i64,
    ) -> Result<(), SourceDbError> {
        let cursor = cursor.map(normalize_relative_path).transpose()?;
        let retry_cursor = retry_cursor.map(normalize_relative_path).transpose()?;
        self.tx
            .execute(
                "UPDATE source_content_audit_state
                 SET cursor = COALESCE(?1, cursor),
                     retry_cursor = COALESCE(?2, retry_cursor),
                     retry_next = ?3,
                     checkpoint_revision = ?4,
                     verified_entries = (
                         SELECT COUNT(*)
                         FROM source_content_audit_entries AS entry
                         JOIN wav_files AS wav ON wav.path = entry.path
                         WHERE entry.verified_rotation =
                                   source_content_audit_state.rotation_id
                           AND entry.verified_file_size = wav.file_size
                           AND entry.verified_modified_ns = wav.modified_ns
                           AND entry.verified_file_identity IS wav.file_identity
                           AND entry.skip_reason IS NULL
                           AND wav.missing = 0
                     ),
                     verified_bytes = COALESCE((
                         SELECT SUM(wav.file_size)
                         FROM source_content_audit_entries AS entry
                         JOIN wav_files AS wav ON wav.path = entry.path
                         WHERE entry.verified_rotation =
                                   source_content_audit_state.rotation_id
                           AND entry.verified_file_size = wav.file_size
                           AND entry.verified_modified_ns = wav.modified_ns
                           AND entry.verified_file_identity IS wav.file_identity
                           AND entry.skip_reason IS NULL
                           AND wav.missing = 0
                     ), 0),
                     bytes_read = bytes_read + ?5,
                     skipped_entries = (
                         SELECT COUNT(*)
                         FROM source_content_audit_entries AS entry
                         JOIN wav_files AS wav ON wav.path = entry.path
                         WHERE entry.skip_reason IS NOT NULL
                           AND wav.missing = 0
                     ),
                     last_batch_at = ?6
                 WHERE singleton = 1",
                params![
                    cursor,
                    retry_cursor,
                    i64::from(retry_next),
                    i64::try_from(checkpoint_revision).unwrap_or(i64::MAX),
                    i64::try_from(bytes_read).unwrap_or(i64::MAX),
                    batch_at
                ],
            )
            .map_err(map_sql_error)?;
        Ok(())
    }

    /// Close a fully verified rotation and begin the next durable rotation.
    pub fn complete_content_audit_rotation(
        &mut self,
        completed_at: i64,
        checkpoint_revision: u64,
    ) -> Result<(), SourceDbError> {
        self.tx
            .execute(
                "UPDATE source_content_audit_state
                 SET rotation_id = rotation_id + 1,
                     rotation_started_at = ?1,
                     cursor = '',
                     retry_cursor = '',
                     retry_next = 0,
                     checkpoint_revision = ?2,
                     verified_entries = 0,
                     verified_bytes = 0,
                     bytes_read = 0,
                     skipped_entries = 0,
                     last_batch_at = ?1,
                     last_rotation_completed_at = ?1,
                     last_rotation_seconds = MAX(0, ?1 - rotation_started_at)
                 WHERE singleton = 1",
                params![
                    completed_at,
                    i64::try_from(checkpoint_revision).unwrap_or(i64::MAX)
                ],
            )
            .map_err(map_sql_error)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn legacy_cursor_is_imported_without_claiming_verification() {
        let directory = tempfile::tempdir().expect("source");
        let database =
            SourceDatabase::open_for_source_write(directory.path()).expect("source database");
        let mut batch = database.write_batch().expect("legacy metadata batch");
        batch
            .set_metadata(LEGACY_CURSOR_KEY, "nested/last.wav")
            .expect("legacy cursor");
        batch.commit().expect("commit legacy cursor");

        let checkpoint = database
            .begin_or_resume_content_audit(100, 7)
            .expect("migrate content checkpoint");

        assert_eq!(checkpoint.rotation_id, 1);
        assert_eq!(checkpoint.cursor, "nested/last.wav");
        assert_eq!(checkpoint.retry_cursor, "");
        assert!(!checkpoint.retry_next);
        assert_eq!(checkpoint.checkpoint_revision, 7);
        assert!(
            database
                .get_metadata(LEGACY_CURSOR_KEY)
                .expect("legacy metadata")
                .is_none()
        );
        assert!(database.content_audit_entry_states().unwrap().is_empty());
    }

    #[test]
    fn completed_rotation_advances_generation_and_preserves_measurement() {
        let directory = tempfile::tempdir().expect("source");
        let database =
            SourceDatabase::open_for_source_write(directory.path()).expect("source database");
        database
            .begin_or_resume_content_audit(100, 1)
            .expect("begin rotation");
        let mut batch = database.write_batch().expect("completion batch");
        batch
            .complete_content_audit_rotation(160, 2)
            .expect("complete rotation");
        batch.commit().expect("commit completion");

        let report = database.content_audit_report(170).expect("coverage report");

        assert_eq!(report.rotation_id, 2);
        assert_eq!(report.rotation_started_at, 160);
        assert_eq!(report.last_rotation_completed_at, Some(160));
        assert_eq!(report.last_rotation_seconds, Some(60));
    }

    fn measure_vm_callbacks<T>(
        database: &SourceDatabase,
        operation: impl FnOnce() -> Result<T, SourceDbError>,
    ) -> usize {
        let callbacks = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&callbacks);
        database.connection.progress_handler(
            100,
            Some(move || {
                observed.fetch_add(1, Ordering::Relaxed);
                false
            }),
        );
        operation().expect("bounded planning");
        database
            .connection
            .progress_handler(0, None::<fn() -> bool>);
        callbacks.load(Ordering::Relaxed)
    }

    fn forward_planning_vm_callbacks(total_entries: usize) -> usize {
        let directory = tempfile::tempdir().expect("source");
        let database =
            SourceDatabase::open_for_source_write(directory.path()).expect("source database");
        let transaction = database
            .connection
            .unchecked_transaction()
            .expect("fixture transaction");
        {
            let mut insert_manifest = transaction
                .prepare(
                    "INSERT INTO wav_files (path, file_size, modified_ns, extension)
                     VALUES (?1, 64, ?2, ?3)",
                )
                .expect("manifest insert");
            for index in 0..total_entries {
                let path = format!("sample-{index:06}.wav");
                insert_manifest
                    .execute(params![
                        path,
                        index as i64,
                        if index == 0 { "wav" } else { "txt" }
                    ])
                    .expect("insert manifest row");
            }
        }
        transaction.commit().expect("commit planning fixture");

        let cursor = format!("sample-{:06}.wav", total_entries / 2);
        measure_vm_callbacks(&database, || {
            let candidates = database.content_audit_forward_candidates(1, &cursor, 1)?;
            assert_eq!(candidates.len(), 1);
            assert_eq!(
                candidates[0].entry.relative_path,
                Path::new("sample-000000.wav")
            );
            Ok(candidates)
        })
    }

    fn retry_planning_vm_callbacks(total_entries: usize) -> (usize, usize) {
        let directory = tempfile::tempdir().expect("source");
        let database =
            SourceDatabase::open_for_source_write(directory.path()).expect("source database");
        let transaction = database
            .connection
            .unchecked_transaction()
            .expect("fixture transaction");
        {
            let mut insert_manifest = transaction
                .prepare(
                    "INSERT INTO wav_files (path, file_size, modified_ns, extension)
                     VALUES (?1, 64, ?2, ?3)",
                )
                .expect("manifest insert");
            let mut insert_retry = transaction
                .prepare(
                    "INSERT INTO source_content_audit_entries (
                         path, last_attempt_at, retry_at, skip_reason, attempts
                     ) VALUES (?1, 1, ?2, 'unavailable', 1)",
                )
                .expect("retry insert");
            for index in 0..total_entries {
                let path = format!("sample-{index:06}.wav");
                insert_manifest
                    .execute(params![
                        path,
                        index as i64,
                        if index + 1 == total_entries {
                            "wav"
                        } else {
                            "txt"
                        }
                    ])
                    .expect("insert manifest row");
                insert_retry
                    .execute(params![path, 1])
                    .expect("insert retry row");
            }
        }
        transaction.commit().expect("commit retry fixture");

        let sparse_due = measure_vm_callbacks(&database, || {
            let candidates = database.content_audit_retry_candidates("", 10, 1)?;
            assert_eq!(candidates.len(), 1);
            assert!(candidates.iter().all(|candidate| candidate.entry.is_none()));
            Ok(candidates)
        });
        let none_due = measure_vm_callbacks(&database, || {
            let candidates = database.content_audit_retry_candidates("", 0, 1)?;
            assert!(candidates.is_empty());
            Ok(candidates)
        });
        (sparse_due, none_due)
    }

    fn forward_audit_state_vm_callbacks(total_entries: usize) -> usize {
        let directory = tempfile::tempdir().expect("source");
        let database =
            SourceDatabase::open_for_source_write(directory.path()).expect("source database");
        let transaction = database
            .connection
            .unchecked_transaction()
            .expect("fixture transaction");
        {
            let mut insert_manifest = transaction
                .prepare(
                    "INSERT INTO wav_files (path, file_size, modified_ns, extension)
                     VALUES (?1, 64, ?2, 'wav')",
                )
                .expect("manifest insert");
            let mut insert_audit = transaction
                .prepare(
                    "INSERT INTO source_content_audit_entries (
                         path, verified_rotation, verified_file_size, verified_modified_ns,
                         skip_reason
                     ) VALUES (?1, 1, 64, ?2, ?3)",
                )
                .expect("audit insert");
            for index in 0..total_entries {
                let path = format!("sample-{index:06}.wav");
                insert_manifest
                    .execute(params![path, index as i64])
                    .expect("insert manifest row");
                insert_audit
                    .execute(params![
                        path,
                        index as i64,
                        (index % 2 == 1).then_some("unavailable")
                    ])
                    .expect("insert audit row");
            }
        }
        transaction.commit().expect("commit audit-state fixture");

        let cursor = format!("sample-{:06}.wav", total_entries / 2);
        measure_vm_callbacks(&database, || {
            let candidates = database.content_audit_forward_candidates(1, &cursor, 1)?;
            assert_eq!(candidates.len(), 1);
            assert!(!candidates[0].needs_verification);
            Ok(candidates)
        })
    }

    #[test]
    fn content_audit_planning_stays_constant_at_large_manifest_scale() {
        let forward_10k = forward_planning_vm_callbacks(10_000);
        let forward_100k = forward_planning_vm_callbacks(100_000);
        let forward_state_10k = forward_audit_state_vm_callbacks(10_000);
        let forward_state_100k = forward_audit_state_vm_callbacks(100_000);
        let (sparse_retry_10k, no_retry_10k) = retry_planning_vm_callbacks(10_000);
        let (sparse_retry_100k, no_retry_100k) = retry_planning_vm_callbacks(100_000);

        assert!(
            forward_100k <= forward_10k.saturating_add(5)
                && forward_state_100k <= forward_state_10k.saturating_add(5)
                && sparse_retry_100k <= sparse_retry_10k.saturating_add(5)
                && no_retry_100k <= no_retry_10k.saturating_add(5),
            "bounded candidate planning must not scale with manifest rows: \
             forward 10k={forward_10k}, 100k={forward_100k}; \
             forward state 10k={forward_state_10k}, 100k={forward_state_100k}; \
             sparse retry 10k={sparse_retry_10k}, 100k={sparse_retry_100k}; \
             no retry 10k={no_retry_10k}, 100k={no_retry_100k}"
        );
        assert!(
            [
                forward_10k,
                forward_100k,
                forward_state_10k,
                forward_state_100k,
                sparse_retry_10k,
                sparse_retry_100k,
                no_retry_10k,
                no_retry_100k,
            ]
            .into_iter()
            .all(|callbacks| callbacks <= 50),
            "one-entry planning exceeded the 5,000 SQLite VM-instruction ceiling: \
             forward 10k={forward_10k}, 100k={forward_100k}; \
             forward state 10k={forward_state_10k}, 100k={forward_state_100k}; \
             sparse retry 10k={sparse_retry_10k}, 100k={sparse_retry_100k}; \
             no retry 10k={no_retry_10k}, 100k={no_retry_100k} callbacks"
        );
    }
}
