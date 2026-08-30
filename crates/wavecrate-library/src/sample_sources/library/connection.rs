use std::path::{Path, PathBuf};

use rusqlite::Connection;

use super::error::map_app_dir_error;
use super::{LIBRARY_DB_FILE_NAME, LibraryError};
use crate::app_dirs::{self, WritableProfileGuard};

#[cfg(test)]
use std::collections::HashMap;

#[cfg(test)]
use std::sync::{LazyLock, Mutex};

pub(super) struct LibraryDatabase {
    pub(super) connection: Connection,
}

impl LibraryDatabase {
    pub(super) fn open() -> Result<Self, LibraryError> {
        let db_path = database_path()?;
        Self::open_at(&db_path)
    }

    pub(super) fn open_for_profile_guard(
        profile_guard: &WritableProfileGuard,
    ) -> Result<Self, LibraryError> {
        profile_guard.validate_current()?;
        let db_path = profile_guard.profile_root().join(LIBRARY_DB_FILE_NAME);
        let database = Self::open_at(&db_path)?;
        profile_guard.validate_current()?;
        Ok(database)
    }

    fn open_at(db_path: &Path) -> Result<Self, LibraryError> {
        create_parent_if_needed(db_path)?;
        let connection = Connection::open(db_path)?;
        #[cfg(test)]
        record_test_open(db_path);
        let mut db = Self { connection };
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

    pub(super) fn into_connection(self) -> Connection {
        self.connection
    }
}

#[cfg(test)]
static TEST_OPEN_COUNTS: LazyLock<Mutex<HashMap<PathBuf, usize>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[cfg(test)]
fn record_test_open(path: &Path) {
    let mut counts = TEST_OPEN_COUNTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *counts.entry(path.to_path_buf()).or_default() += 1;
}

#[cfg(test)]
pub(super) fn test_open_count(path: &Path) -> usize {
    TEST_OPEN_COUNTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(path)
        .copied()
        .unwrap_or_default()
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
