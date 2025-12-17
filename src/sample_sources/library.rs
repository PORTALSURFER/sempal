//! Global SQLite storage for sources and collections that should not live in the config file.

use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use rusqlite::{Connection, Transaction, params};
use thiserror::Error;

mod schema;

use super::{Collection, CollectionId, SampleSource, SourceId};
use crate::app_dirs;
use crate::sample_sources::collections::{CollectionMember, collection_folder_name_from_str};
use crate::sample_sources::config::normalize_path;

/// Filename for the global library database stored under the user app directory.
pub const LIBRARY_DB_FILE_NAME: &str = "library.db";

/// Aggregate state loaded from or written to the library database.
#[derive(Debug, Clone, Default)]
pub struct LibraryState {
    pub sources: Vec<SampleSource>,
    pub collections: Vec<Collection>,
}

const COLLECTION_EXPORT_PATHS_VERSION_KEY: &str = "collections_export_paths_version";
const COLLECTION_EXPORT_PATHS_VERSION_V2: &str = "2";
const COLLECTION_MEMBER_CLIP_ROOT_VERSION_KEY: &str = "collection_members_clip_root_version";
const COLLECTION_MEMBER_CLIP_ROOT_VERSION_V1: &str = "1";

static LIBRARY_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

/// Errors returned when operating on the library database.
#[derive(Debug, Error)]
pub enum LibraryError {
    /// No suitable application directory was available.
    #[error("No suitable config directory available for library database")]
    NoConfigDir,
    /// Failed to create the directory for the database file.
    #[error("Could not create library directory {path}: {source}")]
    CreateDir {
        path: PathBuf,
        source: std::io::Error,
    },
    /// Failed to open or query the database.
    #[error("Library database query failed: {0}")]
    Sql(#[from] rusqlite::Error),
}

/// Load all sources and collections from the global library database, creating it if missing.
pub fn load() -> Result<LibraryState, LibraryError> {
    let _guard = LIBRARY_LOCK.lock().expect("library lock mutex poisoned");
    let db = LibraryDatabase::open()?;
    db.load_state()
}

/// Persist sources and collections to the global library database, replacing existing rows.
pub fn save(state: &LibraryState) -> Result<(), LibraryError> {
    let _guard = LIBRARY_LOCK.lock().expect("library lock mutex poisoned");
    let mut db = LibraryDatabase::open()?;
    db.replace_state(state)
}

struct LibraryDatabase {
    connection: Connection,
}

impl LibraryDatabase {
    fn open() -> Result<Self, LibraryError> {
        let db_path = database_path()?;
        create_parent_if_needed(&db_path)?;
        let connection = Connection::open(&db_path)?;
        let mut db = Self { connection };
        db.apply_pragmas()?;
        db.apply_schema()?;
        db.migrate_collection_member_clip_roots()?;
        db.migrate_collection_export_paths()?;
        db.migrate_analysis_jobs_content_hash()?;
        db.migrate_samples_analysis_metadata()?;
        db.migrate_features_table()?;
        db.migrate_labels_weak_table()?;
        Ok(db)
    }

    fn load_state(&self) -> Result<LibraryState, LibraryError> {
        let sources = self.load_sources()?;
        let collections = self.load_collections()?;
        Ok(LibraryState {
            sources,
            collections,
        })
    }

    fn replace_state(&mut self, state: &LibraryState) -> Result<(), LibraryError> {
        let tx = self.connection.transaction().map_err(map_sql_error)?;
        Self::replace_sources(&tx, &state.sources)?;
        Self::replace_collections(&tx, &state.collections)?;
        tx.commit().map_err(map_sql_error)?;
        Ok(())
    }

    fn load_sources(&self) -> Result<Vec<SampleSource>, LibraryError> {
        let mut stmt = self
            .connection
            .prepare(
                "SELECT id, root
                 FROM sources
                 ORDER BY sort_order ASC, id ASC",
            )
            .map_err(map_sql_error)?;
        let rows = stmt
            .query_map([], |row| {
                let id: String = row.get(0)?;
                let root: String = row.get(1)?;
                Ok(SampleSource {
                    id: SourceId::from_string(id),
                    root: PathBuf::from(root),
                })
            })
            .map_err(map_sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_sql_error)?;
        Ok(rows)
    }

    fn load_collections(&self) -> Result<Vec<Collection>, LibraryError> {
        let mut collections = self.fetch_collections()?;
        let members = self.fetch_collection_members()?;
        for (collection_id, member) in members {
            if let Some(collection) = collections
                .iter_mut()
                .find(|collection| collection.id.as_str() == collection_id)
            {
                collection.members.push(member);
            }
        }
        Ok(collections)
    }

    fn fetch_collections(&self) -> Result<Vec<Collection>, LibraryError> {
        let mut stmt = self
            .connection
            .prepare(
                "SELECT id, name, export_path
                 FROM collections
                 ORDER BY sort_order ASC, id ASC",
            )
            .map_err(map_sql_error)?;
        stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let name: String = row.get(1)?;
            let export_path: Option<String> = row.get(2)?;
            Ok(Collection {
                id: CollectionId::from_string(id),
                name,
                members: Vec::new(),
                export_path: export_path.map(PathBuf::from),
            })
        })
        .map_err(map_sql_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_sql_error)
    }

    fn fetch_collection_members(&self) -> Result<Vec<(String, CollectionMember)>, LibraryError> {
        let mut stmt = self
            .connection
            .prepare(
                "SELECT collection_id, source_id, relative_path, clip_root
                 FROM collection_members
                 ORDER BY sort_order ASC",
            )
            .map_err(map_sql_error)?;
        stmt.query_map([], |row| {
            let collection_id: String = row.get(0)?;
            let source_id: String = row.get(1)?;
            let relative_path: String = row.get(2)?;
            let clip_root: Option<String> = row.get(3)?;
            Ok((
                collection_id,
                CollectionMember {
                    source_id: SourceId::from_string(source_id),
                    relative_path: PathBuf::from(relative_path),
                    clip_root: clip_root.map(PathBuf::from),
                },
            ))
        })
        .map_err(map_sql_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_sql_error)
    }

    fn replace_sources(tx: &Transaction<'_>, sources: &[SampleSource]) -> Result<(), LibraryError> {
        tx.execute("DELETE FROM sources", [])
            .map_err(map_sql_error)?;
        if sources.is_empty() {
            return Ok(());
        }
        let mut stmt = tx
            .prepare("INSERT INTO sources (id, root, sort_order) VALUES (?1, ?2, ?3)")
            .map_err(map_sql_error)?;
        for (idx, source) in sources.iter().enumerate() {
            stmt.execute(params![
                source.id.as_str(),
                source.root.to_string_lossy(),
                idx as i64
            ])
            .map_err(map_sql_error)?;
        }
        Ok(())
    }

    fn replace_collections(
        tx: &Transaction<'_>,
        collections: &[Collection],
    ) -> Result<(), LibraryError> {
        Self::clear_collections(tx)?;
        if collections.is_empty() {
            return Ok(());
        }
        Self::insert_collections(tx, collections)?;
        Self::insert_collection_members(tx, collections)
    }

    fn clear_collections(tx: &Transaction<'_>) -> Result<(), LibraryError> {
        tx.execute("DELETE FROM collection_members", [])
            .map_err(map_sql_error)?;
        tx.execute("DELETE FROM collections", [])
            .map_err(map_sql_error)?;
        Ok(())
    }

    fn insert_collections(
        tx: &Transaction<'_>,
        collections: &[Collection],
    ) -> Result<(), LibraryError> {
        let mut insert_collection = tx
            .prepare(
                "INSERT INTO collections (id, name, export_path, sort_order)
                 VALUES (?1, ?2, ?3, ?4)",
            )
            .map_err(map_sql_error)?;
        for (collection_idx, collection) in collections.iter().enumerate() {
            insert_collection
                .execute(params![
                    collection.id.as_str(),
                    &collection.name,
                    collection.export_path.as_ref().map(|p| p.to_string_lossy()),
                    collection_idx as i64
                ])
                .map_err(map_sql_error)?;
        }
        Ok(())
    }

    fn insert_collection_members(
        tx: &Transaction<'_>,
        collections: &[Collection],
    ) -> Result<(), LibraryError> {
        let mut insert_member = tx
            .prepare(
                "INSERT INTO collection_members (collection_id, source_id, relative_path, clip_root, sort_order)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )
            .map_err(map_sql_error)?;
        for collection in collections {
            for (member_idx, member) in collection.members.iter().enumerate() {
                let clip_root_str = member
                    .clip_root
                    .as_ref()
                    .map(|path| path.to_string_lossy().to_string());
                insert_member
                    .execute(params![
                        collection.id.as_str(),
                        member.source_id.as_str(),
                        member.relative_path.to_string_lossy(),
                        clip_root_str,
                        member_idx as i64
                    ])
                    .map_err(map_sql_error)?;
            }
        }
        Ok(())
    }
}

fn database_path() -> Result<PathBuf, LibraryError> {
    app_dirs::app_root_dir()
        .map_err(map_app_dir_error)
        .map(|dir| dir.join(LIBRARY_DB_FILE_NAME))
}

fn create_parent_if_needed(path: &Path) -> Result<(), LibraryError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| LibraryError::CreateDir {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    Ok(())
}

fn map_sql_error(err: rusqlite::Error) -> LibraryError {
    LibraryError::Sql(err)
}

fn map_app_dir_error(error: app_dirs::AppDirError) -> LibraryError {
    match error {
        app_dirs::AppDirError::NoBaseDir => LibraryError::NoConfigDir,
        app_dirs::AppDirError::CreateDir { path, source } => {
            LibraryError::CreateDir { path, source }
        }
    }
}

#[cfg(test)]
mod tests;
