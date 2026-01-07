use rusqlite::params;
use std::path::PathBuf;

use super::{LibraryDatabase, LibraryError, map_sql_error};

impl LibraryDatabase {
    pub(super) fn migrate_analysis_jobs_content_hash(&mut self) -> Result<(), LibraryError> {
        let columns = self.table_columns("analysis_jobs")?;
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
        let columns = self.table_columns("samples")?;
        if columns.contains("duration_seconds")
            && columns.contains("sr_used")
            && columns.contains("analysis_version")
        {
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
        if !columns.contains("analysis_version") {
            tx.execute("ALTER TABLE samples ADD COLUMN analysis_version TEXT", [])
                .map_err(map_sql_error)?;
        }
        tx.commit().map_err(map_sql_error)?;
        Ok(())
    }

    pub(super) fn migrate_features_table(&mut self) -> Result<(), LibraryError> {
        if self.table_exists("features")? {
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

    pub(super) fn migrate_layout_umap_table(&mut self) -> Result<(), LibraryError> {
        if self.table_exists("layout_umap")? {
            return Ok(());
        }
        self.connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS layout_umap (
                    sample_id TEXT PRIMARY KEY,
                    model_id TEXT NOT NULL,
                    umap_version TEXT NOT NULL,
                    x REAL NOT NULL,
                    y REAL NOT NULL,
                    created_at INTEGER NOT NULL,
                    FOREIGN KEY(sample_id) REFERENCES samples(sample_id) ON DELETE CASCADE
                ) WITHOUT ROWID;
                CREATE INDEX IF NOT EXISTS idx_layout_umap_model_version
                    ON layout_umap (model_id, umap_version);
                CREATE INDEX IF NOT EXISTS idx_layout_umap_xy
                    ON layout_umap (x, y);",
            )
            .map_err(map_sql_error)?;
        Ok(())
    }

    pub(super) fn migrate_hdbscan_clusters_table(&mut self) -> Result<(), LibraryError> {
        if self.table_exists("hdbscan_clusters")? {
            return Ok(());
        }
        self.connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS hdbscan_clusters (
                    sample_id TEXT NOT NULL,
                    model_id TEXT NOT NULL,
                    method TEXT NOT NULL,
                    umap_version TEXT NOT NULL,
                    cluster_id INTEGER NOT NULL,
                    created_at INTEGER NOT NULL,
                    PRIMARY KEY (sample_id, model_id, method, umap_version),
                    FOREIGN KEY(sample_id) REFERENCES samples(sample_id) ON DELETE CASCADE
                ) WITHOUT ROWID;
                CREATE INDEX IF NOT EXISTS idx_hdbscan_clusters_set
                    ON hdbscan_clusters (model_id, method, umap_version);
                CREATE INDEX IF NOT EXISTS idx_hdbscan_clusters_cluster_id
                    ON hdbscan_clusters (cluster_id);",
            )
            .map_err(map_sql_error)?;
        Ok(())
    }

    pub(super) fn migrate_embeddings_table(&mut self) -> Result<(), LibraryError> {
        if !self.table_exists("embeddings")? {
            self.connection
                .execute_batch(
                    "CREATE TABLE IF NOT EXISTS embeddings (
                        sample_id TEXT PRIMARY KEY,
                        model_id TEXT NOT NULL,
                        dim INTEGER NOT NULL,
                        dtype TEXT NOT NULL,
                        l2_normed INTEGER NOT NULL,
                        vec BLOB NOT NULL
                    ) WITHOUT ROWID;
                    CREATE INDEX IF NOT EXISTS idx_embeddings_model_id ON embeddings (model_id);",
                )
                .map_err(map_sql_error)?;
            return Ok(());
        }

        let columns = self.table_columns("embeddings")?;
        let has_vec = columns.contains("vec");
        let has_l2 = columns.contains("l2_normed");
        let has_dtype = columns.contains("dtype");
        let has_vec_blob = columns.contains("vec_blob");
        let has_created_at = columns.contains("created_at");
        if has_vec && has_l2 && has_dtype && !has_vec_blob && has_created_at {
            return Ok(());
        }

        if has_vec && has_l2 && has_dtype && !has_vec_blob && !has_created_at {
            self.connection
                .execute(
                    "ALTER TABLE embeddings ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0",
                    [],
                )
                .map_err(map_sql_error)?;
            return Ok(());
        }

        self.connection
            .execute_batch(
                "BEGIN;
                 CREATE TABLE IF NOT EXISTS embeddings_new (
                    sample_id TEXT PRIMARY KEY,
                    model_id TEXT NOT NULL,
                    dim INTEGER NOT NULL,
                    dtype TEXT NOT NULL,
                    l2_normed INTEGER NOT NULL,
                    vec BLOB NOT NULL,
                    created_at INTEGER NOT NULL
                 ) WITHOUT ROWID;
                 INSERT INTO embeddings_new (sample_id, model_id, dim, dtype, l2_normed, vec, created_at)
                    SELECT sample_id, model_id, dim, 'f32', 1, vec_blob, 0
                    FROM embeddings;
                 DROP TABLE embeddings;
                 ALTER TABLE embeddings_new RENAME TO embeddings;
                 CREATE INDEX IF NOT EXISTS idx_embeddings_model_id ON embeddings (model_id);
                 COMMIT;",
            )
            .map_err(map_sql_error)?;
        Ok(())
    }

    pub(super) fn migrate_ann_index_meta_table(&mut self) -> Result<(), LibraryError> {
        if self.table_exists("ann_index_meta")? {
            return Ok(());
        }
        self.connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS ann_index_meta (
                    model_id TEXT PRIMARY KEY,
                    index_path TEXT NOT NULL,
                    count INTEGER NOT NULL,
                    params_json TEXT NOT NULL,
                    updated_at INTEGER NOT NULL
                ) WITHOUT ROWID;",
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

    pub(super) fn migrate_collection_hotkeys(&mut self) -> Result<(), LibraryError> {
        let current = self.get_metadata(super::COLLECTION_HOTKEY_VERSION_KEY)?;
        if current.as_deref() == Some(super::COLLECTION_HOTKEY_VERSION_V1) {
            return Ok(());
        }
        let tx = self.connection.transaction().map_err(map_sql_error)?;
        let alter_result = tx.execute("ALTER TABLE collections ADD COLUMN hotkey INTEGER", []);
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
            super::COLLECTION_HOTKEY_VERSION_KEY,
            super::COLLECTION_HOTKEY_VERSION_V1,
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
}
