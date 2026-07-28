use super::*;

type ReopenedDerivationRow = (
    String,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<String>,
    Option<i64>,
    String,
    Option<String>,
    String,
);
use crate::sample_sources::{SourceId, SourceMetadataStorage, SourceRole};
use rusqlite::{Connection, OptionalExtension};
use std::collections::HashSet;
use std::path::PathBuf;
use tempfile::tempdir;

struct TableContract {
    name: &'static str,
    columns: &'static [&'static str],
}

const LIBRARY_DB_SCHEMA_CONTRACT: &[TableContract] = &[
    TableContract {
        name: "metadata",
        columns: &["key", "value"],
    },
    TableContract {
        name: "sources",
        columns: &[
            "id",
            "root",
            "sort_order",
            "role",
            "metadata_storage",
            "primary_import_folder",
        ],
    },
    TableContract {
        name: "analysis_jobs",
        columns: &[
            "id",
            "sample_id",
            "job_type",
            "content_hash",
            "status",
            "attempts",
            "created_at",
            "last_error",
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
        name: "harvest_files",
        columns: &[
            "source_id",
            "relative_path",
            "file_size",
            "modified_ns",
            "content_hash",
            "harvest_state",
            "discovered_at",
            "seen_at",
            "touched_at",
            "done_at",
            "ignored_at",
            "note",
        ],
    },
    TableContract {
        name: "harvest_derivations",
        columns: &[
            "id",
            "parent_source_id",
            "parent_relative_path",
            "parent_file_size",
            "parent_modified_ns",
            "parent_content_hash",
            "child_source_id",
            "child_relative_path",
            "child_file_size",
            "child_modified_ns",
            "child_content_hash",
            "operation",
            "source_range_start",
            "source_range_end",
            "output_duration_seconds",
            "destination_folder",
            "inherited_rating",
            "inherited_tags_json",
            "inherited_playback_type",
            "tool_version",
            "created_at",
        ],
    },
];

fn with_config_home<T>(dir: &Path, f: impl FnOnce() -> T) -> T {
    let _guard = crate::app_dirs::ConfigBaseGuard::set(dir.to_path_buf());
    f()
}

#[test]
fn library_database_satisfies_schema_contract() {
    let temp = tempdir().unwrap();
    with_config_home(temp.path(), || {
        let conn = open_connection().unwrap();
        assert_library_db_schema_contract(&conn);
    });
}

#[test]
fn recovers_from_library_lock_poisoning() {
    let temp = tempdir().unwrap();
    with_config_home(temp.path(), || {
        let result = std::panic::catch_unwind(|| {
            let _guard = LIBRARY_LOCK.lock().unwrap();
            panic!("poison library lock");
        });
        assert!(result.is_err());

        let state = LibraryState {
            sources: Vec::new(),
        };
        save(&state).unwrap();
        let loaded = load().unwrap();
        assert!(loaded.sources.is_empty());
    });
}

#[test]
fn source_roles_and_metadata_storage_roundtrip_through_library_state() {
    let temp = tempdir().unwrap();
    with_config_home(temp.path(), || {
        let protected = SampleSource::new(temp.path().join("ableton-project")).protected();
        let primary = SampleSource::new(temp.path().join("editable-library")).primary();
        save(&LibraryState {
            sources: vec![protected.clone(), primary.clone()],
        })
        .unwrap();

        let loaded = load().unwrap();
        assert_eq!(loaded.sources.len(), 2);
        assert_eq!(loaded.sources[0].role, SourceRole::Protected);
        assert_eq!(
            loaded.sources[0].metadata_storage,
            SourceMetadataStorage::AppData
        );
        assert_eq!(loaded.sources[1].role, SourceRole::Primary);
        assert_eq!(
            loaded.sources[1].metadata_storage,
            SourceMetadataStorage::SourceFolder
        );
    });
}

#[test]
fn protected_source_database_root_is_outside_source_folder() {
    let temp = tempdir().unwrap();
    with_config_home(temp.path(), || {
        let source_root = temp.path().join("protected-audio");
        let protected = SampleSource::new(source_root.clone()).protected();
        let database_root = protected.database_root().unwrap();

        assert!(!database_root.starts_with(&source_root));
        assert!(database_root.ends_with(protected.id.as_str()));
    });
}

#[test]
fn harvest_state_auto_transitions_only_move_forward() {
    let temp = tempdir().unwrap();
    with_config_home(temp.path(), || {
        let identity = harvest_identity("source-a", "drums/kick.wav");

        let discovered = upsert_harvest_file(&identity).unwrap();
        assert_eq!(discovered.state, HarvestState::New);

        let seen = mark_harvest_seen(&identity).unwrap();
        assert_eq!(seen.state, HarvestState::Seen);
        assert!(seen.seen_at.is_some());

        let touched = mark_harvest_touched(&identity).unwrap();
        assert_eq!(touched.state, HarvestState::Touched);
        assert!(touched.touched_at.is_some());

        let still_touched = mark_harvest_seen(&identity).unwrap();
        assert_eq!(still_touched.state, HarvestState::Touched);

        let done = set_harvest_state(&identity.key, HarvestState::Done).unwrap();
        assert_eq!(done.state, HarvestState::Done);
        assert!(done.done_at.is_some());

        let still_done = mark_harvest_touched(&identity).unwrap();
        assert_eq!(still_done.state, HarvestState::Done);
    });
}

#[test]
fn current_harvest_touch_fence_rechecks_before_state_write() {
    let temp = tempdir().unwrap();
    with_config_home(temp.path(), || {
        let identity = harvest_identity("source-fence", "drums/kick.wav");
        upsert_harvest_file(&identity).unwrap();
        let checks = std::sync::atomic::AtomicUsize::new(0);
        let result = mark_harvest_touched_if_current(&identity, || {
            checks.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == 0
        })
        .unwrap();
        assert!(result.is_none());
        assert_eq!(checks.load(std::sync::atomic::Ordering::Relaxed), 2);
        assert_eq!(
            harvest_file(&identity.key).unwrap().unwrap().state,
            HarvestState::New
        );
    });
}

#[test]
fn harvest_file_batch_upsert_persists_every_identity() {
    let temp = tempdir().unwrap();
    with_config_home(temp.path(), || {
        let first = harvest_identity("source-a", "drums/kick.wav");
        let second = harvest_identity("source-a", "drums/snare.wav");

        upsert_harvest_files(&[first.clone(), second.clone()]).unwrap();

        assert_eq!(
            harvest_file(&first.key).unwrap().unwrap().file_size,
            first.file_size
        );
        assert_eq!(
            harvest_file(&second.key).unwrap().unwrap().content_hash,
            second.content_hash
        );
    });
}

#[test]
fn manual_harvest_reset_returns_file_to_new_queue() {
    let temp = tempdir().unwrap();
    with_config_home(temp.path(), || {
        let identity = harvest_identity("source-a", "loops/top.wav");

        mark_harvest_touched(&identity).unwrap();
        set_harvest_state(&identity.key, HarvestState::Ignored).unwrap();
        let reset = set_harvest_state(&identity.key, HarvestState::New).unwrap();

        assert_eq!(reset.state, HarvestState::New);
        assert!(reset.seen_at.is_none());
        assert!(reset.touched_at.is_none());
        assert!(reset.done_at.is_none());
        assert!(reset.ignored_at.is_none());
    });
}

#[test]
fn manual_harvest_state_update_preserves_known_identity_metadata() {
    let temp = tempdir().unwrap();
    with_config_home(temp.path(), || {
        let identity = harvest_identity("source-a", "loops/keep.wav");

        upsert_harvest_file(&identity).unwrap();
        let done = set_harvest_state(&identity.key, HarvestState::Done).unwrap();

        assert_eq!(done.state, HarvestState::Done);
        assert_eq!(done.file_size, identity.file_size);
        assert_eq!(done.modified_ns, identity.modified_ns);
        assert_eq!(done.content_hash, identity.content_hash);
    });
}

#[test]
fn harvest_derivation_graph_survives_reopen_and_marks_parent_touched() {
    let temp = tempdir().unwrap();
    with_config_home(temp.path(), || {
        let parent = harvest_identity("source-a", "jam.wav");
        let child = harvest_identity("source-b", "_Harvests/source-a/jam - chop 01.wav");
        let edge = NewHarvestDerivation {
            parent: parent.clone(),
            child: child.clone(),
            operation: HarvestDerivationOperation::Extract,
            source_range: Some(HarvestSourceRange {
                start_seconds: 1.25,
                end_seconds: 2.5,
            }),
            output_duration_seconds: Some(1.25),
            destination_folder: Some(PathBuf::from("_Harvests/source-a")),
            inherited_metadata: HarvestMetadataSnapshot {
                rating: Some(1),
                tags: vec!["drum".to_string(), "keep".to_string()],
                playback_type: Some("one-shot".to_string()),
            },
            tool_version: "test".to_string(),
        };

        let edge_id = record_harvest_derivation(&edge).unwrap();
        assert!(edge_id > 0);

        let parent_record = harvest_file(&parent.key).unwrap().unwrap();
        assert_eq!(parent_record.state, HarvestState::Touched);
        assert_eq!(harvest_derivative_count(&parent.key).unwrap(), 1);

        let parent_edges = harvest_derivations_for_parent(&parent.key).unwrap();
        assert_eq!(parent_edges.len(), 1);
        assert_eq!(parent_edges[0].child.key, child.key);
        assert_eq!(
            parent_edges[0].operation,
            HarvestDerivationOperation::Extract
        );
        assert_eq!(
            parent_edges[0].inherited_metadata.tags,
            vec!["drum".to_string(), "keep".to_string()]
        );

        let child_edges = harvest_parents_for_child(&child.key).unwrap();
        assert_eq!(child_edges.len(), 1);
        assert_eq!(child_edges[0].parent.key, parent.key);

        let reopened = open_connection().unwrap();
        assert_library_db_schema_contract(&reopened);
        let reopened_parent: (String, Option<String>, Option<i64>) = reopened
            .query_row(
                "SELECT harvest_state, content_hash, touched_at
                 FROM harvest_files
                 WHERE source_id = ?1 AND relative_path = ?2",
                (
                    parent.key.source_id.as_str(),
                    parent.key.relative_path.to_string_lossy().as_ref(),
                ),
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(reopened_parent.0, HarvestState::Touched.as_str());
        assert_eq!(reopened_parent.1.as_deref(), parent.content_hash.as_deref());
        assert!(
            reopened_parent.2.is_some(),
            "reopened graph should preserve the parent's touched timestamp"
        );
        let reopened_child: (Option<i64>, Option<i64>, Option<String>) = reopened
            .query_row(
                "SELECT file_size, modified_ns, content_hash
                 FROM harvest_files
                 WHERE source_id = ?1 AND relative_path = ?2",
                (
                    child.key.source_id.as_str(),
                    child.key.relative_path.to_string_lossy().as_ref(),
                ),
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(reopened_child.0, child.file_size.map(|value| value as i64));
        assert_eq!(reopened_child.1, child.modified_ns);
        assert_eq!(reopened_child.2.as_deref(), child.content_hash.as_deref());
        let reopened_edge: ReopenedDerivationRow = reopened
            .query_row(
                "SELECT operation, source_range_start, source_range_end,
                    output_duration_seconds, destination_folder, inherited_rating,
                    inherited_tags_json, inherited_playback_type, tool_version
                 FROM harvest_derivations
                 WHERE parent_source_id = ?1 AND parent_relative_path = ?2
                    AND child_source_id = ?3 AND child_relative_path = ?4",
                (
                    parent.key.source_id.as_str(),
                    parent.key.relative_path.to_string_lossy().as_ref(),
                    child.key.source_id.as_str(),
                    child.key.relative_path.to_string_lossy().as_ref(),
                ),
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            reopened_edge.0,
            HarvestDerivationOperation::Extract.as_str()
        );
        assert_eq!(reopened_edge.1, Some(1.25));
        assert_eq!(reopened_edge.2, Some(2.5));
        assert_eq!(reopened_edge.3, Some(1.25));
        assert_eq!(reopened_edge.4.as_deref(), Some("_Harvests/source-a"));
        assert_eq!(reopened_edge.5, Some(1));
        assert_eq!(reopened_edge.6, "[\"drum\",\"keep\"]");
        assert_eq!(reopened_edge.7.as_deref(), Some("one-shot"));
        assert_eq!(reopened_edge.8, "test");
    });
}

#[test]
fn remaps_harvest_graph_keys_after_file_moves() {
    let temp = tempdir().unwrap();
    with_config_home(temp.path(), || {
        let parent = harvest_identity("source-a", "incoming/jam.wav");
        let child = harvest_identity("source-a", "incoming/jam_chop.wav");
        record_harvest_derivation(&NewHarvestDerivation {
            parent: parent.clone(),
            child: child.clone(),
            operation: HarvestDerivationOperation::Extract,
            source_range: Some(HarvestSourceRange {
                start_seconds: 0.5,
                end_seconds: 1.0,
            }),
            output_duration_seconds: Some(0.5),
            destination_folder: Some(PathBuf::from("incoming")),
            inherited_metadata: HarvestMetadataSnapshot {
                rating: Some(2),
                tags: vec!["keep".to_string()],
                playback_type: Some("loop".to_string()),
            },
            tool_version: "test".to_string(),
        })
        .unwrap();

        let moved_parent = HarvestFileKey::new(
            SourceId::from_string("source-a"),
            PathBuf::from("reviewed/jam.wav"),
        );
        let moved_child = HarvestFileKey::new(
            SourceId::from_string("source-b"),
            PathBuf::from("_Harvests/jam_chop.wav"),
        );

        assert!(remap_harvest_file_key(&parent.key, &moved_parent).unwrap() > 0);
        assert!(
            harvest_derivations_for_parent(&parent.key)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            harvest_derivations_for_parent(&moved_parent)
                .unwrap()
                .first()
                .map(|edge| edge.parent.key.clone()),
            Some(moved_parent.clone())
        );

        assert!(remap_harvest_file_key(&child.key, &moved_child).unwrap() > 0);
        assert!(harvest_parents_for_child(&child.key).unwrap().is_empty());
        let parent_edges = harvest_derivations_for_parent(&moved_parent).unwrap();
        assert_eq!(parent_edges.len(), 1);
        assert_eq!(parent_edges[0].parent.key, moved_parent);
        assert_eq!(parent_edges[0].child.key, moved_child);
        assert_eq!(
            parent_edges[0].operation,
            HarvestDerivationOperation::Extract
        );
        assert_eq!(
            parent_edges[0].inherited_metadata.tags,
            vec!["keep".to_string()]
        );
    });
}

#[test]
fn remaps_harvest_graph_prefix_after_folder_moves() {
    let temp = tempdir().unwrap();
    with_config_home(temp.path(), || {
        let parent = harvest_identity("source-a", "incoming/kicks/kick.wav");
        let child = harvest_identity("source-a", "incoming/kicks/kick_chop.wav");
        let sibling_parent = harvest_identity("source-a", "incoming/kicks-old/hat.wav");
        let sibling_child = harvest_identity("source-a", "incoming/kicks-old/hat_chop.wav");
        record_harvest_derivation(&NewHarvestDerivation {
            parent: parent.clone(),
            child: child.clone(),
            operation: HarvestDerivationOperation::Extract,
            source_range: None,
            output_duration_seconds: None,
            destination_folder: Some(PathBuf::from("incoming/kicks")),
            inherited_metadata: HarvestMetadataSnapshot::default(),
            tool_version: "test".to_string(),
        })
        .unwrap();
        record_harvest_derivation(&NewHarvestDerivation {
            parent: sibling_parent.clone(),
            child: sibling_child.clone(),
            operation: HarvestDerivationOperation::Extract,
            source_range: None,
            output_duration_seconds: None,
            destination_folder: Some(PathBuf::from("incoming/kicks-old")),
            inherited_metadata: HarvestMetadataSnapshot::default(),
            tool_version: "test".to_string(),
        })
        .unwrap();

        assert!(
            remap_harvest_file_prefix(
                &SourceId::from_string("source-a"),
                PathBuf::from("incoming/kicks").as_path(),
                PathBuf::from("reviewed/kicks").as_path(),
            )
            .unwrap()
                > 0
        );

        let moved_parent = HarvestFileKey::new(
            SourceId::from_string("source-a"),
            PathBuf::from("reviewed/kicks/kick.wav"),
        );
        let moved_child = HarvestFileKey::new(
            SourceId::from_string("source-a"),
            PathBuf::from("reviewed/kicks/kick_chop.wav"),
        );
        assert!(
            harvest_derivations_for_parent(&parent.key)
                .unwrap()
                .is_empty()
        );
        assert!(harvest_parents_for_child(&child.key).unwrap().is_empty());

        let moved_edges = harvest_derivations_for_parent(&moved_parent).unwrap();
        assert_eq!(moved_edges.len(), 1);
        assert_eq!(moved_edges[0].parent.key, moved_parent);
        assert_eq!(moved_edges[0].child.key, moved_child);

        let sibling_edges = harvest_derivations_for_parent(&sibling_parent.key).unwrap();
        assert_eq!(sibling_edges.len(), 1);
        assert_eq!(sibling_edges[0].parent.key, sibling_parent.key);
        assert_eq!(sibling_edges[0].child.key, sibling_child.key);
    });
}

fn assert_library_db_schema_contract(connection: &Connection) {
    for table in LIBRARY_DB_SCHEMA_CONTRACT {
        let actual = column_names(connection, table.name)
            .into_iter()
            .collect::<HashSet<_>>();
        assert!(
            !actual.is_empty(),
            "expected library DB table `{}` to exist",
            table.name
        );
        for column in table.columns {
            assert!(
                actual.contains(*column),
                "expected library DB table `{}` to contain column `{}`; actual columns: {:?}",
                table.name,
                column,
                actual
            );
        }
    }
}

fn column_names(connection: &Connection, table_name: &str) -> Vec<String> {
    let pragma = format!("PRAGMA table_info({table_name})");
    let mut stmt = connection.prepare(&pragma).unwrap();
    stmt.query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn harvest_identity(source_id: &str, relative_path: &str) -> HarvestFileIdentity {
    HarvestFileIdentity {
        key: HarvestFileKey::new(
            SourceId::from_string(source_id.to_string()),
            PathBuf::from(relative_path),
        ),
        file_size: Some(123),
        modified_ns: Some(456),
        content_hash: Some(format!("hash-{relative_path}")),
    }
}

#[test]
fn creates_embedding_tables() {
    let temp = tempdir().unwrap();
    with_config_home(temp.path(), || {
        let conn = open_connection().unwrap();
        for table in [
            "embeddings",
            "ann_index_meta",
            "layout_umap",
            "hdbscan_clusters",
        ] {
            let exists: Option<String> = conn
                .query_row(
                    "SELECT name FROM sqlite_master WHERE type='table' AND name = ?1",
                    [table],
                    |row| row.get(0),
                )
                .optional()
                .unwrap();
            assert_eq!(exists.as_deref(), Some(table));
        }
    });
}

#[test]
fn applies_workload_pragmas_and_indices() {
    let temp = tempdir().unwrap();
    with_config_home(temp.path(), || {
        let conn = open_connection().unwrap();

        let journal_mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(journal_mode.to_ascii_lowercase(), "wal");

        let synchronous: i64 = conn
            .query_row("PRAGMA synchronous", [], |row| row.get(0))
            .unwrap();
        assert_eq!(synchronous, 1, "expected PRAGMA synchronous=NORMAL (1)");

        let wal_autocheckpoint: i64 = conn
            .query_row("PRAGMA wal_autocheckpoint", [], |row| row.get(0))
            .unwrap();
        assert_eq!(wal_autocheckpoint, 4096);

        let journal_size_limit: i64 = conn
            .query_row("PRAGMA journal_size_limit", [], |row| row.get(0))
            .unwrap();
        assert_eq!(journal_size_limit, 67_108_864);

        let busy_timeout: i64 = conn
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
            .unwrap();
        assert_eq!(busy_timeout, 5000);

        let idx: Option<String> = conn
            .query_row(
                "SELECT name FROM sqlite_master WHERE type='index' AND name='idx_analysis_jobs_status_created_id'",
                [],
                |row| row.get(0),
            )
            .optional()
            .unwrap();
        assert_eq!(idx.as_deref(), Some("idx_analysis_jobs_status_created_id"));
    });
}

#[test]
fn reuses_known_source_id_for_same_root() {
    let temp = tempdir().unwrap();
    with_config_home(temp.path(), || {
        let root = normalize_path(Path::new("some/root"));
        let id = SourceId::new();
        save(&LibraryState {
            sources: vec![SampleSource::new_with_id(id.clone(), root.clone())],
        })
        .unwrap();

        // Simulate removal by saving with no sources; mapping should still be remembered.
        save(&LibraryState { sources: vec![] }).unwrap();

        let reused = lookup_source_id_for_root(&root)
            .unwrap()
            .expect("expected mapping");
        assert_eq!(reused.as_str(), id.as_str());
    });
}

#[test]
fn retained_source_restores_protected_app_data_descriptor_after_removal() {
    let temp = tempdir().unwrap();
    with_config_home(temp.path(), || {
        let root = normalize_path(Path::new("protected/root"));
        let mut protected = SampleSource::new(root.clone()).protected();
        protected.primary_import_folder = PathBuf::from("Retained Inbox");
        save(&LibraryState {
            sources: vec![protected.clone()],
        })
        .unwrap();
        save(&LibraryState { sources: vec![] }).unwrap();

        let retained = lookup_retained_source_for_root(&root)
            .unwrap()
            .expect("retained descriptor");
        assert_eq!(retained.id, protected.id);
        assert_eq!(retained.role, SourceRole::Protected);
        assert_eq!(retained.metadata_storage, SourceMetadataStorage::AppData);
        assert_eq!(retained.primary_import_folder, Path::new("Retained Inbox"));

        let retained_set = retained_sources().unwrap();
        assert_eq!(retained_set.len(), 1);
        assert_eq!(retained_set[0].id, retained.id);
        assert_eq!(retained_set[0].root, retained.root);
        assert_eq!(retained_set[0].role, retained.role);
        assert_eq!(retained_set[0].metadata_storage, retained.metadata_storage);
    });
}

#[test]
fn unknown_retained_descriptor_values_fail_closed() {
    let temp = tempdir().unwrap();
    with_config_home(temp.path(), || {
        let db = LibraryDatabase::open().unwrap();
        db.set_metadata(
            KNOWN_SOURCES_KEY,
            r#"[{"root":"protected/root","source_id":"protected-id","role":"future_role"}]"#,
        )
        .unwrap();

        let err = lookup_retained_source_for_root(Path::new("protected/root")).unwrap_err();
        assert!(matches!(
            err,
            LibraryError::InvalidRetainedSourceDescriptor { field: "role", .. }
        ));
    });
}

#[test]
fn legacy_known_source_mapping_defaults_to_normal_source_folder_descriptor() {
    let temp = tempdir().unwrap();
    with_config_home(temp.path(), || {
        let db = LibraryDatabase::open().unwrap();
        db.set_metadata(
            KNOWN_SOURCES_KEY,
            r#"[{"root":"legacy/root","source_id":"legacy-id"}]"#,
        )
        .unwrap();

        let retained = lookup_retained_source_for_root(Path::new("legacy/root"))
            .unwrap()
            .expect("legacy retained descriptor");
        assert_eq!(retained.id.as_str(), "legacy-id");
        assert_eq!(retained.role, SourceRole::Normal);
        assert_eq!(
            retained.metadata_storage,
            SourceMetadataStorage::SourceFolder
        );
    });
}

#[test]
fn corrupt_known_source_metadata_is_reported() {
    let temp = tempdir().unwrap();
    with_config_home(temp.path(), || {
        let db = LibraryDatabase::open().unwrap();
        db.set_metadata(KNOWN_SOURCES_KEY, "{not valid json")
            .unwrap();

        let err = db
            .lookup_known_source_id(Path::new("some/root"))
            .unwrap_err();

        assert!(matches!(
            err,
            LibraryError::MetadataJson {
                key: KNOWN_SOURCES_KEY,
                ..
            }
        ));
        assert!(err.to_string().contains(KNOWN_SOURCES_KEY));
    });
}

#[test]
fn replace_state_rolls_back_sources_when_known_source_metadata_write_fails() {
    let temp = tempdir().unwrap();
    with_config_home(temp.path(), || {
        let original_root = normalize_path(Path::new("original/root"));
        let original_id = SourceId::new();
        save(&LibraryState {
            sources: vec![SampleSource::new_with_id(
                original_id.clone(),
                original_root.clone(),
            )],
        })
        .unwrap();

        let mut db = LibraryDatabase::open().unwrap();
        db.connection
            .execute_batch(
                "CREATE TRIGGER fail_known_sources_update
                 BEFORE UPDATE ON metadata
                 WHEN NEW.key = 'known_sources_v1'
                 BEGIN
                     SELECT RAISE(FAIL, 'known source metadata write failed');
                 END;",
            )
            .unwrap();

        let new_root = normalize_path(Path::new("new/root"));
        let err = db
            .replace_state(&LibraryState {
                sources: vec![SampleSource::new(new_root)],
            })
            .unwrap_err();

        assert!(matches!(err, LibraryError::Sql(_)));
        let sources = db.load_sources().unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].id.as_str(), original_id.as_str());
        assert_eq!(sources[0].root, original_root);
    });
}
