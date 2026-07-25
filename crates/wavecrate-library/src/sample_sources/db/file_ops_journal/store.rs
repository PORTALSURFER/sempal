use rusqlite::{OptionalExtension, params};

use super::super::util::{map_sql_error, normalize_relative_path};
use super::super::{SourceDatabase, SourceDbError};
use super::decode::{ListedJournalEntries, decode_journal_row};
use super::entry::{FileOpJournalEntry, FileOpStage};
use super::recovery_io::{capture_file_identity, capture_root_identity};

/// Narrow persistence boundary for durable file-operation journal rows.
pub(crate) struct FileOpJournalStore<'a> {
    db: &'a SourceDatabase,
}

impl<'a> FileOpJournalStore<'a> {
    pub(crate) fn new(db: &'a SourceDatabase) -> Self {
        Self { db }
    }

    pub(crate) fn insert(&self, entry: &FileOpJournalEntry) -> Result<(), SourceDbError> {
        insert_entry(self.db, entry)
    }

    pub(crate) fn update_stage(
        &self,
        id: &str,
        stage: FileOpStage,
        file_size: Option<u64>,
        modified_ns: Option<i64>,
    ) -> Result<(), SourceDbError> {
        update_stage(self.db, id, stage, file_size, modified_ns)
    }

    pub(crate) fn remove(&self, id: &str) -> Result<(), SourceDbError> {
        remove_entry(self.db, id)
    }

    pub(crate) fn list(&self) -> Result<ListedJournalEntries, SourceDbError> {
        list_entries(self.db)
    }
}

/// Insert a new journal entry before mutating the filesystem.
pub(crate) fn insert_entry(
    db: &SourceDatabase,
    entry: &FileOpJournalEntry,
) -> Result<(), SourceDbError> {
    let target_relative = normalize_relative_path(&entry.target_relative)?;
    let staged_relative = match entry.staged_relative.as_ref() {
        Some(path) => Some(normalize_relative_path(path)?),
        None => None,
    };
    let source_relative = match entry.source_relative.as_ref() {
        Some(path) => Some(normalize_relative_path(path)?),
        None => None,
    };
    let target_root_identity = match entry.target_root_identity.clone() {
        Some(identity) => Some(identity),
        None => Some(capture_root_identity(db.root()).map_err(|source| {
            SourceDbError::InspectSourceDatabasePath {
                path: db.root().to_path_buf(),
                source,
            }
        })?),
    };
    let source_root_identity = match entry.source_root.as_ref() {
        Some(source_root) => match entry.source_root_identity.clone() {
            Some(identity) => Some(identity),
            None => Some(capture_root_identity(source_root).map_err(|source| {
                SourceDbError::InspectSourceDatabasePath {
                    path: source_root.clone(),
                    source,
                }
            })?),
        },
        None => None,
    };
    let file_identity = entry.file_identity.clone();
    db.connection
        .execute(
            "INSERT INTO file_ops_journal (
                 id, op_type, stage, source_root, source_relative, target_relative,
                 staged_relative, file_size, modified_ns, file_identity,
                 target_root_identity, source_root_identity, tag, looped, locked,
                 last_played_at, last_curated_at, created_at
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
            params![
                entry.id,
                entry.kind.as_str(),
                entry.stage.as_str(),
                entry
                    .source_root
                    .as_ref()
                    .map(|path| path.to_string_lossy().to_string()),
                source_relative,
                target_relative,
                staged_relative,
                entry.file_size.map(|size| size as i64),
                entry.modified_ns,
                file_identity,
                target_root_identity,
                source_root_identity,
                entry.tag.map(|tag| tag.as_i64()),
                entry.looped.map(|looped| if looped { 1i64 } else { 0i64 }),
                entry.locked.map(|locked| if locked { 1i64 } else { 0i64 }),
                entry.last_played_at,
                entry.last_curated_at,
                entry.created_at,
            ],
        )
        .map_err(map_sql_error)?;
    Ok(())
}

/// Update a journal entry stage and optional metadata after filesystem work.
pub(crate) fn update_stage(
    db: &SourceDatabase,
    id: &str,
    stage: FileOpStage,
    file_size: Option<u64>,
    modified_ns: Option<i64>,
) -> Result<(), SourceDbError> {
    let file_identity = if file_size.is_some() || modified_ns.is_some() {
        file_identity_for_row(db, id)?
    } else {
        None
    };
    db.connection
        .execute(
            "UPDATE file_ops_journal
             SET stage = ?1,
                 file_size = COALESCE(?2, file_size),
                 modified_ns = COALESCE(?3, modified_ns),
                 file_identity = COALESCE(?4, file_identity)
             WHERE id = ?5",
            params![
                stage.as_str(),
                file_size.map(|size| size as i64),
                modified_ns,
                file_identity,
                id,
            ],
        )
        .map_err(map_sql_error)?;
    Ok(())
}

/// Remove a resolved journal entry after reconciliation.
pub(crate) fn remove_entry(db: &SourceDatabase, id: &str) -> Result<(), SourceDbError> {
    db.connection
        .execute("DELETE FROM file_ops_journal WHERE id = ?1", params![id])
        .map_err(map_sql_error)?;
    Ok(())
}

/// Load all pending journal entries for reconciliation.
pub(crate) fn list_entries(db: &SourceDatabase) -> Result<ListedJournalEntries, SourceDbError> {
    let mut stmt = db
        .connection
        .prepare(
            "SELECT id, op_type, stage, source_root, source_relative, target_relative,
                    staged_relative, file_size, modified_ns, tag, looped, locked, last_played_at,
                    last_curated_at, created_at, file_identity, target_root_identity,
                    source_root_identity
             FROM file_ops_journal",
        )
        .map_err(map_sql_error)?;
    let mut rows = stmt.query([]).map_err(map_sql_error)?;
    let mut listed = ListedJournalEntries::default();
    while let Some(row) = rows.next().map_err(map_sql_error)? {
        match decode_journal_row(row) {
            Ok(entry) => listed.entries.push(entry),
            Err(malformed) => {
                tracing::warn!("{}", malformed.describe());
                listed.malformed.push(malformed);
            }
        }
    }
    Ok(listed)
}

fn file_identity_for_row(db: &SourceDatabase, id: &str) -> Result<Option<String>, SourceDbError> {
    let paths = db
        .connection
        .query_row(
            "SELECT staged_relative, target_relative
             FROM file_ops_journal
             WHERE id = ?1",
            params![id],
            |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(map_sql_error)?;
    let Some((staged_relative, target_relative)) = paths else {
        return Ok(None);
    };
    let try_capture =
        |relative: &str| match capture_file_identity(db.root(), std::path::Path::new(relative)) {
            Ok(identity) => Ok(Some(identity)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(SourceDbError::InspectSourceDatabasePath {
                path: db.root().join(relative),
                source,
            }),
        };
    if let Some(staged_relative) = staged_relative
        && let Some(identity) = try_capture(&staged_relative)?
    {
        return Ok(Some(identity));
    }
    try_capture(&target_relative)
}
