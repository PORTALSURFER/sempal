use super::*;
use rusqlite::Connection;
use std::collections::HashSet;
use tempfile::tempdir;

pub(super) struct TableContract {
    pub(super) name: &'static str,
    pub(super) columns: &'static [&'static str],
}

pub(super) const SOURCE_DB_SCHEMA_CONTRACT: &[TableContract] = &[
    TableContract {
        name: "metadata",
        columns: &["key", "value"],
    },
    TableContract {
        name: "wav_files",
        columns: &[
            "path",
            "file_size",
            "modified_ns",
            "content_hash",
            "tag",
            "looped",
            "sound_type",
            "locked",
            "missing",
            "extension",
            "last_played_at",
            "last_curated_at",
            "user_tag",
            "tag_named",
            "collection",
        ],
    },
    TableContract {
        name: "source_index_entries",
        columns: &[
            "path",
            "classification",
            "file_size",
            "modified_ns",
            "file_identity",
            "diagnostic",
            "format_policy_version",
        ],
    },
    TableContract {
        name: "source_tags",
        columns: &["id", "normalized_text", "display_label"],
    },
    TableContract {
        name: "wav_file_tags",
        columns: &["path", "tag_id"],
    },
    TableContract {
        name: "wav_file_collections",
        columns: &["path", "collection"],
    },
    TableContract {
        name: "analysis_jobs",
        columns: &[
            "id",
            "sample_id",
            "source_id",
            "relative_path",
            "job_type",
            "content_hash",
            "status",
            "attempts",
            "created_at",
            "running_at",
            "last_error",
            "readiness_managed",
            "readiness_scope_kind",
            "readiness_scope_id",
            "readiness_stage",
            "artifact_version",
            "source_generation",
            "content_generation",
            "retry_at",
            "failure_kind",
            "lease_expires_at",
        ],
    },
    TableContract {
        name: "source_readiness_sources",
        columns: &[
            "source_id",
            "source_generation",
            "readiness_revision",
            "availability",
            "contract_version",
            "membership_digest",
            "membership_count",
            "updated_at",
        ],
    },
    TableContract {
        name: "source_readiness_targets",
        columns: &[
            "source_id",
            "scope_kind",
            "scope_id",
            "relative_path",
            "stage",
            "required_version",
            "source_generation",
            "content_generation",
            "eligibility",
            "updated_at",
        ],
    },
    TableContract {
        name: "source_readiness_artifacts",
        columns: &[
            "source_id",
            "scope_kind",
            "scope_id",
            "stage",
            "artifact_version",
            "source_generation",
            "content_generation",
            "completed_at",
        ],
    },
    TableContract {
        name: "samples",
        columns: &[
            "sample_id",
            "content_hash",
            "size",
            "mtime_ns",
            "duration_seconds",
            "sr_used",
            "analysis_version",
            "bpm",
            "long_sample_mark",
        ],
    },
    TableContract {
        name: "analysis_features",
        columns: &["sample_id", "content_hash", "features"],
    },
    TableContract {
        name: "features",
        columns: &[
            "sample_id",
            "feat_version",
            "vec_blob",
            "light_dsp_blob",
            "rms",
            "computed_at",
        ],
    },
    TableContract {
        name: "analysis_cache_features",
        columns: &[
            "content_hash",
            "analysis_version",
            "feat_version",
            "vec_blob",
            "light_dsp_blob",
            "rms",
            "computed_at",
            "duration_seconds",
            "sr_used",
        ],
    },
    TableContract {
        name: "layout_umap",
        columns: &[
            "sample_id",
            "model_id",
            "umap_version",
            "x",
            "y",
            "created_at",
        ],
    },
    TableContract {
        name: "hdbscan_clusters",
        columns: &[
            "sample_id",
            "model_id",
            "method",
            "umap_version",
            "cluster_id",
            "created_at",
        ],
    },
    TableContract {
        name: "embeddings",
        columns: &[
            "sample_id",
            "model_id",
            "dim",
            "dtype",
            "l2_normed",
            "vec",
            "created_at",
        ],
    },
    TableContract {
        name: "similarity_aspect_descriptors",
        columns: &[
            "sample_id",
            "model_id",
            "dim",
            "dtype",
            "l2_normed",
            "valid_mask",
            "vec",
            "created_at",
        ],
    },
    TableContract {
        name: "analysis_cache_aspect_descriptors",
        columns: &[
            "content_hash",
            "analysis_version",
            "model_id",
            "dim",
            "dtype",
            "l2_normed",
            "valid_mask",
            "vec",
            "created_at",
        ],
    },
    TableContract {
        name: "analysis_cache_embeddings",
        columns: &[
            "content_hash",
            "analysis_version",
            "model_id",
            "dim",
            "dtype",
            "l2_normed",
            "vec",
            "created_at",
        ],
    },
    TableContract {
        name: "ann_index_meta",
        columns: &[
            "model_id",
            "index_path",
            "count",
            "params_json",
            "updated_at",
        ],
    },
    TableContract {
        name: "file_ops_journal",
        columns: &[
            "id",
            "op_type",
            "stage",
            "source_root",
            "source_relative",
            "target_relative",
            "staged_relative",
            "file_size",
            "modified_ns",
            "tag",
            "looped",
            "locked",
            "last_played_at",
            "last_curated_at",
            "created_at",
        ],
    },
    TableContract {
        name: "pending_wav_renames",
        columns: &[
            "path",
            "file_size",
            "modified_ns",
            "content_hash",
            "tag",
            "looped",
            "sound_type",
            "locked",
            "last_played_at",
            "last_curated_at",
            "user_tag",
            "normal_tags",
            "collection",
            "collections",
            "tag_named",
        ],
    },
];

pub(super) fn with_legacy_db(setup_sql: &str) -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let db_file = dir.path().join(DB_FILE_NAME);
    let conn = Connection::open(&db_file).unwrap();
    conn.execute_batch(setup_sql).unwrap();
    dir
}

pub(super) fn column_names(connection: &Connection, table_name: &str) -> Vec<String> {
    let pragma = format!("PRAGMA table_info({table_name})");
    let mut stmt = connection.prepare(&pragma).unwrap();
    stmt.query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

pub(super) fn assert_source_db_schema_contract(connection: &Connection) {
    for table in SOURCE_DB_SCHEMA_CONTRACT {
        let actual = column_names(connection, table.name)
            .into_iter()
            .collect::<HashSet<_>>();
        assert!(
            !actual.is_empty(),
            "expected source DB table `{}` to exist",
            table.name
        );
        for column in table.columns {
            assert!(
                actual.contains(*column),
                "expected source DB table `{}` to contain column `{}`; actual columns: {:?}",
                table.name,
                column,
                actual
            );
        }
    }
}
