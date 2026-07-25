use std::{
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rusqlite::{OptionalExtension, params};

use super::super::util::map_sql_error;
use super::super::{Rating, SampleSoundType, SourceDbError, SourceWriteBatch};
use super::command::{SourceContentHashWrite, SourceFileWrite, SourceTagWrite};
use super::mutation::{
    update_flag_statement, update_path_i64_statement, update_path_null_statement,
    update_path_text_statement,
};
use super::upsert::{ContentHashPolicy, TagPolicy, WavFileWriteSpec, execute_wav_upsert};

const UPDATE_TAG_SQL: &str = "UPDATE wav_files SET tag = ?1 WHERE path = ?2";
const UPDATE_LOOPED_SQL: &str = "UPDATE wav_files SET looped = ?1 WHERE path = ?2";
const UPDATE_LOCKED_SQL: &str = "UPDATE wav_files SET locked = ?1 WHERE path = ?2";
const UPDATE_SOUND_TYPE_SQL: &str = "UPDATE wav_files SET sound_type = ?1 WHERE path = ?2";
const CLEAR_SOUND_TYPE_SQL: &str = "UPDATE wav_files SET sound_type = NULL WHERE path = ?1";
const UPDATE_USER_TAG_SQL: &str = "UPDATE wav_files SET user_tag = ?1 WHERE path = ?2";
const CLEAR_USER_TAG_SQL: &str = "UPDATE wav_files SET user_tag = NULL WHERE path = ?1";
const UPDATE_TAG_NAMED_SQL: &str = "UPDATE wav_files SET tag_named = ?1 WHERE path = ?2";
const UPDATE_MISSING_SQL: &str = "UPDATE wav_files SET missing = ?1 WHERE path = ?2";
const UPDATE_LAST_PLAYED_AT_SQL: &str = "UPDATE wav_files SET last_played_at = ?1 WHERE path = ?2";
const CLEAR_LAST_PLAYED_AT_SQL: &str = "UPDATE wav_files SET last_played_at = NULL WHERE path = ?1";
const UPDATE_LAST_CURATED_AT_SQL: &str =
    "UPDATE wav_files SET last_curated_at = ?1 WHERE path = ?2";
const CLEAR_LAST_CURATED_AT_SQL: &str =
    "UPDATE wav_files SET last_curated_at = NULL WHERE path = ?1";

impl<'conn> SourceWriteBatch<'conn> {
    pub(super) fn apply_file_write(
        &mut self,
        write: SourceFileWrite<'_>,
    ) -> Result<(), SourceDbError> {
        let content_hash = match write.content_hash {
            SourceContentHashWrite::Preserve => ContentHashPolicy::Preserve,
            SourceContentHashWrite::Clear => ContentHashPolicy::Clear,
            SourceContentHashWrite::Set(value) => ContentHashPolicy::Set(value),
        };
        let tag = match write.tag {
            SourceTagWrite::Preserve => TagPolicy::Preserve,
            SourceTagWrite::Set(value) => TagPolicy::Set(value),
        };
        self.upsert_file_with_policies(
            write.relative_path,
            write.file_size,
            write.modified_ns,
            content_hash,
            tag,
            write.missing,
        )
    }

    /// Insert or update a metadata key/value pair within the active batch.
    pub fn set_metadata(&mut self, key: &str, value: &str) -> Result<(), SourceDbError> {
        self.tx
            .execute(
                "INSERT INTO metadata (key, value)
                 VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map_err(map_sql_error)?;
        Ok(())
    }

    /// Insert or update a wav row, resetting the tag to neutral on first insert.
    pub fn upsert_file(
        &mut self,
        relative_path: &Path,
        file_size: u64,
        modified_ns: i64,
    ) -> Result<(), SourceDbError> {
        self.upsert_file_with_policies(
            relative_path,
            file_size,
            modified_ns,
            ContentHashPolicy::Preserve,
            TagPolicy::Preserve,
            false,
        )
    }

    /// Insert or update a wav file row while clearing any stored content hash.
    pub fn upsert_file_without_hash(
        &mut self,
        relative_path: &Path,
        file_size: u64,
        modified_ns: i64,
    ) -> Result<(), SourceDbError> {
        self.upsert_file_with_policies(
            relative_path,
            file_size,
            modified_ns,
            ContentHashPolicy::Clear,
            TagPolicy::Preserve,
            false,
        )
    }

    /// Insert or update a wav file row, including the content hash.
    pub fn upsert_file_with_hash(
        &mut self,
        relative_path: &Path,
        file_size: u64,
        modified_ns: i64,
        content_hash: &str,
    ) -> Result<(), SourceDbError> {
        self.upsert_file_with_policies(
            relative_path,
            file_size,
            modified_ns,
            ContentHashPolicy::Set(content_hash),
            TagPolicy::Preserve,
            false,
        )
    }

    /// Insert or update a wav file row with a specific tag and missing flag.
    pub fn upsert_file_with_hash_and_tag(
        &mut self,
        relative_path: &Path,
        file_size: u64,
        modified_ns: i64,
        content_hash: &str,
        tag: Rating,
        missing: bool,
    ) -> Result<(), SourceDbError> {
        self.upsert_file_with_policies(
            relative_path,
            file_size,
            modified_ns,
            ContentHashPolicy::Set(content_hash),
            TagPolicy::Set(tag),
            missing,
        )
    }

    /// Insert or update a wav file with a specific tag while leaving its hash unset.
    pub fn upsert_file_without_hash_and_tag(
        &mut self,
        relative_path: &Path,
        file_size: u64,
        modified_ns: i64,
        tag: Rating,
        missing: bool,
    ) -> Result<(), SourceDbError> {
        self.upsert_file_with_policies(
            relative_path,
            file_size,
            modified_ns,
            ContentHashPolicy::Clear,
            TagPolicy::Set(tag),
            missing,
        )
    }

    /// Persist the stable filesystem-object identity observed by the scanner.
    pub fn set_file_identity(
        &mut self,
        relative_path: &Path,
        file_identity: Option<&str>,
    ) -> Result<(), SourceDbError> {
        let previous_identity = self
            .tx
            .query_row(
                "SELECT file_identity FROM wav_files WHERE path = ?1",
                [relative_path.to_string_lossy().as_ref()],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(map_sql_error)?
            .flatten();
        if previous_identity.as_deref() != file_identity {
            self.identities_revision_dirty = true;
        }
        self.manifest_touched_paths
            .insert(relative_path.to_path_buf());
        match file_identity {
            Some(identity) => update_path_text_statement(
                &self.tx,
                "UPDATE wav_files SET file_identity = ?1 WHERE path = ?2",
                relative_path,
                identity,
            ),
            None => update_path_null_statement(
                &self.tx,
                "UPDATE wav_files SET file_identity = NULL WHERE path = ?1",
                relative_path,
            ),
        }
    }

    fn upsert_file_with_policies(
        &mut self,
        relative_path: &Path,
        file_size: u64,
        modified_ns: i64,
        content_hash: ContentHashPolicy<'_>,
        tag: TagPolicy,
        missing: bool,
    ) -> Result<(), SourceDbError> {
        self.paths_revision_dirty = true;
        self.identities_revision_dirty = true;
        self.manifest_touched_paths
            .insert(relative_path.to_path_buf());
        self.clear_pending_rename_for_live_path(relative_path)?;
        execute_wav_upsert(
            &self.tx,
            WavFileWriteSpec {
                relative_path,
                file_size,
                modified_ns,
                content_hash,
                tag,
                missing,
            },
        )
    }

    /// Update the tag for a wav row within the batch.
    pub fn set_tag(&mut self, relative_path: &Path, tag: Rating) -> Result<(), SourceDbError> {
        update_path_i64_statement(&self.tx, UPDATE_TAG_SQL, relative_path, tag.as_i64())?;
        self.touch_last_curated_at(relative_path)
    }

    /// Update the loop marker for a wav row within the batch.
    pub fn set_looped(&mut self, relative_path: &Path, looped: bool) -> Result<(), SourceDbError> {
        update_flag_statement(&self.tx, UPDATE_LOOPED_SQL, relative_path, looped)?;
        self.touch_last_curated_at(relative_path)
    }

    /// Update the keep-lock marker for a wav row within the batch.
    pub fn set_locked(&mut self, relative_path: &Path, locked: bool) -> Result<(), SourceDbError> {
        update_flag_statement(&self.tx, UPDATE_LOCKED_SQL, relative_path, locked)?;
        self.touch_last_curated_at(relative_path)
    }

    /// Update the sound classification for a wav row within the batch.
    pub fn set_sound_type(
        &mut self,
        relative_path: &Path,
        sound_type: Option<SampleSoundType>,
    ) -> Result<(), SourceDbError> {
        match sound_type {
            Some(sound_type) => update_path_text_statement(
                &self.tx,
                UPDATE_SOUND_TYPE_SQL,
                relative_path,
                sound_type.token(),
            ),
            None => update_path_null_statement(&self.tx, CLEAR_SOUND_TYPE_SQL, relative_path),
        }?;
        self.touch_last_curated_at(relative_path)
    }

    /// Update the custom user tag for a wav row within the batch.
    pub fn set_user_tag(
        &mut self,
        relative_path: &Path,
        user_tag: Option<&str>,
    ) -> Result<(), SourceDbError> {
        match user_tag.filter(|tag| !tag.trim().is_empty()) {
            Some(user_tag) => {
                update_path_text_statement(&self.tx, UPDATE_USER_TAG_SQL, relative_path, user_tag)
            }
            None => update_path_null_statement(&self.tx, CLEAR_USER_TAG_SQL, relative_path),
        }?;
        self.touch_last_curated_at(relative_path)
    }

    /// Update the tag-derived filename marker for a wav row within the batch.
    pub fn set_tag_named(
        &mut self,
        relative_path: &Path,
        tag_named: bool,
    ) -> Result<(), SourceDbError> {
        update_flag_statement(&self.tx, UPDATE_TAG_NAMED_SQL, relative_path, tag_named)?;
        self.touch_last_curated_at(relative_path)
    }

    /// Update the missing flag for a wav row within the batch.
    pub fn set_missing(
        &mut self,
        relative_path: &Path,
        missing: bool,
    ) -> Result<(), SourceDbError> {
        self.identities_revision_dirty = true;
        self.manifest_touched_paths
            .insert(relative_path.to_path_buf());
        update_flag_statement(&self.tx, UPDATE_MISSING_SQL, relative_path, missing)
    }

    /// Update the last played timestamp for a wav row within the batch.
    pub fn set_last_played_at(
        &mut self,
        relative_path: &Path,
        played_at: i64,
    ) -> Result<(), SourceDbError> {
        update_path_i64_statement(
            &self.tx,
            UPDATE_LAST_PLAYED_AT_SQL,
            relative_path,
            played_at,
        )
    }

    /// Clear the last played timestamp for a wav row within the batch.
    pub fn clear_last_played_at(&mut self, relative_path: &Path) -> Result<(), SourceDbError> {
        update_path_null_statement(&self.tx, CLEAR_LAST_PLAYED_AT_SQL, relative_path)
    }

    /// Update the last curation timestamp for a wav row within the batch.
    pub fn set_last_curated_at(
        &mut self,
        relative_path: &Path,
        curated_at: i64,
    ) -> Result<(), SourceDbError> {
        update_path_i64_statement(
            &self.tx,
            UPDATE_LAST_CURATED_AT_SQL,
            relative_path,
            curated_at,
        )
    }

    /// Clear the last curation timestamp for a wav row within the batch.
    pub fn clear_last_curated_at(&mut self, relative_path: &Path) -> Result<(), SourceDbError> {
        update_path_null_statement(&self.tx, CLEAR_LAST_CURATED_AT_SQL, relative_path)
    }

    pub(crate) fn touch_last_curated_at(
        &mut self,
        relative_path: &Path,
    ) -> Result<(), SourceDbError> {
        self.set_last_curated_at(relative_path, epoch_seconds())
    }
}

fn epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs() as i64
}
