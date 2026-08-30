use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

use super::error::map_app_dir_error;
use super::{LIBRARY_DB_FILE_NAME, LibraryError};
use crate::app_dirs::{self, WritableProfileGuard};
#[cfg(test)]
use crate::filesystem_identity::stable_filesystem_identity;
use crate::sample_sources::db::BoundDatabaseRoot;

#[cfg(test)]
use std::collections::HashMap;

#[cfg(test)]
use std::sync::{Arc, Barrier, LazyLock, Mutex};

const LIBRARY_DB_WAL_FILE_NAME: &str = "library.db-wal";
const LIBRARY_DB_SHM_FILE_NAME: &str = "library.db-shm";

pub(super) struct LibraryDatabase {
    pub(super) connection: Connection,
    _database_root_binding: Option<BoundDatabaseRoot>,
}

impl LibraryDatabase {
    pub(super) fn open() -> Result<Self, LibraryError> {
        let db_path = database_path()?;
        Self::open_at(&db_path)
    }

    pub(super) fn open_for_profile_guard(
        profile_guard: &WritableProfileGuard,
        #[cfg(test)] binding_open_gate: Option<(Arc<Barrier>, Arc<Barrier>)>,
    ) -> Result<Self, LibraryError> {
        profile_guard.validate_current()?;
        let binding = BoundDatabaseRoot::for_profile_guard(profile_guard)?;
        binding.validate_profile_guard(profile_guard)?;
        validate_database_entries(binding.path())?;
        #[cfg(test)]
        if let Some((ready, release)) = binding_open_gate {
            ready.wait();
            release.wait();
        }
        let db_path = binding.path().join(LIBRARY_DB_FILE_NAME);
        let connection = Connection::open_with_flags(
            &db_path,
            OpenFlags::default() | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        #[cfg(test)]
        record_test_open(&db_path);
        let database = Self::initialize(connection, Some(binding))?;
        database
            ._database_root_binding
            .as_ref()
            .expect("profile-owned library database retains its root binding")
            .validate_profile_guard(profile_guard)?;
        Ok(database)
    }

    fn open_at(db_path: &Path) -> Result<Self, LibraryError> {
        create_parent_if_needed(db_path)?;
        let connection = Connection::open(db_path)?;
        #[cfg(test)]
        record_test_open(db_path);
        Self::initialize(connection, None)
    }

    fn initialize(
        connection: Connection,
        database_root_binding: Option<BoundDatabaseRoot>,
    ) -> Result<Self, LibraryError> {
        let mut db = Self {
            connection,
            _database_root_binding: database_root_binding,
        };
        db.apply_pragmas()?;
        db.apply_schema()?;
        db.migrate_source_roles()?;
        db.migrate_analysis_jobs_content_hash()?;
        db.migrate_samples_analysis_metadata()?;
        db.migrate_features_table()?;
        db.migrate_layout_umap_table()?;
        db.migrate_hdbscan_clusters_table()?;
        db.migrate_embeddings_table()?;
        db.migrate_ann_index_meta_table()?;
        db.migrate_harvest_tables()?;
        Ok(db)
    }

    pub(super) fn validate_profile_guard(
        &self,
        profile_guard: &WritableProfileGuard,
    ) -> Result<(), crate::app_dirs::ProfileOwnershipError> {
        if let Some(binding) = self._database_root_binding.as_ref() {
            binding.validate_profile_guard(profile_guard)
        } else {
            profile_guard.validate_current()
        }
    }

    pub(super) fn into_connection(self) -> Connection {
        self.connection
    }
}

fn validate_database_entries(database_root: &Path) -> Result<(), LibraryError> {
    for filename in [
        LIBRARY_DB_FILE_NAME,
        LIBRARY_DB_WAL_FILE_NAME,
        LIBRARY_DB_SHM_FILE_NAME,
    ] {
        let path = database_root.join(filename);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(LibraryError::DatabasePathValidation {
                    path,
                    reason: error.to_string(),
                });
            }
        };
        if !is_regular_database_entry(&metadata) {
            return Err(LibraryError::DatabasePathValidation {
                path,
                reason: String::from("entry is not a regular file"),
            });
        }
    }
    Ok(())
}

fn is_regular_database_entry(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

        return metadata.is_file()
            && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 == 0;
    }
    #[cfg(not(windows))]
    {
        metadata.is_file() && !metadata.file_type().is_symlink()
    }
}

#[cfg(test)]
static TEST_OPEN_COUNTS: LazyLock<Mutex<HashMap<PathBuf, usize>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[cfg(test)]
fn record_test_open(path: &Path) {
    let key = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut counts = TEST_OPEN_COUNTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *counts.entry(key).or_default() += 1;
}

#[cfg(test)]
pub(super) fn test_open_count(path: &Path) -> usize {
    let key = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let counts = TEST_OPEN_COUNTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(count) = counts.get(&key).or_else(|| counts.get(path)).copied() {
        return count;
    }

    let Some(expected_name) = path.file_name() else {
        return 0;
    };
    let Some(expected_parent_identity) = parent_identity(path) else {
        return 0;
    };
    counts
        .iter()
        .filter(|(opened_path, _)| {
            opened_path.file_name() == Some(expected_name)
                && parent_identity(opened_path).as_deref()
                    == Some(expected_parent_identity.as_str())
        })
        .map(|(_, count)| *count)
        .sum()
}

#[cfg(test)]
fn parent_identity(path: &Path) -> Option<String> {
    let parent = path.parent()?;
    let metadata = fs::metadata(parent).ok()?;
    stable_filesystem_identity(parent, &metadata)
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
