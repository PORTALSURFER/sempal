use super::super::test_support::{
    dummy_controller, prepare_with_source_and_wav_entries, sample_entry,
};
use crate::app::state::StatusTone;

#[test]
fn adding_source_rejects_same_resolved_root_with_different_spelling() {
    let config_root = tempfile::tempdir().expect("create config root");
    let _guard = crate::app_dirs::ConfigBaseGuard::set(config_root.path().to_path_buf());
    let source_root = tempfile::tempdir().expect("create source root");
    let (mut controller, _) = dummy_controller();
    controller.library.sources.clear();

    controller
        .add_source_from_path(source_root.path().to_path_buf())
        .expect("add source");
    let persisted = crate::sample_sources::config::load_or_default().expect("load saved config");
    assert_eq!(
        persisted.core.last_selected_source,
        controller.selected_source_id()
    );
    controller
        .add_source_from_path(source_root.path().join("."))
        .expect("duplicate source alias should short-circuit");

    assert_eq!(controller.library.sources.len(), 1);
    assert_eq!(controller.ui.status.text, "Source already added");
    assert_eq!(controller.ui.status.status_tone, StatusTone::Info);
}

#[test]
fn adding_source_rejects_nested_source_roots() {
    let config_root = tempfile::tempdir().expect("create config root");
    let _guard = crate::app_dirs::ConfigBaseGuard::set(config_root.path().to_path_buf());
    let library_root = tempfile::tempdir().expect("create library root");
    let parent = library_root.path().join("packs");
    let child = parent.join("drums");
    std::fs::create_dir_all(&child).expect("create nested source roots");

    let (mut controller, _) = dummy_controller();
    controller.library.sources.clear();
    controller
        .add_source_from_path(parent.clone())
        .expect("add parent source");

    let child_err = controller
        .add_source_from_path(child.clone())
        .expect_err("nested child source should be rejected");
    assert!(child_err.contains("Source folders cannot be nested"));
    assert!(child_err.contains("is inside existing source"));
    assert!(child_err.contains("Remove or remap the existing source"));
    assert_eq!(controller.library.sources.len(), 1);

    let (mut controller, _) = dummy_controller();
    controller.library.sources.clear();
    controller
        .add_source_from_path(child)
        .expect("add child source first");

    let parent_err = controller
        .add_source_from_path(parent)
        .expect_err("containing parent source should be rejected");
    assert!(parent_err.contains("Source folders cannot be nested"));
    assert!(parent_err.contains("contains existing source"));
    assert!(parent_err.contains("Remove or remap the existing source"));
    assert_eq!(controller.library.sources.len(), 1);
}

#[test]
/// Verifies removing source rolls back when config save fails.
fn removing_source_rolls_back_when_config_save_fails() {
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "one.wav",
        crate::sample_sources::Rating::NEUTRAL,
    )]);
    controller.assign_source_to_folder_pane(
        crate::app::state::FolderPaneId::Upper,
        Some(source.id.clone()),
    );
    controller
        .selection_state
        .ctx
        .last_selected_browsable_source = Some(source.id.clone());

    let config_blocker = tempfile::NamedTempFile::new().expect("create config blocker file");
    let _guard = crate::app_dirs::ConfigBaseGuard::set(config_blocker.path().to_path_buf());

    controller.remove_source(0);

    assert_eq!(controller.library.sources.len(), 1);
    assert_eq!(controller.library.sources[0].id, source.id);
    assert_eq!(controller.selected_source_id(), Some(source.id.clone()));
    assert_eq!(controller.ui.status.status_tone, StatusTone::Error);
    assert!(
        controller
            .ui
            .status
            .text
            .contains("Failed to save config after removing source")
    );
    assert_ne!(controller.ui.status.text, "Source removed");
}

#[test]
fn remapping_source_snapshots_wal_resident_metadata() {
    let config_root = tempfile::tempdir().expect("create config root");
    let _guard = crate::app_dirs::ConfigBaseGuard::set(config_root.path().to_path_buf());
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "wal.wav",
        crate::sample_sources::Rating::NEUTRAL,
    )]);
    let source_db = controller.cache.db.get(&source.id).unwrap().clone();
    source_db
        .set_tag(
            std::path::Path::new("wal.wav"),
            crate::sample_sources::Rating::KEEP_3,
        )
        .expect("commit WAL-resident tag");
    let source_wal = std::path::PathBuf::from(format!(
        "{}-wal",
        crate::sample_sources::database_path_for(&source.root).display()
    ));
    assert!(source_wal.metadata().expect("source WAL").len() > 0);
    let destination = tempfile::tempdir().expect("create destination root");

    controller
        .remap_source_to(0, destination.path().to_path_buf())
        .expect("remap source");

    let destination_db = crate::sample_sources::SourceDatabase::open_for_test_fixture_source_write(
        destination.path(),
    )
    .expect("open destination snapshot");
    let entry = destination_db
        .entry_for_path(std::path::Path::new("wal.wav"))
        .expect("query snapshot")
        .expect("snapshotted row");
    assert_eq!(entry.tag, crate::sample_sources::Rating::KEEP_3);
}

#[test]
fn remapping_source_rolls_back_root_and_snapshot_when_config_save_fails() {
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "rollback.wav",
        crate::sample_sources::Rating::KEEP_1,
    )]);
    let destination = tempfile::tempdir().expect("destination");
    let config_blocker = tempfile::NamedTempFile::new().expect("config blocker");
    let _guard = crate::app_dirs::ConfigBaseGuard::set(config_blocker.path().to_path_buf());

    let error = controller
        .remap_source_to(0, destination.path().to_path_buf())
        .expect_err("config save should fail");

    assert!(error.contains("Failed to save config after remapping source"));
    assert_eq!(controller.library.sources[0].root, source.root);
    assert!(!crate::sample_sources::database_path_for(destination.path()).exists());
}

#[test]
fn remapping_source_preserves_and_migrates_legacy_destination_database() {
    let config_root = tempfile::tempdir().expect("create config root");
    let _guard = crate::app_dirs::ConfigBaseGuard::set(config_root.path().to_path_buf());
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "source.wav",
        crate::sample_sources::Rating::NEUTRAL,
    )]);
    controller.cache.db.remove(&source.id);
    std::fs::remove_file(crate::sample_sources::database_path_for(&source.root))
        .expect("remove source database");
    let destination = tempfile::tempdir().expect("destination root");
    let legacy = destination
        .path()
        .join(crate::sample_sources::db::LEGACY_DB_FILE_NAME);
    let destination_db = crate::sample_sources::SourceDatabase::open_for_test_fixture_source_write(
        destination.path(),
    )
    .expect("destination database");
    destination_db
        .upsert_file(std::path::Path::new("legacy.wav"), 10, 5)
        .expect("legacy row");
    drop(destination_db);
    std::fs::rename(
        crate::sample_sources::database_path_for(destination.path()),
        &legacy,
    )
    .expect("rename current database to legacy name");

    controller
        .remap_source_to(0, destination.path().to_path_buf())
        .expect("remap source");

    let destination_db = crate::sample_sources::SourceDatabase::open_for_test_fixture_source_write(
        destination.path(),
    )
    .expect("migrated destination database");
    assert!(
        destination_db
            .entry_for_path(std::path::Path::new("legacy.wav"))
            .expect("legacy query")
            .is_some()
    );
}

#[test]
fn remapping_source_restores_legacy_destination_when_config_save_fails() {
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "source.wav",
        crate::sample_sources::Rating::NEUTRAL,
    )]);
    let destination = tempfile::tempdir().expect("destination root");
    let current = crate::sample_sources::database_path_for(destination.path());
    let legacy = destination
        .path()
        .join(crate::sample_sources::db::LEGACY_DB_FILE_NAME);
    let destination_db = crate::sample_sources::SourceDatabase::open_for_test_fixture_source_write(
        destination.path(),
    )
    .expect("destination database");
    destination_db
        .upsert_file(std::path::Path::new("legacy.wav"), 10, 5)
        .expect("legacy row");
    drop(destination_db);
    std::fs::rename(&current, &legacy).expect("rename current database to legacy name");
    let config_blocker = tempfile::NamedTempFile::new().expect("config blocker");
    let _guard = crate::app_dirs::ConfigBaseGuard::set(config_blocker.path().to_path_buf());

    let error = controller
        .remap_source_to(0, destination.path().to_path_buf())
        .expect_err("config save should fail");

    assert!(error.contains("Failed to save config after remapping source"));
    assert_eq!(controller.library.sources[0].root, source.root);
    assert!(legacy.is_file(), "legacy database name must be restored");
    assert!(!current.exists());
}

#[test]
fn remapping_source_rejects_destination_owned_by_pending_add() {
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "one.wav",
        crate::sample_sources::Rating::NEUTRAL,
    )]);
    let destination = tempfile::tempdir().expect("destination root");
    let pending_source = crate::sample_sources::SampleSource::new(destination.path().to_path_buf());
    controller.runtime.source_lane.pending_adds.insert(
        pending_source.root.clone(),
        crate::app::controller::state::runtime::PendingSourceAdd {
            request_id: 77,
            source: pending_source,
            queued_at: std::time::Instant::now(),
        },
    );

    let error = controller
        .remap_source_to(0, destination.path().to_path_buf())
        .expect_err("pending add destination must reject remap");

    assert!(error.contains("being added"));
    assert_eq!(controller.library.sources[0].root, source.root);
}

#[test]
fn removing_source_cancels_matching_pending_remap_generation() {
    let config_root = tempfile::tempdir().expect("config root");
    let _guard = crate::app_dirs::ConfigBaseGuard::set(config_root.path().to_path_buf());
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "pending.wav",
        crate::sample_sources::Rating::NEUTRAL,
    )]);
    controller.runtime.source_lane.pending_remap =
        Some(crate::app::controller::state::runtime::PendingSourceRemap {
            request_id: 41,
            source: source.clone(),
            new_root: tempfile::tempdir().expect("destination").keep(),
            queued_at: std::time::Instant::now(),
            canceled: false,
            write_fence: std::sync::Arc::new(
                crate::app::controller::jobs::SourceRemapWriteFence::default(),
            ),
        });

    controller.remove_source(0);

    assert!(
        controller
            .runtime
            .source_lane
            .pending_remap
            .as_ref()
            .is_some_and(|pending| pending.request_id == 41 && pending.canceled)
    );
}

#[test]
fn canceled_remap_completion_clears_active_status() {
    let config_root = tempfile::tempdir().expect("config root");
    let _guard = crate::app_dirs::ConfigBaseGuard::set(config_root.path().to_path_buf());
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "pending.wav",
        crate::sample_sources::Rating::NEUTRAL,
    )]);
    let new_root = tempfile::tempdir().expect("destination").keep();
    controller.runtime.source_lane.pending_remap =
        Some(crate::app::controller::state::runtime::PendingSourceRemap {
            request_id: 42,
            source: source.clone(),
            new_root: new_root.clone(),
            queued_at: std::time::Instant::now(),
            canceled: true,
            write_fence: std::sync::Arc::new(
                crate::app::controller::jobs::SourceRemapWriteFence::default(),
            ),
        });
    controller.set_status(
        format!("Remapping source to {}", new_root.display()),
        StatusTone::Info,
    );

    controller.handle_source_remap_prepared_message(
        crate::app::controller::jobs::SourceRemapPreparedResult {
            request_id: 42,
            source,
            new_root,
            staged_database: None,
            destination_current_database_identity:
                crate::app::controller::jobs::SourceRemapDatabaseIdentity::default(),
            destination_legacy_database_identity:
                crate::app::controller::jobs::SourceRemapDatabaseIdentity::default(),
            write_fence: std::sync::Arc::new(
                crate::app::controller::jobs::SourceRemapWriteFence::default(),
            ),
            result: Ok(()),
        },
    );

    assert!(controller.runtime.source_lane.pending_remap.is_none());
    assert_eq!(controller.ui.status.text, "Source remap canceled");
}

#[test]
fn source_mutation_cancels_matching_pending_remap_generation() {
    let config_root = tempfile::tempdir().expect("config root");
    let _guard = crate::app_dirs::ConfigBaseGuard::set(config_root.path().to_path_buf());
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "pending.wav",
        crate::sample_sources::Rating::NEUTRAL,
    )]);
    controller.runtime.source_lane.pending_remap =
        Some(crate::app::controller::state::runtime::PendingSourceRemap {
            request_id: 42,
            source: source.clone(),
            new_root: tempfile::tempdir().expect("destination").keep(),
            queued_at: std::time::Instant::now(),
            canceled: false,
            write_fence: std::sync::Arc::new(
                crate::app::controller::jobs::SourceRemapWriteFence::default(),
            ),
        });

    controller.begin_pending_file_mutation(&source.id, [std::path::PathBuf::from("pending.wav")]);

    assert!(
        controller
            .runtime
            .source_lane
            .pending_remap
            .as_ref()
            .is_some_and(|pending| pending.request_id == 42 && pending.canceled)
    );
    controller.finish_pending_file_mutation(&source.id, [std::path::PathBuf::from("pending.wav")]);
}

#[test]
fn metadata_mutation_cancels_matching_pending_remap_generation() {
    let config_root = tempfile::tempdir().expect("config root");
    let _guard = crate::app_dirs::ConfigBaseGuard::set(config_root.path().to_path_buf());
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "pending.wav",
        crate::sample_sources::Rating::NEUTRAL,
    )]);
    controller.runtime.source_lane.pending_remap =
        Some(crate::app::controller::state::runtime::PendingSourceRemap {
            request_id: 43,
            source: source.clone(),
            new_root: tempfile::tempdir().expect("destination").keep(),
            queued_at: std::time::Instant::now(),
            canceled: false,
            write_fence: std::sync::Arc::new(
                crate::app::controller::jobs::SourceRemapWriteFence::default(),
            ),
        });

    controller.queue_metadata_mutation(
        &source,
        vec![
            crate::app::controller::jobs::SourceMetadataMutationOp::SetTagAndLocked {
                relative_path: std::path::PathBuf::from("pending.wav"),
                tag: crate::sample_sources::Rating::KEEP_1,
                locked: false,
            },
        ],
        Vec::new(),
        Vec::new(),
        false,
    );

    assert!(
        controller
            .runtime
            .source_lane
            .pending_remap
            .as_ref()
            .is_some_and(|pending| pending.request_id == 43 && pending.canceled)
    );
}

#[test]
fn remap_rejects_existing_file_mutation() {
    let config_root = tempfile::tempdir().expect("config root");
    let _guard = crate::app_dirs::ConfigBaseGuard::set(config_root.path().to_path_buf());
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "pending.wav",
        crate::sample_sources::Rating::NEUTRAL,
    )]);
    let destination = tempfile::tempdir().expect("destination");
    controller.begin_pending_file_mutation(&source.id, [std::path::PathBuf::from("pending.wav")]);
    let _open_count_scope =
        crate::sample_sources::db::test_scope_source_db_open_total_count(&source.root);

    let error = controller
        .remap_source_to(0, destination.path().to_path_buf())
        .expect_err("pending file mutation must block remap");

    assert!(error.contains("changes are pending"));
    assert_eq!(controller.library.sources[0].root, source.root);
    assert_eq!(
        crate::sample_sources::db::test_source_db_open_total_count(&source.root),
        0,
        "pending mutation gate must run before analysis DB inspection"
    );
    controller.finish_pending_file_mutation(&source.id, [std::path::PathBuf::from("pending.wav")]);
}

#[test]
fn remap_rejects_active_scan_for_same_source() {
    let config_root = tempfile::tempdir().expect("config root");
    let _guard = crate::app_dirs::ConfigBaseGuard::set(config_root.path().to_path_buf());
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "scanning.wav",
        crate::sample_sources::Rating::NEUTRAL,
    )]);
    let destination = tempfile::tempdir().expect("destination");
    let (_sender, receiver) = std::sync::mpsc::channel();
    controller.runtime.jobs.start_scan(
        source.id.clone(),
        receiver,
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    );

    let error = controller
        .remap_source_to(0, destination.path().to_path_buf())
        .expect_err("active source scan must block remap");

    assert!(error.contains("being scanned"));
    assert_eq!(controller.library.sources[0].root, source.root);
}

#[test]
fn remap_rejects_active_trash_move() {
    let config_root = tempfile::tempdir().expect("config root");
    let _guard = crate::app_dirs::ConfigBaseGuard::set(config_root.path().to_path_buf());
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "trashed.wav",
        crate::sample_sources::Rating::TRASH_3,
    )]);
    let destination = tempfile::tempdir().expect("destination");
    let (_sender, receiver) = std::sync::mpsc::channel();
    controller.runtime.jobs.start_trash_move(
        receiver,
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    );

    let error = controller
        .remap_source_to(0, destination.path().to_path_buf())
        .expect_err("active trash move must block remap");

    assert!(error.contains("being moved to trash"));
    assert_eq!(controller.library.sources[0].root, source.root);
    controller.runtime.jobs.clear_trash_move();
}

#[test]
fn remap_rejects_active_deferred_file_operation() {
    let config_root = tempfile::tempdir().expect("config root");
    let _guard = crate::app_dirs::ConfigBaseGuard::set(config_root.path().to_path_buf());
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "undo.wav",
        crate::sample_sources::Rating::NEUTRAL,
    )]);
    let destination = tempfile::tempdir().expect("destination");
    let (_sender, receiver) = std::sync::mpsc::channel();
    controller.runtime.jobs.start_file_ops(
        receiver,
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    );

    let error = controller
        .remap_source_to(0, destination.path().to_path_buf())
        .expect_err("active deferred file operation must block remap");

    assert!(error.contains("file operations are running"));
    assert_eq!(controller.library.sources[0].root, source.root);
    controller.runtime.jobs.clear_file_ops();
}

#[test]
fn source_file_operation_started_during_live_remap_cancels_it() {
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "undo.wav",
        crate::sample_sources::Rating::NEUTRAL,
    )]);
    let write_fence =
        std::sync::Arc::new(crate::app::controller::jobs::SourceRemapWriteFence::default());
    controller.runtime.source_lane.pending_remap =
        Some(crate::app::controller::state::runtime::PendingSourceRemap {
            request_id: 95,
            source: source.clone(),
            new_root: tempfile::tempdir().expect("destination").keep(),
            queued_at: std::time::Instant::now(),
            canceled: false,
            write_fence: std::sync::Arc::clone(&write_fence),
        });
    let (_sender, receiver) = std::sync::mpsc::channel();

    controller.start_file_ops_with_remap_cancellation(
        receiver,
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    );

    assert!(write_fence.is_canceled());
    assert!(
        controller
            .runtime
            .source_lane
            .pending_remap
            .as_ref()
            .is_some_and(|pending| pending.canceled)
    );
    assert!(controller.runtime.jobs.file_ops_in_progress());
    controller.runtime.jobs.clear_file_ops();
}

#[test]
fn remap_rejects_active_database_maintenance_for_same_source() {
    let config_root = tempfile::tempdir().expect("config root");
    let _guard = crate::app_dirs::ConfigBaseGuard::set(config_root.path().to_path_buf());
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "maintenance.wav",
        crate::sample_sources::Rating::NEUTRAL,
    )]);
    let destination = tempfile::tempdir().expect("destination");
    controller.runtime.jobs.begin_source_db_maintenance(vec![
        crate::app::controller::jobs::SourceDbMaintenanceJob {
            source_id: source.id.clone(),
            source_root: source.root.clone(),
        },
    ]);

    let error = controller
        .remap_source_to(0, destination.path().to_path_buf())
        .expect_err("active source database maintenance must block remap");

    assert!(error.contains("database maintenance is running"));
    assert_eq!(controller.library.sources[0].root, source.root);
}

#[test]
fn remap_rejects_pending_analysis_jobs_for_same_source() {
    assert_remap_rejects_analysis_job_with_status("pending");
}

#[test]
fn remap_rejects_running_analysis_jobs_for_same_source() {
    assert_remap_rejects_analysis_job_with_status("running");
}

#[test]
fn remap_repairs_analysis_schema_before_activity_check() {
    let config_root = tempfile::tempdir().expect("config root");
    let _guard = crate::app_dirs::ConfigBaseGuard::set(config_root.path().to_path_buf());
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "legacy-schema.wav",
        crate::sample_sources::Rating::NEUTRAL,
    )]);
    let connection =
        crate::app::controller::library::analysis_jobs::db::open_source_db(&source.root)
            .expect("analysis database");
    connection
        .execute_batch("DROP TABLE analysis_jobs;")
        .expect("remove newer analysis schema");
    let destination = tempfile::tempdir().expect("destination");

    controller
        .remap_source_to(0, destination.path().to_path_buf())
        .expect("schema-assured remap");

    assert_eq!(controller.library.sources[0].root, destination.path());
    let destination_connection =
        crate::app::controller::library::analysis_jobs::db::open_source_db(destination.path())
            .expect("remapped analysis database");
    let table_count: i64 = destination_connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'analysis_jobs'",
            [],
            |row| row.get(0),
        )
        .expect("inspect repaired schema");
    assert_eq!(table_count, 1);
}

fn assert_remap_rejects_analysis_job_with_status(status: &str) {
    let config_root = tempfile::tempdir().expect("config root");
    let _guard = crate::app_dirs::ConfigBaseGuard::set(config_root.path().to_path_buf());
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "pending-analysis.wav",
        crate::sample_sources::Rating::NEUTRAL,
    )]);
    let destination = tempfile::tempdir().expect("destination");
    let connection =
        crate::app::controller::library::analysis_jobs::db::open_source_db(&source.root)
            .expect("analysis database");
    connection
        .execute(
            "INSERT INTO analysis_jobs
                 (sample_id, source_id, relative_path, job_type, status, attempts, created_at,
                  readiness_managed, readiness_scope_kind, readiness_scope_id, readiness_stage,
                  artifact_version, source_generation, content_generation,
                  readiness_claim_generation)
             VALUES (?1, ?2, ?3, ?4, ?5, 0, 0, 1, 'file', ?3,
                     'analysis_features', 'test-v1', 1, 'hash', 0)",
            rusqlite::params![
                format!("{}::pending-analysis.wav", source.id.as_str()),
                source.id.as_str(),
                "pending-analysis.wav",
                "readiness_analysis_features_v1",
                status,
            ],
        )
        .expect("active analysis job");

    let error = controller
        .remap_source_to(0, destination.path().to_path_buf())
        .expect_err("active analysis job must block remap");

    assert!(error.contains("pending or running"));
    assert_eq!(controller.library.sources[0].root, source.root);
}

#[test]
fn remap_rejects_existing_metadata_mutation() {
    let config_root = tempfile::tempdir().expect("config root");
    let _guard = crate::app_dirs::ConfigBaseGuard::set(config_root.path().to_path_buf());
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "pending.wav",
        crate::sample_sources::Rating::NEUTRAL,
    )]);
    let destination = tempfile::tempdir().expect("destination");
    controller
        .runtime
        .source_lane
        .mutations
        .insert_metadata_mutation(
            crate::app::controller::state::runtime::PendingMetadataMutation {
                request_id: 99,
                source_id: source.id.clone(),
                paths: [std::path::PathBuf::from("pending.wav")]
                    .into_iter()
                    .collect(),
                blocks_file_mutation: true,
                rollback: Vec::new(),
                refresh_browser_projection: false,
            },
        );

    let error = controller
        .remap_source_to(0, destination.path().to_path_buf())
        .expect_err("pending metadata mutation must block remap");

    assert!(error.contains("changes are pending"));
    assert_eq!(controller.library.sources[0].root, source.root);
}

#[test]
fn adding_source_publishes_no_runtime_state_when_config_save_fails() {
    let (mut controller, original_source) = dummy_controller();
    controller.library.sources.push(original_source.clone());
    controller.cache_db(&original_source).unwrap();
    let added_root = tempfile::tempdir().expect("create source root");
    let selected_before = controller.selected_source_id();
    let source_count_before = controller.library.sources.len();
    let db_cache_count_before = controller.cache.db.len();
    let ui_rows_before = controller.ui.sources.rows.len();
    let config_base = tempfile::tempdir().expect("create config base");
    let _guard = crate::app_dirs::ConfigBaseGuard::set(config_base.path().to_path_buf());
    let config_path =
        crate::sample_sources::config::config_path().expect("resolve isolated config path");
    std::fs::create_dir(&config_path).expect("block config file with a directory");

    let error = controller
        .add_source_from_path(added_root.path().to_path_buf())
        .expect_err("config persistence must fail");

    assert!(
        error.contains("Failed to save config after adding source"),
        "unexpected add-source failure: {error}"
    );
    assert_eq!(controller.library.sources.len(), source_count_before);
    assert_eq!(controller.library.sources[0].id, original_source.id);
    assert_eq!(controller.selected_source_id(), selected_before);
    assert_eq!(controller.cache.db.len(), db_cache_count_before);
    assert_eq!(controller.ui.sources.rows.len(), ui_rows_before);
}

#[test]
fn remapping_source_rolls_back_runtime_and_created_artifacts_when_config_save_fails() {
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "one.wav",
        crate::sample_sources::Rating::KEEP_1,
    )]);
    let old_root = source.root.clone();
    let new_root = tempfile::tempdir().expect("create remap root");
    let new_database = crate::sample_sources::database_path_for(new_root.path());
    let selected_before = controller.selected_source_id();
    let db_cache_before = controller.cache.db.get(&source.id).unwrap().clone();
    let wav_cache_count_before = controller.cache.wav.entries.len();
    let missing_before = controller.library.missing.sources.clone();
    let config_blocker = tempfile::NamedTempFile::new().expect("create config blocker file");
    let _guard = crate::app_dirs::ConfigBaseGuard::set(config_blocker.path().to_path_buf());

    let error = controller
        .remap_source_to(0, new_root.path().to_path_buf())
        .expect_err("config persistence must fail");

    assert!(error.contains("Failed to save config after remapping source"));
    assert_eq!(controller.library.sources[0].root, old_root);
    assert_eq!(controller.selected_source_id(), selected_before);
    assert!(std::rc::Rc::ptr_eq(
        controller.cache.db.get(&source.id).unwrap(),
        &db_cache_before
    ));
    assert_eq!(controller.cache.wav.entries.len(), wav_cache_count_before);
    assert_eq!(controller.library.missing.sources, missing_before);
    for artifact in [
        new_database.clone(),
        std::path::PathBuf::from(format!("{}-wal", new_database.display())),
        std::path::PathBuf::from(format!("{}-shm", new_database.display())),
        std::path::PathBuf::from(format!("{}-journal", new_database.display())),
    ] {
        assert!(
            !artifact.exists(),
            "orphan artifact: {}",
            artifact.display()
        );
    }
}

#[test]
fn remapping_source_publishes_runtime_changes_after_persistence_and_db_prepare() {
    let config_root = tempfile::tempdir().expect("create config root");
    let _guard = crate::app_dirs::ConfigBaseGuard::set(config_root.path().to_path_buf());
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "one.wav",
        crate::sample_sources::Rating::KEEP_1,
    )]);
    let new_root = tempfile::tempdir().expect("create remap root");

    controller
        .remap_source_to(0, new_root.path().to_path_buf())
        .expect("remap source");

    assert_eq!(controller.library.sources[0].root, new_root.path());
    assert_eq!(controller.selected_source_id(), Some(source.id.clone()));
    assert!(crate::sample_sources::database_path_for(new_root.path()).is_file());
    assert_eq!(controller.ui.status.text, "Source remapped");
    assert_eq!(controller.ui.status.status_tone, StatusTone::Info);
}

#[test]
fn failed_remap_forgets_target_root_mapping() {
    let config_root = tempfile::tempdir().expect("create config root");
    let _guard = crate::app_dirs::ConfigBaseGuard::set(config_root.path().to_path_buf());
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "one.wav",
        crate::sample_sources::Rating::KEEP_1,
    )]);
    let old_root = source.root.clone();
    let new_root = tempfile::tempdir().expect("create remap root");
    std::fs::create_dir(crate::sample_sources::database_path_for(new_root.path()))
        .expect("block destination database open");

    controller
        .remap_source_to(0, new_root.path().to_path_buf())
        .expect_err("database preparation must fail");

    assert_eq!(controller.library.sources[0].root, old_root);
    assert_eq!(
        crate::sample_sources::library::lookup_source_id_for_root(new_root.path())
            .expect("lookup failed remap root"),
        None
    );
}

#[test]
fn failed_remap_restores_preexisting_legacy_database_name() {
    let config_root = tempfile::tempdir().expect("create config root");
    let _guard = crate::app_dirs::ConfigBaseGuard::set(config_root.path().to_path_buf());
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "one.wav",
        crate::sample_sources::Rating::KEEP_1,
    )]);
    controller.cache.db.remove(&source.id);
    std::fs::remove_file(crate::sample_sources::database_path_for(&source.root))
        .expect("remove source database so remap does not copy it");
    let new_root = tempfile::tempdir().expect("create remap root");
    let legacy = new_root
        .path()
        .join(crate::sample_sources::db::LEGACY_DB_FILE_NAME);
    {
        let connection = rusqlite::Connection::open(&legacy).expect("create legacy database");
        connection
            .execute_batch("CREATE TABLE analysis_jobs (id INTEGER PRIMARY KEY);")
            .expect("create incompatible legacy schema");
    }

    controller
        .remap_source_to(0, new_root.path().to_path_buf())
        .expect_err("invalid migrated legacy database must fail preparation");

    assert!(legacy.is_file(), "legacy database name must be restored");
    assert!(!crate::sample_sources::database_path_for(new_root.path()).exists());
}
