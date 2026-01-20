use rusqlite::Connection;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::SourceDbError;
use super::util::{map_sql_error, parse_relative_path_from_db};

pub(super) fn apply_schema(connection: &Connection) -> Result<(), SourceDbError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS metadata (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
             CREATE TABLE IF NOT EXISTS wav_files (
                path TEXT PRIMARY KEY,
                file_size INTEGER NOT NULL,
                modified_ns INTEGER NOT NULL,
                tag INTEGER NOT NULL DEFAULT 0,
                looped INTEGER NOT NULL DEFAULT 0,
                missing INTEGER NOT NULL DEFAULT 0,
                extension TEXT NOT NULL DEFAULT '',
                last_played_at INTEGER
             );
             CREATE TABLE IF NOT EXISTS analysis_jobs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                sample_id TEXT NOT NULL,
                source_id TEXT NOT NULL DEFAULT '',
                relative_path TEXT NOT NULL DEFAULT '',
                job_type TEXT NOT NULL,
                content_hash TEXT,
                status TEXT NOT NULL,
                attempts INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                running_at INTEGER,
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
                sr_used INTEGER,
                analysis_version TEXT,
                bpm REAL
             );
             CREATE TABLE IF NOT EXISTS analysis_features (
                sample_id TEXT PRIMARY KEY,
                content_hash TEXT NOT NULL,
                features BLOB
             );
             CREATE TABLE IF NOT EXISTS features (
                sample_id TEXT PRIMARY KEY,
                feat_version INTEGER NOT NULL,
                vec_blob BLOB NOT NULL,
                computed_at INTEGER NOT NULL
             ) WITHOUT ROWID;
             CREATE TABLE IF NOT EXISTS layout_umap (
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
                ON layout_umap (x, y);
             CREATE TABLE IF NOT EXISTS hdbscan_clusters (
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
                ON hdbscan_clusters (cluster_id);
             CREATE TABLE IF NOT EXISTS embeddings (
                sample_id TEXT PRIMARY KEY,
                model_id TEXT NOT NULL,
                dim INTEGER NOT NULL,
                dtype TEXT NOT NULL,
                l2_normed INTEGER NOT NULL,
                vec BLOB NOT NULL,
                created_at INTEGER NOT NULL
             ) WITHOUT ROWID;
             CREATE INDEX IF NOT EXISTS idx_embeddings_model_id ON embeddings (model_id);
             CREATE TABLE IF NOT EXISTS analysis_cache_features (
                content_hash TEXT PRIMARY KEY,
                analysis_version TEXT NOT NULL,
                feat_version INTEGER NOT NULL,
                vec_blob BLOB NOT NULL,
                computed_at INTEGER NOT NULL,
                duration_seconds REAL NOT NULL,
                sr_used INTEGER NOT NULL
             ) WITHOUT ROWID;
             CREATE TABLE IF NOT EXISTS analysis_cache_embeddings (
                content_hash TEXT NOT NULL,
                analysis_version TEXT NOT NULL,
                model_id TEXT NOT NULL,
                dim INTEGER NOT NULL,
                dtype TEXT NOT NULL,
                l2_normed INTEGER NOT NULL,
                vec BLOB NOT NULL,
                created_at INTEGER NOT NULL,
                PRIMARY KEY (content_hash, model_id)
             ) WITHOUT ROWID;
             CREATE INDEX IF NOT EXISTS idx_cache_embeddings_model_id
                ON analysis_cache_embeddings (model_id);
             CREATE TABLE IF NOT EXISTS ann_index_meta (
                model_id TEXT PRIMARY KEY,
                index_path TEXT NOT NULL,
                count INTEGER NOT NULL,
                params_json TEXT NOT NULL,
                updated_at INTEGER NOT NULL
             ) WITHOUT ROWID;
             CREATE TABLE IF NOT EXISTS file_ops_journal (
                id TEXT PRIMARY KEY,
                op_type TEXT NOT NULL,
                stage TEXT NOT NULL,
                source_root TEXT,
                source_relative TEXT,
                target_relative TEXT NOT NULL,
                staged_relative TEXT,
                file_size INTEGER,
                modified_ns INTEGER,
                tag INTEGER,
                looped INTEGER,
                last_played_at INTEGER,
                created_at INTEGER NOT NULL
             );",
        )
        .map_err(map_sql_error)?;
    ensure_optional_columns(connection)?;
    remove_invalid_relative_paths(connection)?;
    connection
        .execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_wav_files_missing
                 ON wav_files(path) WHERE missing != 0;
             CREATE INDEX IF NOT EXISTS idx_wav_files_extension
                 ON wav_files(extension);
             CREATE INDEX IF NOT EXISTS idx_analysis_jobs_source_job_status_created
                 ON analysis_jobs (source_id, job_type, status, created_at);
             CREATE INDEX IF NOT EXISTS idx_analysis_jobs_job_status
                 ON analysis_jobs (job_type, status);
             CREATE INDEX IF NOT EXISTS idx_file_ops_journal_stage
                 ON file_ops_journal (stage);",
        )
        .map_err(map_sql_error)?;
    Ok(())
}

fn remove_invalid_relative_paths(connection: &Connection) -> Result<(), SourceDbError> {
    let mut stmt = connection
        .prepare("SELECT path FROM wav_files")
        .map_err(map_sql_error)?;
    let paths = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(map_sql_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_sql_error)?;
    let invalid_paths: Vec<String> = paths
        .into_iter()
        .filter(|path| parse_relative_path_from_db(path).is_err())
        .collect();
    if invalid_paths.is_empty() {
        return Ok(());
    }
    let mut delete_stmt = connection
        .prepare("DELETE FROM wav_files WHERE path = ?1")
        .map_err(map_sql_error)?;
    for path in invalid_paths {
        tracing::warn!("Removing wav row with invalid relative path: {path}");
        delete_stmt.execute([path]).map_err(map_sql_error)?;
    }
    Ok(())
}

fn ensure_optional_columns(connection: &Connection) -> Result<(), SourceDbError> {
    ensure_wav_files_optional_columns(connection)?;
    ensure_analysis_jobs_optional_columns(connection)?;
    ensure_samples_optional_columns(connection)?;
    Ok(())
}

fn ensure_wav_files_optional_columns(connection: &Connection) -> Result<(), SourceDbError> {
    let mut stmt = connection
        .prepare("PRAGMA table_info(wav_files)")
        .map_err(map_sql_error)?;
    let columns: std::collections::HashSet<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(map_sql_error)?
        .filter_map(Result::ok)
        .collect();
    if !columns.contains("tag") {
        connection
            .execute(
                "ALTER TABLE wav_files ADD COLUMN tag INTEGER NOT NULL DEFAULT 0",
                [],
            )
            .map_err(map_sql_error)?;
    }
    if !columns.contains("missing") {
        connection
            .execute(
                "ALTER TABLE wav_files ADD COLUMN missing INTEGER NOT NULL DEFAULT 0",
                [],
            )
            .map_err(map_sql_error)?;
    }
    if !columns.contains("looped") {
        connection
            .execute(
                "ALTER TABLE wav_files ADD COLUMN looped INTEGER NOT NULL DEFAULT 0",
                [],
            )
            .map_err(map_sql_error)?;
    }
    if !columns.contains("content_hash") {
        connection
            .execute("ALTER TABLE wav_files ADD COLUMN content_hash TEXT", [])
            .map_err(map_sql_error)?;
    }
    if !columns.contains("extension") {
        connection
            .execute(
                "ALTER TABLE wav_files ADD COLUMN extension TEXT NOT NULL DEFAULT ''",
                [],
            )
            .map_err(map_sql_error)?;
        connection
            .execute(
                "UPDATE wav_files
                 SET extension = lower(
                    substr(path, instr(path, '.') + 1)
                 )
                 WHERE extension = '' AND instr(path, '.') > 0",
                [],
            )
            .map_err(map_sql_error)?;
    }
    if !columns.contains("last_played_at") {
        connection
            .execute("ALTER TABLE wav_files ADD COLUMN last_played_at INTEGER", [])
            .map_err(map_sql_error)?;
    }
    Ok(())
}

fn ensure_analysis_jobs_optional_columns(connection: &Connection) -> Result<(), SourceDbError> {
    let mut stmt = connection
        .prepare("PRAGMA table_info(analysis_jobs)")
        .map_err(map_sql_error)?;
    let columns: std::collections::HashSet<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(map_sql_error)?
        .filter_map(Result::ok)
        .collect();
    if !columns.contains("running_at") {
        connection
            .execute(
                "ALTER TABLE analysis_jobs ADD COLUMN running_at INTEGER",
                [],
            )
            .map_err(map_sql_error)?;
        let now = now_epoch_seconds();
        connection
            .execute(
                "UPDATE analysis_jobs SET running_at = ?1 WHERE status = 'running'",
                [now],
            )
            .map_err(map_sql_error)?;
    }
    if !columns.contains("source_id") {
        connection
            .execute(
                "ALTER TABLE analysis_jobs ADD COLUMN source_id TEXT NOT NULL DEFAULT ''",
                [],
            )
            .map_err(map_sql_error)?;
        connection
            .execute(
                "UPDATE analysis_jobs
                 SET source_id = CASE
                     WHEN instr(sample_id, '::') > 0
                     THEN substr(sample_id, 1, instr(sample_id, '::') - 1)
                     ELSE source_id
                 END
                 WHERE source_id = '' OR source_id IS NULL",
                [],
            )
            .map_err(map_sql_error)?;
    }
    if !columns.contains("relative_path") {
        connection
            .execute(
                "ALTER TABLE analysis_jobs ADD COLUMN relative_path TEXT NOT NULL DEFAULT ''",
                [],
            )
            .map_err(map_sql_error)?;
        connection
            .execute(
                "UPDATE analysis_jobs
                 SET relative_path = CASE
                     WHEN instr(sample_id, '::') > 0
                     THEN substr(sample_id, instr(sample_id, '::') + 2)
                     ELSE relative_path
                 END
                 WHERE relative_path = '' OR relative_path IS NULL",
                [],
            )
            .map_err(map_sql_error)?;
    }
    Ok(())
}

fn ensure_samples_optional_columns(connection: &Connection) -> Result<(), SourceDbError> {
    let mut stmt = connection
        .prepare("PRAGMA table_info(samples)")
        .map_err(map_sql_error)?;
    let columns: std::collections::HashSet<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(map_sql_error)?
        .filter_map(Result::ok)
        .collect();
    if !columns.contains("duration_seconds") {
        connection
            .execute("ALTER TABLE samples ADD COLUMN duration_seconds REAL", [])
            .map_err(map_sql_error)?;
    }
    if !columns.contains("sr_used") {
        connection
            .execute("ALTER TABLE samples ADD COLUMN sr_used INTEGER", [])
            .map_err(map_sql_error)?;
    }
    if !columns.contains("analysis_version") {
        connection
            .execute("ALTER TABLE samples ADD COLUMN analysis_version TEXT", [])
            .map_err(map_sql_error)?;
    }
    if !columns.contains("bpm") {
        connection
            .execute("ALTER TABLE samples ADD COLUMN bpm REAL", [])
            .map_err(map_sql_error)?;
    }
    Ok(())
}

fn now_epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs() as i64
}
