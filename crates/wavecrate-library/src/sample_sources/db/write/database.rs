use std::path::{Path, PathBuf};
use std::time::Instant;

use rusqlite::{Transaction, TransactionBehavior};

use super::super::util::map_sql_error;
use super::super::{
    META_WAV_PATHS_REVISION, Rating, SampleSoundType, SourceDatabase, SourceDbError,
    SourceWriteBatch,
};
use super::event::record_source_db_event;
use super::{SourceContentHashWrite, SourceFileWrite, SourceTagWrite, SourceWriteCommand};

impl SourceDatabase {
    /// Upsert a wav file row using the path relative to the source root.
    pub fn upsert_file(
        &self,
        relative_path: &Path,
        file_size: u64,
        modified_ns: i64,
    ) -> Result<(), SourceDbError> {
        self.execute_write(SourceWriteCommand::UpsertFile(SourceFileWrite {
            relative_path,
            file_size,
            modified_ns,
            content_hash: SourceContentHashWrite::Preserve,
            tag: SourceTagWrite::Preserve,
            missing: false,
        }))
    }

    /// Persist a keep/trash tag for a single wav file by relative path.
    pub fn set_tag(&self, relative_path: &Path, tag: Rating) -> Result<(), SourceDbError> {
        self.execute_write(SourceWriteCommand::SetTag {
            path: relative_path,
            tag,
        })
    }

    /// Persist a loop marker for a single wav file by relative path.
    pub fn set_looped(&self, relative_path: &Path, looped: bool) -> Result<(), SourceDbError> {
        self.execute_write(SourceWriteCommand::SetLooped {
            path: relative_path,
            looped,
        })
    }

    /// Persist a keep-lock marker for a single wav file by relative path.
    pub fn set_locked(&self, relative_path: &Path, locked: bool) -> Result<(), SourceDbError> {
        self.execute_write(SourceWriteCommand::SetLocked {
            path: relative_path,
            locked,
        })
    }

    /// Persist a canonical sound classification for a single wav file by relative path.
    pub fn set_sound_type(
        &self,
        relative_path: &Path,
        sound_type: Option<SampleSoundType>,
    ) -> Result<(), SourceDbError> {
        self.execute_write(SourceWriteCommand::SetSoundType {
            path: relative_path,
            sound_type,
        })
    }

    /// Persist a single custom tag for a wav file by relative path.
    pub fn set_user_tag(
        &self,
        relative_path: &Path,
        user_tag: Option<&str>,
    ) -> Result<(), SourceDbError> {
        self.execute_write(SourceWriteCommand::SetUserTag {
            path: relative_path,
            user_tag,
        })
    }

    /// Persist whether a wav file is named from tag metadata.
    pub fn set_tag_named(
        &self,
        relative_path: &Path,
        tag_named: bool,
    ) -> Result<(), SourceDbError> {
        self.execute_write(SourceWriteCommand::SetTagNamed {
            path: relative_path,
            tag_named,
        })
    }

    /// Persist multiple tag changes in one transaction, coalescing SQLite work.
    pub fn set_tags_batch(&self, updates: &[(PathBuf, Rating)]) -> Result<(), SourceDbError> {
        if updates.is_empty() {
            return Ok(());
        }

        let started_at = Instant::now();
        let mut batch = self.write_batch()?;
        for (path, tag) in updates {
            batch.execute_write(SourceWriteCommand::SetTag { path, tag: *tag })?;
        }
        let result = batch.commit();
        record_source_db_event(
            "source_db.set_tags_batch",
            &self.root,
            started_at,
            result.as_ref().map(|_| ()),
        );
        result
    }

    /// Update the missing flag for a wav file by relative path.
    pub fn set_missing(&self, relative_path: &Path, missing: bool) -> Result<(), SourceDbError> {
        self.execute_write(SourceWriteCommand::SetMissing {
            path: relative_path,
            missing,
        })
    }

    /// Record the most recent playback timestamp for a wav file.
    pub fn set_last_played_at(
        &self,
        relative_path: &Path,
        played_at: i64,
    ) -> Result<(), SourceDbError> {
        self.execute_write(SourceWriteCommand::SetLastPlayedAt {
            path: relative_path,
            played_at: Some(played_at),
        })
    }

    /// Clear the recorded most recent playback timestamp for a wav file.
    pub fn clear_last_played_at(&self, relative_path: &Path) -> Result<(), SourceDbError> {
        self.execute_write(SourceWriteCommand::SetLastPlayedAt {
            path: relative_path,
            played_at: None,
        })
    }

    /// Record the most recent explicit curation timestamp for a wav file.
    pub fn set_last_curated_at(
        &self,
        relative_path: &Path,
        curated_at: i64,
    ) -> Result<(), SourceDbError> {
        self.execute_write(SourceWriteCommand::SetLastCuratedAt {
            path: relative_path,
            curated_at: Some(curated_at),
        })
    }

    /// Clear the recorded curation timestamp for a wav file.
    pub fn clear_last_curated_at(&self, relative_path: &Path) -> Result<(), SourceDbError> {
        self.execute_write(SourceWriteCommand::SetLastCuratedAt {
            path: relative_path,
            curated_at: None,
        })
    }

    /// Remove a wav file row by relative path.
    pub fn remove_file(&self, relative_path: &Path) -> Result<(), SourceDbError> {
        self.execute_write(SourceWriteCommand::RemoveFile {
            path: relative_path,
        })
    }

    /// Start a write batch that wraps related mutations in a single transaction.
    pub fn write_batch(&self) -> Result<SourceWriteBatch<'_>, SourceDbError> {
        // Acquire the writer lock up front so mixed scan/metadata workloads wait
        // on SQLite's busy timeout instead of failing partway through the batch.
        let tx = Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)
            .map_err(map_sql_error)?;
        Ok(SourceWriteBatch {
            tx,
            db_path: self.db_path.clone(),
            paths_revision_dirty: false,
            identities_revision_dirty: false,
            index_revision_dirty: false,
            manifest_touched_paths: std::collections::BTreeSet::new(),
            telemetry_label: self.telemetry_label,
        })
    }

    /// Insert or update a metadata key/value pair.
    pub fn set_metadata(&self, key: &str, value: &str) -> Result<(), SourceDbError> {
        self.execute_write(SourceWriteCommand::SetMetadata { key, value })
    }

    pub(super) fn bump_revision(conn: &rusqlite::Connection) -> Result<(), SourceDbError> {
        Self::bump_metadata_counter(conn, "revision")
    }

    pub(super) fn bump_wav_paths_revision(
        conn: &rusqlite::Connection,
    ) -> Result<(), SourceDbError> {
        Self::bump_metadata_counter(conn, META_WAV_PATHS_REVISION)
    }

    pub(super) fn bump_wav_identities_revision(
        conn: &rusqlite::Connection,
    ) -> Result<(), SourceDbError> {
        Self::bump_metadata_counter(conn, super::super::META_WAV_IDENTITIES_REVISION)
    }

    pub(super) fn bump_source_index_revision(
        conn: &rusqlite::Connection,
    ) -> Result<(), SourceDbError> {
        Self::bump_metadata_counter(conn, super::super::META_SOURCE_INDEX_REVISION)
    }

    fn bump_metadata_counter(conn: &rusqlite::Connection, key: &str) -> Result<(), SourceDbError> {
        conn.execute(
            "INSERT INTO metadata (key, value)
             VALUES (?1, '1')
             ON CONFLICT(key) DO UPDATE SET value = CAST(CAST(value AS INTEGER) + 1 AS TEXT)",
            [key],
        )
        .map_err(map_sql_error)?;
        Ok(())
    }

    pub(super) fn mutate_with_batch(
        &self,
        operation: &'static str,
        mutate: impl FnOnce(&mut SourceWriteBatch<'_>) -> Result<(), SourceDbError>,
    ) -> Result<(), SourceDbError> {
        let started_at = Instant::now();
        let mut batch = self.write_batch()?;
        mutate(&mut batch)?;
        let result = batch.commit();
        record_source_db_event(
            operation,
            &self.root,
            started_at,
            result.as_ref().map(|_| ()),
        );
        result
    }
}
