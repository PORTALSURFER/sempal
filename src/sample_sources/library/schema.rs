use rusqlite::{OptionalExtension, params};
use std::collections::HashSet;
use std::path::PathBuf;

use super::{LibraryDatabase, LibraryError, map_sql_error};

impl LibraryDatabase {
    pub(super) fn apply_pragmas(&self) -> Result<(), LibraryError> {
        self.connection
            .execute_batch(
                "PRAGMA journal_mode=WAL;
                 PRAGMA synchronous = NORMAL;
                 PRAGMA foreign_keys=ON;
                 PRAGMA busy_timeout=5000;
                 PRAGMA temp_store=MEMORY;
                 PRAGMA cache_size=-64000;
                 PRAGMA mmap_size=268435456;",
            )
            .map_err(map_sql_error)?;
        if let Err(err) = crate::sqlite_ext::try_load_optional_extension(&self.connection) {
            tracing::debug!("SQLite extension not loaded: {err}");
        }
        Ok(())
    }

    pub(super) fn apply_schema(&self) -> Result<(), LibraryError> {
        self.connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS metadata (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                );
                 CREATE TABLE IF NOT EXISTS sources (
                    id TEXT PRIMARY KEY,
                    root TEXT NOT NULL,
                    sort_order INTEGER NOT NULL
                );
                 CREATE TABLE IF NOT EXISTS collections (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    export_path TEXT,
                    sort_order INTEGER NOT NULL
                );
                 CREATE TABLE IF NOT EXISTS collection_members (
                    collection_id TEXT NOT NULL,
                    source_id TEXT NOT NULL,
                    relative_path TEXT NOT NULL,
                    clip_root TEXT,
                    sort_order INTEGER NOT NULL,
                    PRIMARY KEY (collection_id, source_id, relative_path),
                    FOREIGN KEY(collection_id) REFERENCES collections(id) ON DELETE CASCADE
                );
                 CREATE TABLE IF NOT EXISTS analysis_jobs (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    sample_id TEXT NOT NULL,
                    job_type TEXT NOT NULL,
                    content_hash TEXT,
                    status TEXT NOT NULL,
                    attempts INTEGER NOT NULL DEFAULT 0,
                    created_at INTEGER NOT NULL,
                    last_error TEXT,
                    UNIQUE(sample_id, job_type)
                 );
                 CREATE INDEX IF NOT EXISTS idx_analysis_jobs_status_created_id
                    ON analysis_jobs (status, created_at, id);
                 CREATE INDEX IF NOT EXISTS idx_analysis_jobs_status_sample_id
                    ON analysis_jobs (status, sample_id);
                 CREATE TABLE IF NOT EXISTS samples (
                    sample_id TEXT PRIMARY KEY,
                    content_hash TEXT NOT NULL,
                    size INTEGER NOT NULL,
                    mtime_ns INTEGER NOT NULL,
                    duration_seconds REAL,
                    sr_used INTEGER
                );
                 CREATE TABLE IF NOT EXISTS analysis_features (
                    sample_id TEXT PRIMARY KEY,
                    content_hash TEXT NOT NULL,
                    features BLOB
                );
                 CREATE TABLE IF NOT EXISTS analysis_predictions (
                    sample_id TEXT PRIMARY KEY,
                    content_hash TEXT NOT NULL,
                    predictions BLOB
                );
                 CREATE TABLE IF NOT EXISTS features (
                    sample_id TEXT PRIMARY KEY,
                    feat_version INTEGER NOT NULL,
                    vec_blob BLOB NOT NULL,
                    computed_at INTEGER NOT NULL
                ) WITHOUT ROWID;
                 CREATE TABLE IF NOT EXISTS models (
                    model_id TEXT PRIMARY KEY,
                    kind TEXT NOT NULL,
                    model_version INTEGER NOT NULL,
                    feat_version INTEGER NOT NULL,
                    feature_len_f32 INTEGER NOT NULL,
                    classes_json TEXT NOT NULL,
                    model_json TEXT NOT NULL,
                    created_at INTEGER NOT NULL
                ) WITHOUT ROWID;
                 CREATE INDEX IF NOT EXISTS idx_models_created_at ON models (created_at);
                 CREATE TABLE IF NOT EXISTS predictions (
                    sample_id TEXT NOT NULL,
                    model_id TEXT NOT NULL,
                    content_hash TEXT NOT NULL,
                    top_class TEXT NOT NULL,
                    confidence REAL NOT NULL,
                    topk_json TEXT NOT NULL,
                    computed_at INTEGER NOT NULL,
                    PRIMARY KEY (sample_id, model_id),
                    FOREIGN KEY(model_id) REFERENCES models(model_id) ON DELETE CASCADE
                ) WITHOUT ROWID;
                 CREATE INDEX IF NOT EXISTS idx_predictions_model_id ON predictions (model_id);
                 CREATE INDEX IF NOT EXISTS idx_predictions_model_sample_id ON predictions (model_id, sample_id);
                 CREATE INDEX IF NOT EXISTS idx_predictions_top_class ON predictions (top_class);
                 CREATE INDEX IF NOT EXISTS idx_predictions_top_class_confidence ON predictions (top_class, confidence);
                 CREATE INDEX IF NOT EXISTS idx_predictions_model_top_class_confidence ON predictions (model_id, top_class, confidence);
                 CREATE TABLE IF NOT EXISTS labels_weak (
                    sample_id TEXT NOT NULL,
                    ruleset_version INTEGER NOT NULL,
                    class_id TEXT NOT NULL,
                    confidence REAL NOT NULL,
                    rule_id TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    PRIMARY KEY (sample_id, class_id)
                ) WITHOUT ROWID;
                 CREATE INDEX IF NOT EXISTS idx_labels_weak_class_id ON labels_weak (class_id);
                 CREATE TABLE IF NOT EXISTS labels_user (
                    sample_id TEXT PRIMARY KEY,
                    class_id TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL
                 ) WITHOUT ROWID;
                 CREATE INDEX IF NOT EXISTS idx_labels_user_class_id ON labels_user (class_id);",
            )
            .map_err(map_sql_error)?;
        Ok(())
    }

    pub(super) fn migrate_models_table(&mut self) -> Result<(), LibraryError> {
        let mut stmt = self
            .connection
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='models'")
            .map_err(map_sql_error)?;
        let exists: Option<String> = stmt
            .query_row([], |row| row.get(0))
            .optional()
            .map_err(map_sql_error)?;
        drop(stmt);
        if exists.is_some() {
            return Ok(());
        }
        self.connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS models (
                    model_id TEXT PRIMARY KEY,
                    kind TEXT NOT NULL,
                    model_version INTEGER NOT NULL,
                    feat_version INTEGER NOT NULL,
                    feature_len_f32 INTEGER NOT NULL,
                    classes_json TEXT NOT NULL,
                    model_json TEXT NOT NULL,
                    created_at INTEGER NOT NULL
                ) WITHOUT ROWID;
                CREATE INDEX IF NOT EXISTS idx_models_created_at ON models (created_at);",
            )
            .map_err(map_sql_error)?;
        Ok(())
    }

    pub(super) fn migrate_predictions_table(&mut self) -> Result<(), LibraryError> {
        self.connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS predictions (
                    sample_id TEXT NOT NULL,
                    model_id TEXT NOT NULL,
                    content_hash TEXT NOT NULL,
                    top_class TEXT NOT NULL,
                    confidence REAL NOT NULL,
                    topk_json TEXT NOT NULL,
                    computed_at INTEGER NOT NULL,
                    PRIMARY KEY (sample_id, model_id),
                    FOREIGN KEY(model_id) REFERENCES models(model_id) ON DELETE CASCADE
                ) WITHOUT ROWID;
                CREATE INDEX IF NOT EXISTS idx_predictions_model_id ON predictions (model_id);
                CREATE INDEX IF NOT EXISTS idx_predictions_model_sample_id ON predictions (model_id, sample_id);
                CREATE INDEX IF NOT EXISTS idx_predictions_top_class ON predictions (top_class);
                CREATE INDEX IF NOT EXISTS idx_predictions_top_class_confidence ON predictions (top_class, confidence);
                CREATE INDEX IF NOT EXISTS idx_predictions_model_top_class_confidence ON predictions (model_id, top_class, confidence);",
            )
            .map_err(map_sql_error)?;
        Ok(())
    }

    pub(super) fn migrate_analysis_jobs_content_hash(&mut self) -> Result<(), LibraryError> {
        let mut stmt = self
            .connection
            .prepare("PRAGMA table_info(analysis_jobs)")
            .map_err(map_sql_error)?;
        let columns: HashSet<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(map_sql_error)?
            .filter_map(Result::ok)
            .collect();
        drop(stmt);
        if columns.contains("content_hash") {
            return Ok(());
        }
        let tx = self.connection.transaction().map_err(map_sql_error)?;
        tx.execute("ALTER TABLE analysis_jobs ADD COLUMN content_hash TEXT", [])
            .map_err(map_sql_error)?;
        tx.commit().map_err(map_sql_error)?;
        Ok(())
    }

    pub(super) fn migrate_samples_analysis_metadata(&mut self) -> Result<(), LibraryError> {
        let mut stmt = self
            .connection
            .prepare("PRAGMA table_info(samples)")
            .map_err(map_sql_error)?;
        let columns: HashSet<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(map_sql_error)?
            .filter_map(Result::ok)
            .collect();
        drop(stmt);
        if columns.contains("duration_seconds") && columns.contains("sr_used") {
            return Ok(());
        }
        let tx = self.connection.transaction().map_err(map_sql_error)?;
        if !columns.contains("duration_seconds") {
            tx.execute("ALTER TABLE samples ADD COLUMN duration_seconds REAL", [])
                .map_err(map_sql_error)?;
        }
        if !columns.contains("sr_used") {
            tx.execute("ALTER TABLE samples ADD COLUMN sr_used INTEGER", [])
                .map_err(map_sql_error)?;
        }
        tx.commit().map_err(map_sql_error)?;
        Ok(())
    }

    pub(super) fn migrate_features_table(&mut self) -> Result<(), LibraryError> {
        let mut stmt = self
            .connection
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='features'")
            .map_err(map_sql_error)?;
        let exists: Option<String> = stmt
            .query_row([], |row| row.get(0))
            .optional()
            .map_err(map_sql_error)?;
        drop(stmt);
        if exists.is_some() {
            return Ok(());
        }
        self.connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS features (
                    sample_id TEXT PRIMARY KEY,
                    feat_version INTEGER NOT NULL,
                    vec_blob BLOB NOT NULL,
                    computed_at INTEGER NOT NULL
                ) WITHOUT ROWID;",
            )
            .map_err(map_sql_error)?;
        Ok(())
    }

    pub(super) fn migrate_labels_weak_table(&mut self) -> Result<(), LibraryError> {
        let mut stmt = self
            .connection
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='labels_weak'")
            .map_err(map_sql_error)?;
        let exists: Option<String> = stmt
            .query_row([], |row| row.get(0))
            .optional()
            .map_err(map_sql_error)?;
        drop(stmt);
        if exists.is_some() {
            return Ok(());
        }
        self.connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS labels_weak (
                    sample_id TEXT NOT NULL,
                    ruleset_version INTEGER NOT NULL,
                    class_id TEXT NOT NULL,
                    confidence REAL NOT NULL,
                    rule_id TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    PRIMARY KEY (sample_id, class_id)
                ) WITHOUT ROWID;
                CREATE INDEX IF NOT EXISTS idx_labels_weak_class_id ON labels_weak (class_id);",
            )
            .map_err(map_sql_error)?;
        Ok(())
    }

    pub(super) fn migrate_labels_user_table(&mut self) -> Result<(), LibraryError> {
        let mut stmt = self
            .connection
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='labels_user'")
            .map_err(map_sql_error)?;
        let exists: Option<String> = stmt
            .query_row([], |row| row.get(0))
            .optional()
            .map_err(map_sql_error)?;
        drop(stmt);
        if exists.is_some() {
            return Ok(());
        }
        self.connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS labels_user (
                    sample_id TEXT PRIMARY KEY,
                    class_id TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL
                ) WITHOUT ROWID;
                CREATE INDEX IF NOT EXISTS idx_labels_user_class_id ON labels_user (class_id);",
            )
            .map_err(map_sql_error)?;
        Ok(())
    }

    pub(super) fn migrate_collection_member_clip_roots(&mut self) -> Result<(), LibraryError> {
        let current = self.get_metadata(super::COLLECTION_MEMBER_CLIP_ROOT_VERSION_KEY)?;
        if current.as_deref() == Some(super::COLLECTION_MEMBER_CLIP_ROOT_VERSION_V1) {
            return Ok(());
        }
        let tx = self.connection.transaction().map_err(map_sql_error)?;
        let alter_result = tx.execute(
            "ALTER TABLE collection_members ADD COLUMN clip_root TEXT",
            [],
        );
        match alter_result {
            Ok(_) => {}
            Err(err) => {
                let message = err.to_string().to_ascii_lowercase();
                if !message.contains("duplicate column") {
                    return Err(map_sql_error(err));
                }
            }
        }
        tx.commit().map_err(map_sql_error)?;
        self.set_metadata(
            super::COLLECTION_MEMBER_CLIP_ROOT_VERSION_KEY,
            super::COLLECTION_MEMBER_CLIP_ROOT_VERSION_V1,
        )
    }

    pub(super) fn migrate_collection_export_paths(&mut self) -> Result<(), LibraryError> {
        let current = self.get_metadata(super::COLLECTION_EXPORT_PATHS_VERSION_KEY)?;
        if current.as_deref() == Some(super::COLLECTION_EXPORT_PATHS_VERSION_V2) {
            return Ok(());
        }
        self.convert_export_paths_to_final_dirs()?;
        self.set_metadata(
            super::COLLECTION_EXPORT_PATHS_VERSION_KEY,
            super::COLLECTION_EXPORT_PATHS_VERSION_V2,
        )
    }

    fn convert_export_paths_to_final_dirs(&mut self) -> Result<(), LibraryError> {
        let tx = self.connection.transaction().map_err(map_sql_error)?;
        let mut select = tx
            .prepare(
                "SELECT id, name, export_path
                 FROM collections
                 WHERE export_path IS NOT NULL",
            )
            .map_err(map_sql_error)?;
        let updates = select
            .query_map([], |row| {
                let id: String = row.get(0)?;
                let name: String = row.get(1)?;
                let export_path: String = row.get(2)?;
                Ok((id, name, export_path))
            })
            .map_err(map_sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_sql_error)?;
        drop(select);
        if !updates.is_empty() {
            let mut update = tx
                .prepare("UPDATE collections SET export_path = ?1 WHERE id = ?2")
                .map_err(map_sql_error)?;
            for (id, name, export_path) in updates {
                let legacy_root = PathBuf::from(export_path);
                let folder_name = super::collection_folder_name_from_str(&name);
                let new_path = super::normalize_path(legacy_root.join(folder_name).as_path());
                update
                    .execute(params![new_path.to_string_lossy(), id])
                    .map_err(map_sql_error)?;
            }
        }
        tx.commit().map_err(map_sql_error)
    }

    fn get_metadata(&self, key: &str) -> Result<Option<String>, LibraryError> {
        self.connection
            .query_row(
                "SELECT value FROM metadata WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
            .map_err(map_sql_error)
    }

    fn set_metadata(&self, key: &str, value: &str) -> Result<(), LibraryError> {
        self.connection
            .execute(
                "INSERT INTO metadata (key, value)
                 VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map_err(map_sql_error)?;
        Ok(())
    }
}
