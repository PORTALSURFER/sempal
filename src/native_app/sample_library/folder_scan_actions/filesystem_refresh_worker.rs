use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};

use wavecrate::sample_sources::{BrowserMetadataSnapshot, SourceDatabase};
use wavecrate_library::sample_sources::is_supported_audio;
use wavecrate_scan::sample_sources::scanner::{
    self, ScanWritePhase, ScanWriter, UncoordinatedScanWriter,
};

use crate::native_app::{
    app::{BrowserProjectionDelta, SourceFilesystemSyncResult, SourceFilesystemSyncSuccess},
    sample_library::folder_browser::model::file_entry_with_snapshot_metadata,
};

const MAX_SYNC_ATTEMPTS: usize = 3;
const SYNC_RETRY_DELAYS: [Duration; MAX_SYNC_ATTEMPTS - 1] =
    [Duration::from_millis(50), Duration::from_millis(200)];

pub(in crate::native_app) fn recover_source_filesystem_sync(
    source_id: String,
    lifecycle_generation: u64,
    changed_count: usize,
    work: impl FnOnce() -> SourceFilesystemSyncResult,
) -> SourceFilesystemSyncResult {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
        Ok(mut result) => {
            result.lifecycle_generation = lifecycle_generation;
            result
        }
        Err(_) => SourceFilesystemSyncResult {
            source_id,
            lifecycle_generation,
            changed_count,
            journal_checkpoint_event_id: None,
            cancelled: false,
            result: Err(String::from(
                "Source filesystem sync worker stopped unexpectedly",
            )),
        },
    }
}

pub(in crate::native_app) fn sync_source_database_paths(
    source_id: String,
    root: PathBuf,
    database_root: PathBuf,
    paths: Vec<PathBuf>,
    changed_count: usize,
    cancel: &AtomicBool,
) -> SourceFilesystemSyncResult {
    sync_source_database_paths_with_writer(
        source_id,
        root,
        database_root,
        paths,
        changed_count,
        cancel,
        &UncoordinatedScanWriter,
    )
}

pub(in crate::native_app) fn sync_source_database_paths_with_writer(
    source_id: String,
    root: PathBuf,
    database_root: PathBuf,
    paths: Vec<PathBuf>,
    changed_count: usize,
    cancel: &AtomicBool,
    writer: &impl ScanWriter,
) -> SourceFilesystemSyncResult {
    let mut result = Err(String::from("Source filesystem sync did not run"));
    for attempt in 0..MAX_SYNC_ATTEMPTS {
        result = sync_source_database_paths_once(
            &source_id,
            &root,
            &database_root,
            &paths,
            cancel,
            writer,
        );
        if result.is_ok() || cancel.load(Ordering::Acquire) {
            break;
        }
        let Some(delay) = SYNC_RETRY_DELAYS.get(attempt).copied() else {
            break;
        };
        tracing::warn!(
            source_id,
            attempt = attempt + 1,
            max_attempts = MAX_SYNC_ATTEMPTS,
            delay_ms = delay.as_millis(),
            error = %result.as_ref().expect_err("failed attempt"),
            "Retrying targeted source sync"
        );
        if !wait_for_retry(cancel, delay) {
            break;
        }
    }
    SourceFilesystemSyncResult {
        source_id,
        lifecycle_generation: 0,
        changed_count,
        journal_checkpoint_event_id: None,
        cancelled: cancel.load(Ordering::Acquire),
        result,
    }
}

fn sync_source_database_paths_once(
    source_id: &str,
    root: &std::path::Path,
    database_root: &std::path::Path,
    paths: &[PathBuf],
    cancel: &AtomicBool,
    writer: &impl ScanWriter,
) -> Result<SourceFilesystemSyncSuccess, String> {
    let browser_delta_eligible = paths.iter().all(|path| is_supported_audio(path));
    let _writer = writer.lock(ScanWritePhase::Open);
    if cancel.load(Ordering::Acquire) {
        return Err(String::from(
            "Source filesystem sync canceled before database open",
        ));
    }
    let database = SourceDatabase::open_for_background_job_with_database_root(root, database_root);
    drop(_writer);
    database
        .map_err(|err| format!("open source index: {err}"))
        .and_then(|db| {
            let (stats, mut incomplete_error) = match scanner::sync_paths_with_progress_and_writer(
                &db,
                paths,
                Some(cancel),
                &mut |_, _| {},
                writer,
            ) {
                Ok(stats) => (stats, None),
                Err(scanner::ScanError::Incomplete { committed, error }) => {
                    (*committed, Some(error))
                }
                Err(error) => return Err(format!("sync source index: {error}")),
            };
            let committed = stats.clone();
            let completed = if incomplete_error.is_some() {
                committed
            } else {
                match scanner::complete_deferred_rename_candidates_with_cancel_and_writer(
                    &db,
                    stats,
                    Some(cancel),
                    writer,
                ) {
                    Ok(completed) => completed,
                    Err(error) => {
                        incomplete_error = Some(error.to_string());
                        tracing::warn!(
                            source_id,
                            error = %error,
                            "Deferred rename reconciliation failed after filesystem sync committed"
                        );
                        committed
                    }
                }
            };
            let browser_projection_delta = if browser_delta_eligible
                && incomplete_error.is_none()
                && completed.committed_delta.revision > 0
            {
                match build_browser_projection_delta(root, &db, &completed.committed_delta) {
                    Ok(projection) => projection,
                    Err(error) => {
                        tracing::warn!(
                            source_id,
                            error,
                            "Falling back to a full browser projection after delta hydration failed"
                        );
                        None
                    }
                }
            } else {
                None
            };
            Ok(SourceFilesystemSyncSuccess {
                renames_reconciled: completed.renames_reconciled,
                incomplete_error,
                committed_delta: completed.committed_delta,
                browser_projection_delta,
            })
        })
}

fn build_browser_projection_delta(
    root: &std::path::Path,
    db: &SourceDatabase,
    delta: &scanner::CommittedSourceDelta,
) -> Result<Option<BrowserProjectionDelta>, String> {
    let BrowserMetadataSnapshot { revision, files } = db
        .browser_metadata_snapshot()
        .map_err(|error| format!("read committed browser projection delta: {error}"))?;
    if revision != delta.revision {
        tracing::info!(
            committed_revision = delta.revision,
            snapshot_revision = revision,
            "Browser delta snapshot was not the exact committed revision"
        );
        return Ok(None);
    }
    let upsert_paths = delta
        .created
        .iter()
        .map(|entry| entry.relative_path.as_path())
        .chain(
            delta
                .changed
                .iter()
                .map(|entry| entry.relative_path.as_path()),
        )
        .chain(
            delta
                .moved
                .iter()
                .map(|entry| entry.new_relative_path.as_path()),
        )
        .collect::<std::collections::HashSet<_>>();
    let mut folders = std::collections::BTreeSet::new();
    let upserted_files = files
        .into_iter()
        .filter(|entry| !entry.missing && upsert_paths.contains(entry.relative_path.as_path()))
        .map(|entry| {
            let absolute = root.join(&entry.relative_path);
            if let Some(parent) = absolute.parent() {
                folders.insert(parent.to_path_buf());
            }
            file_entry_with_snapshot_metadata(
                &absolute,
                entry.file_size,
                entry.rating,
                entry.locked,
                entry.collections,
                entry.last_played_at,
                entry.last_curated_at,
            )
        })
        .collect();
    let removed_file_ids = delta
        .deleted
        .iter()
        .map(|entry| {
            root.join(&entry.relative_path)
                .to_string_lossy()
                .to_string()
        })
        .chain(delta.moved.iter().map(|entry| {
            root.join(&entry.old_relative_path)
                .to_string_lossy()
                .to_string()
        }))
        .collect();
    Ok(Some(BrowserProjectionDelta {
        manifest_revision: delta.revision,
        snapshot_revision: revision,
        folders: folders.into_iter().collect(),
        removed_file_ids,
        upserted_files,
    }))
}

fn wait_for_retry(cancel: &AtomicBool, delay: Duration) -> bool {
    let deadline = std::time::Instant::now() + delay;
    while std::time::Instant::now() < deadline {
        if cancel.load(Ordering::Acquire) {
            return false;
        }
        thread::sleep(Duration::from_millis(10));
    }
    true
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        sync::atomic::AtomicBool,
    };

    use wavecrate::sample_sources::{Rating, SourceDatabase, scanner};

    use super::{recover_source_filesystem_sync, sync_source_database_paths};

    #[test]
    fn filesystem_sync_panic_returns_a_terminal_result() {
        let result = recover_source_filesystem_sync(String::from("source"), 17, 2, || {
            panic!("simulated targeted sync panic")
        });

        assert_eq!(result.source_id, "source");
        assert_eq!(result.lifecycle_generation, 17);
        assert_eq!(result.changed_count, 2);
        assert!(!result.cancelled);
        assert!(
            result
                .result
                .expect_err("panic must become an error")
                .contains("stopped unexpectedly")
        );
    }

    #[test]
    fn filesystem_sync_returns_deferred_rename_results_for_refresh() {
        let root = tempfile::tempdir().expect("source root");
        let old = root.path().join("old.wav");
        let new = root.path().join("new.wav");
        std::fs::write(&old, vec![5_u8; 9 * 1024 * 1024]).expect("large wav");
        let db =
            SourceDatabase::open_for_test_fixture_source_write(root.path()).expect("source db");
        scanner::hard_rescan(&db).expect("initial scan");
        db.set_tag(Path::new("old.wav"), Rating::KEEP_1)
            .expect("tag old path");
        std::fs::rename(&old, &new).expect("rename wav");

        let result = sync_source_database_paths(
            String::from("source-a"),
            root.path().to_path_buf(),
            root.path().to_path_buf(),
            vec![PathBuf::from("old.wav"), PathBuf::from("new.wav")],
            2,
            &AtomicBool::new(false),
        );

        let success = result.result.expect("sync result");
        assert_eq!(success.renames_reconciled, 1);
        assert_eq!(success.committed_delta.moved.len(), 1);
        let projection = success
            .browser_projection_delta
            .expect("exact browser projection delta");
        assert_eq!(projection.removed_file_ids.len(), 1);
        assert_eq!(projection.upserted_files.len(), 1);
        assert_eq!(
            db.entry_for_path(Path::new("new.wav"))
                .unwrap()
                .unwrap()
                .tag,
            Rating::KEEP_1
        );
    }

    #[test]
    fn filesystem_sync_leaves_non_rename_hashing_for_the_supervisor() {
        let root = tempfile::tempdir().expect("source root");
        let fresh = root.path().join("fresh.wav");
        std::fs::write(&fresh, vec![7_u8; 9 * 1024 * 1024]).expect("large wav");

        let result = sync_source_database_paths(
            String::from("source-a"),
            root.path().to_path_buf(),
            root.path().to_path_buf(),
            vec![PathBuf::from("fresh.wav")],
            1,
            &AtomicBool::new(false),
        );

        let success = result.result.expect("sync result");
        assert_eq!(success.renames_reconciled, 0);
        assert_eq!(success.committed_delta.created.len(), 1);
        assert_eq!(
            success
                .browser_projection_delta
                .expect("exact browser projection delta")
                .upserted_files
                .len(),
            1
        );
        let db =
            SourceDatabase::open_for_test_fixture_source_write(root.path()).expect("source db");
        assert!(
            db.entry_for_path(Path::new("fresh.wav"))
                .expect("read entry")
                .expect("fresh entry")
                .content_hash
                .is_none(),
            "ordinary deep hashing must remain queued for the supervisor"
        );
    }

    #[cfg(unix)]
    #[test]
    fn filesystem_sync_retires_a_symlinked_file_from_the_browser_projection() {
        use std::os::unix::fs as unix_fs;

        let root = tempfile::tempdir().expect("source root");
        let outside = tempfile::tempdir().expect("outside source root");
        let tracked = root.path().join("tracked.wav");
        std::fs::write(&tracked, b"tracked").expect("tracked wav");
        std::fs::write(outside.path().join("outside.wav"), b"outside").expect("outside wav");
        let db =
            SourceDatabase::open_for_test_fixture_source_write(root.path()).expect("source db");
        scanner::hard_rescan(&db).expect("initial scan");
        std::fs::remove_file(&tracked).expect("replace tracked wav");
        unix_fs::symlink(outside.path().join("outside.wav"), &tracked).expect("file link");

        let result = sync_source_database_paths(
            String::from("source-a"),
            root.path().to_path_buf(),
            root.path().to_path_buf(),
            vec![PathBuf::from("tracked.wav")],
            1,
            &AtomicBool::new(false),
        );

        let success = result.result.expect("sync result");
        assert!(
            db.entry_for_path(Path::new("tracked.wav"))
                .expect("read tracked entry")
                .is_none()
        );
        let projection = success
            .browser_projection_delta
            .expect("browser projection delta");
        assert_eq!(
            projection.removed_file_ids,
            vec![tracked.display().to_string()]
        );
        assert!(projection.upserted_files.is_empty());
    }

    #[test]
    fn filesystem_sync_reports_lifecycle_cancellation_for_requeue() {
        let root = tempfile::tempdir().expect("source root");
        let fresh = root.path().join("fresh.wav");
        std::fs::write(&fresh, b"fresh").expect("wav");
        let cancel = AtomicBool::new(true);

        let result = sync_source_database_paths(
            String::from("source-a"),
            root.path().to_path_buf(),
            root.path().to_path_buf(),
            vec![PathBuf::from("fresh.wav")],
            1,
            &cancel,
        );

        assert!(result.cancelled);
        assert!(result.result.is_err());
        let db =
            SourceDatabase::open_for_test_fixture_source_write(root.path()).expect("source db");
        assert!(
            db.entry_for_path(Path::new("fresh.wav"))
                .expect("read entry")
                .is_none()
        );
    }

    #[test]
    fn filesystem_sync_retries_a_transient_database_root_failure() {
        let root = tempfile::tempdir().expect("source root");
        std::fs::write(root.path().join("fresh.wav"), b"fresh").expect("wav");
        let database_parent = tempfile::tempdir().expect("database parent");
        let database_root = database_parent.path().join("source-db");
        std::fs::write(&database_root, b"temporarily blocked").expect("block database root");
        let repaired_root = database_root.clone();
        let repair = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(75));
            std::fs::remove_file(&repaired_root).expect("remove transient blocker");
            std::fs::create_dir(&repaired_root).expect("repair database root");
        });

        let result = sync_source_database_paths(
            String::from("source-a"),
            root.path().to_path_buf(),
            database_root,
            vec![PathBuf::from("fresh.wav")],
            1,
            &AtomicBool::new(false),
        );
        repair.join().expect("repair worker");

        let success = result.result.expect("transient sync should converge");
        assert_eq!(success.committed_delta.created.len(), 1);
    }
}
