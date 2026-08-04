//! Base DDL for source database tables and indices.

use rusqlite::Connection;

use super::super::SourceDbError;
use super::super::util::map_sql_error;

const BASE_SCHEMA_SQL: &str = "CREATE TABLE IF NOT EXISTS metadata (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS source_manifest_audit_state (
        singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
        started_at INTEGER NOT NULL,
        checked_files INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE IF NOT EXISTS source_manifest_audit_seen (
        path TEXT PRIMARY KEY
    ) WITHOUT ROWID;
    CREATE TABLE IF NOT EXISTS source_content_audit_state (
        singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
        rotation_id INTEGER NOT NULL,
        rotation_started_at INTEGER NOT NULL,
        cursor TEXT NOT NULL DEFAULT '',
        retry_cursor TEXT NOT NULL DEFAULT '',
        retry_next INTEGER NOT NULL DEFAULT 0 CHECK(retry_next IN (0, 1)),
        checkpoint_revision INTEGER NOT NULL DEFAULT 0,
        verified_entries INTEGER NOT NULL DEFAULT 0,
        verified_bytes INTEGER NOT NULL DEFAULT 0,
        bytes_read INTEGER NOT NULL DEFAULT 0,
        skipped_entries INTEGER NOT NULL DEFAULT 0,
        last_batch_at INTEGER,
        last_rotation_completed_at INTEGER,
        last_rotation_seconds INTEGER
    );
    CREATE TABLE IF NOT EXISTS source_content_audit_entries (
        path TEXT PRIMARY KEY,
        verified_rotation INTEGER,
        verified_at INTEGER,
        verified_file_size INTEGER,
        verified_modified_ns INTEGER,
        verified_file_identity TEXT,
        last_attempt_at INTEGER,
        retry_at INTEGER,
        skip_reason TEXT,
        attempts INTEGER NOT NULL DEFAULT 0,
        bytes_read INTEGER NOT NULL DEFAULT 0,
        FOREIGN KEY(path) REFERENCES wav_files(path) ON DELETE CASCADE
    ) WITHOUT ROWID;
    CREATE TABLE IF NOT EXISTS wav_files (
        path TEXT PRIMARY KEY,
        file_size INTEGER NOT NULL,
        modified_ns INTEGER NOT NULL,
        tag INTEGER NOT NULL DEFAULT 0,
        looped INTEGER NOT NULL DEFAULT 0,
        locked INTEGER NOT NULL DEFAULT 0,
        sound_type TEXT,
        user_tag TEXT,
        tag_named INTEGER NOT NULL DEFAULT 0,
        missing INTEGER NOT NULL DEFAULT 0,
        extension TEXT NOT NULL DEFAULT '',
        last_played_at INTEGER,
        last_curated_at INTEGER,
        collection INTEGER,
        file_identity TEXT
    );
    CREATE TABLE IF NOT EXISTS source_index_entries (
        path TEXT PRIMARY KEY,
        path_encoding INTEGER NOT NULL DEFAULT 0,
        classification TEXT NOT NULL CHECK(classification IN (
            'unsupported_audio',
            'unsupported_non_audio',
            'inaccessible',
            'practically_unsupported_audio'
        )),
        file_size INTEGER,
        modified_ns INTEGER,
        file_identity TEXT,
        diagnostic TEXT,
        format_policy_version INTEGER NOT NULL
    ) WITHOUT ROWID;
    CREATE TABLE IF NOT EXISTS source_tags (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        normalized_text TEXT NOT NULL UNIQUE,
        display_label TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS wav_file_tags (
        path TEXT NOT NULL,
        tag_id INTEGER NOT NULL,
        PRIMARY KEY (path, tag_id),
        FOREIGN KEY(path) REFERENCES wav_files(path) ON DELETE CASCADE,
        FOREIGN KEY(tag_id) REFERENCES source_tags(id) ON DELETE CASCADE
    ) WITHOUT ROWID;
    CREATE TABLE IF NOT EXISTS wav_file_collections (
        path TEXT NOT NULL,
        collection INTEGER NOT NULL,
        PRIMARY KEY (path, collection),
        FOREIGN KEY(path) REFERENCES wav_files(path) ON DELETE CASCADE
    ) WITHOUT ROWID;
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
        readiness_managed INTEGER NOT NULL DEFAULT 0,
        readiness_claim_generation INTEGER NOT NULL DEFAULT 0,
        readiness_scope_kind TEXT,
        readiness_scope_id TEXT,
        readiness_stage TEXT,
        artifact_version TEXT,
        source_generation INTEGER,
        content_generation TEXT,
        retry_at INTEGER,
        failure_kind TEXT,
        failure_code TEXT,
        lease_expires_at INTEGER,
        UNIQUE(sample_id, job_type)
    );
    CREATE INDEX IF NOT EXISTS idx_analysis_jobs_status_created_id
        ON analysis_jobs (status, created_at, id);
    CREATE INDEX IF NOT EXISTS idx_analysis_jobs_status_sample_id
        ON analysis_jobs (status, sample_id);
    CREATE TABLE IF NOT EXISTS source_readiness_sources (
        source_id TEXT PRIMARY KEY,
        source_generation INTEGER NOT NULL,
        readiness_revision INTEGER NOT NULL,
        availability TEXT NOT NULL,
        contract_version TEXT NOT NULL DEFAULT '',
        membership_digest BLOB NOT NULL DEFAULT X'',
        membership_count INTEGER NOT NULL DEFAULT 0,
        updated_at INTEGER NOT NULL
    ) WITHOUT ROWID;
    CREATE TABLE IF NOT EXISTS source_readiness_targets (
        source_id TEXT NOT NULL,
        scope_kind TEXT NOT NULL,
        scope_id TEXT NOT NULL,
        relative_path TEXT,
        stage TEXT NOT NULL,
        required_version TEXT NOT NULL CHECK(length(trim(required_version)) > 0),
        source_generation INTEGER NOT NULL,
        content_generation TEXT NOT NULL CHECK(length(trim(content_generation)) > 0),
        eligibility TEXT NOT NULL,
        updated_at INTEGER NOT NULL,
        CHECK (
            (stage = 'similarity_layout' AND scope_kind = 'source')
            OR (stage <> 'similarity_layout' AND scope_kind = 'file')
        ),
        CHECK (length(trim(source_id)) > 0 AND length(trim(scope_id)) > 0),
        CHECK (
            (scope_kind = 'source' AND scope_id = source_id AND relative_path IS NULL)
            OR (
                scope_kind = 'file'
                AND (
                    eligibility <> 'eligible'
                    OR (relative_path IS NOT NULL AND length(trim(relative_path)) > 0)
                )
            )
        ),
        PRIMARY KEY (source_id, scope_kind, scope_id, stage)
    ) WITHOUT ROWID;
    CREATE TABLE IF NOT EXISTS source_readiness_artifacts (
        source_id TEXT NOT NULL,
        scope_kind TEXT NOT NULL,
        scope_id TEXT NOT NULL,
        relative_path TEXT,
        stage TEXT NOT NULL,
        artifact_version TEXT NOT NULL CHECK(length(trim(artifact_version)) > 0),
        source_generation INTEGER NOT NULL,
        content_generation TEXT NOT NULL CHECK(length(trim(content_generation)) > 0),
        artifact_ref TEXT,
        completed_at INTEGER NOT NULL,
        CHECK (
            (stage = 'similarity_layout' AND scope_kind = 'source')
            OR (stage <> 'similarity_layout' AND scope_kind = 'file')
        ),
        CHECK (length(trim(source_id)) > 0 AND length(trim(scope_id)) > 0),
        CHECK (scope_kind = 'file' OR (scope_kind = 'source' AND scope_id = source_id)),
        PRIMARY KEY (source_id, scope_kind, scope_id, stage)
    ) WITHOUT ROWID;
    CREATE TABLE IF NOT EXISTS samples (
        sample_id TEXT PRIMARY KEY,
        content_hash TEXT NOT NULL,
        size INTEGER NOT NULL,
        mtime_ns INTEGER NOT NULL,
        duration_seconds REAL,
        sr_used INTEGER,
        analysis_version TEXT,
        bpm REAL,
        long_sample_mark INTEGER
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
        light_dsp_blob BLOB,
        rms REAL,
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
    CREATE TABLE IF NOT EXISTS similarity_aspect_descriptors (
        sample_id TEXT PRIMARY KEY,
        model_id TEXT NOT NULL,
        dim INTEGER NOT NULL,
        dtype TEXT NOT NULL,
        l2_normed INTEGER NOT NULL,
        valid_mask INTEGER NOT NULL,
        vec BLOB NOT NULL,
        created_at INTEGER NOT NULL,
        FOREIGN KEY(sample_id) REFERENCES samples(sample_id) ON DELETE CASCADE
    ) WITHOUT ROWID;
    CREATE INDEX IF NOT EXISTS idx_similarity_aspect_descriptors_model_id
        ON similarity_aspect_descriptors (model_id);
    CREATE TABLE IF NOT EXISTS analysis_cache_features (
        content_hash TEXT PRIMARY KEY,
        analysis_version TEXT NOT NULL,
        feat_version INTEGER NOT NULL,
        vec_blob BLOB NOT NULL,
        light_dsp_blob BLOB,
        rms REAL,
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
    CREATE TABLE IF NOT EXISTS analysis_cache_aspect_descriptors (
        content_hash TEXT NOT NULL,
        analysis_version TEXT NOT NULL,
        model_id TEXT NOT NULL,
        dim INTEGER NOT NULL,
        dtype TEXT NOT NULL,
        l2_normed INTEGER NOT NULL,
        valid_mask INTEGER NOT NULL,
        vec BLOB NOT NULL,
        created_at INTEGER NOT NULL,
        PRIMARY KEY (content_hash, model_id)
    ) WITHOUT ROWID;
    CREATE INDEX IF NOT EXISTS idx_cache_aspect_descriptors_model_id
        ON analysis_cache_aspect_descriptors (model_id);
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
        file_identity TEXT,
        target_root_identity TEXT,
        source_root_identity TEXT,
        tag INTEGER,
        looped INTEGER,
        locked INTEGER,
        last_played_at INTEGER,
        last_curated_at INTEGER,
        created_at INTEGER NOT NULL
    );
    CREATE TABLE IF NOT EXISTS pending_wav_renames (
        path TEXT PRIMARY KEY,
        file_size INTEGER NOT NULL,
        modified_ns INTEGER NOT NULL,
        content_hash TEXT,
        tag INTEGER NOT NULL,
        looped INTEGER NOT NULL,
        sound_type TEXT,
        locked INTEGER NOT NULL,
        last_played_at INTEGER,
        last_curated_at INTEGER,
        user_tag TEXT,
        normal_tags TEXT,
        collection INTEGER,
        collections TEXT,
        tag_named INTEGER NOT NULL DEFAULT 0,
        file_identity TEXT,
        staged_generation INTEGER NOT NULL DEFAULT 0,
        staged_at INTEGER
    );
    CREATE TABLE IF NOT EXISTS pending_wav_rename_destinations (
        path TEXT PRIMARY KEY,
        scan_generation INTEGER NOT NULL,
        retained_hash TEXT
    );
    CREATE TABLE IF NOT EXISTS source_directory_generations (
        generation INTEGER PRIMARY KEY CHECK(generation > 0),
        status TEXT NOT NULL CHECK(status IN ('staging', 'active', 'inactive')),
        expected_entry_count INTEGER NOT NULL CHECK(expected_entry_count >= 0),
        staged_entry_count INTEGER NOT NULL DEFAULT 0
            CHECK(staged_entry_count >= 0 AND staged_entry_count <= expected_entry_count),
        complete INTEGER NOT NULL DEFAULT 0 CHECK(complete IN (0, 1)),
        published_source_revision INTEGER
            CHECK(published_source_revision IS NULL OR published_source_revision > 0),
        created_at INTEGER NOT NULL DEFAULT 0,
        CHECK (
            (status = 'staging' AND published_source_revision IS NULL)
            OR (status IN ('active', 'inactive') AND published_source_revision IS NOT NULL)
        ),
        CHECK(
            status = 'staging'
            OR (complete = 1 AND staged_entry_count = expected_entry_count)
        )
    );
    CREATE TABLE IF NOT EXISTS source_directory_entries (
        generation INTEGER NOT NULL,
        path TEXT NOT NULL CHECK(length(path) > 0),
        path_encoding INTEGER NOT NULL DEFAULT 0 CHECK(path_encoding IN (0, 1)),
        directory_identity TEXT NOT NULL CHECK(length(trim(directory_identity)) > 0),
        PRIMARY KEY (generation, path),
        UNIQUE (generation, directory_identity),
        FOREIGN KEY(generation) REFERENCES source_directory_generations(generation)
            ON DELETE CASCADE
    ) WITHOUT ROWID;";

const INDEX_SQL: &str = "CREATE INDEX IF NOT EXISTS idx_wav_files_missing
         ON wav_files(path) WHERE missing != 0;
     CREATE INDEX IF NOT EXISTS idx_wav_files_extension
         ON wav_files(extension);
     CREATE INDEX IF NOT EXISTS idx_source_index_entries_classification_path
         ON source_index_entries(classification, path);
     CREATE INDEX IF NOT EXISTS idx_source_content_audit_forward_path
         ON wav_files(path)
         WHERE missing = 0
           AND extension IN ('wav')
           AND path NOT GLOB '._*'
           AND path NOT GLOB '*/._*';
     CREATE INDEX IF NOT EXISTS idx_source_content_audit_retry_due
         ON source_content_audit_entries(COALESCE(retry_at, 0), path)
         WHERE skip_reason IS NOT NULL;
     CREATE INDEX IF NOT EXISTS idx_wav_file_tags_tag_id
         ON wav_file_tags(tag_id);
     CREATE INDEX IF NOT EXISTS idx_wav_file_collections_collection
         ON wav_file_collections(collection);
     CREATE INDEX IF NOT EXISTS idx_pending_wav_renames_hash
         ON pending_wav_renames (content_hash);
     CREATE INDEX IF NOT EXISTS idx_pending_wav_renames_file_identity
         ON pending_wav_renames (file_identity);
     CREATE INDEX IF NOT EXISTS idx_pending_wav_renames_identity_facts
         ON pending_wav_renames (file_identity, file_size, modified_ns);
     CREATE INDEX IF NOT EXISTS idx_pending_wav_renames_facts
         ON pending_wav_renames (file_size, modified_ns);
     CREATE INDEX IF NOT EXISTS idx_pending_wav_renames_generation
         ON pending_wav_renames (staged_generation);
     CREATE INDEX IF NOT EXISTS idx_analysis_jobs_source_job_status_created
         ON analysis_jobs (source_id, job_type, status, created_at);
     CREATE INDEX IF NOT EXISTS idx_analysis_jobs_job_relative_path_status
         ON analysis_jobs (job_type, relative_path, status);
     CREATE INDEX IF NOT EXISTS idx_analysis_jobs_job_status
         ON analysis_jobs (job_type, status);
     CREATE INDEX IF NOT EXISTS idx_analysis_jobs_readiness_state
         ON analysis_jobs (
             source_id,
             readiness_managed,
             readiness_stage,
             status,
             retry_at
         );
     CREATE INDEX IF NOT EXISTS idx_source_readiness_targets_generation
         ON source_readiness_targets (source_id, source_generation, stage);
     CREATE INDEX IF NOT EXISTS idx_source_readiness_artifacts_generation
         ON source_readiness_artifacts (source_id, source_generation, stage);
     CREATE INDEX IF NOT EXISTS idx_file_ops_journal_stage
         ON file_ops_journal (stage);
     CREATE UNIQUE INDEX IF NOT EXISTS idx_source_directory_generations_active
         ON source_directory_generations(status)
         WHERE status = 'active';
     CREATE INDEX IF NOT EXISTS idx_source_directory_generations_status_generation
         ON source_directory_generations(status, generation);";

/// Create all base tables used by the source database.
pub(super) fn apply_base_schema(connection: &Connection) -> Result<(), SourceDbError> {
    connection
        .execute_batch(BASE_SCHEMA_SQL)
        .map_err(map_sql_error)?;
    Ok(())
}

/// Create all supporting indices used by the source database.
pub(super) fn apply_indices(connection: &Connection) -> Result<(), SourceDbError> {
    connection.execute_batch(INDEX_SQL).map_err(map_sql_error)?;
    Ok(())
}
