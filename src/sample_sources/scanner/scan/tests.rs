use super::*;
use crate::sample_sources::{Rating, SourceDatabase};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tempfile::tempdir;

#[cfg(unix)]
fn set_file_times(path: &Path, seconds: i64, nanos: i64) {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(path.as_os_str().as_bytes()).unwrap();
    let times = [
        libc::timespec {
            tv_sec: seconds,
            tv_nsec: nanos,
        },
        libc::timespec {
            tv_sec: seconds,
            tv_nsec: nanos,
        },
    ];
    let result = unsafe { libc::utimensat(libc::AT_FDCWD, c_path.as_ptr(), times.as_ptr(), 0) };
    assert_eq!(result, 0);
}

#[test]
fn scan_add_update_mark_missing() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("one.wav");
    std::fs::write(&file_path, b"one").unwrap();

    let db = SourceDatabase::open(dir.path()).unwrap();
    let first = scan_once(&db).unwrap();
    assert_eq!(first.added, 1);
    assert_eq!(first.content_changed, 1);
    assert_eq!(first.changed_samples.len(), 1);
    let initial = db.list_files().unwrap();
    assert_eq!(initial.len(), 1);
    assert_eq!(initial[0].tag, Rating::NEUTRAL);

    std::fs::write(&file_path, b"longer-data").unwrap();
    let second = scan_once(&db).unwrap();
    assert_eq!(second.updated, 1);
    assert_eq!(second.content_changed, 1);
    assert_eq!(second.changed_samples.len(), 1);

    std::fs::remove_file(&file_path).unwrap();
    let third = scan_once(&db).unwrap();
    assert_eq!(third.missing, 1);
    let rows = db.list_files().unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].missing);
    let fourth = scan_once(&db).unwrap();
    assert_eq!(fourth.missing, 0);

    std::fs::write(&file_path, b"one").unwrap();
    let fifth = scan_once(&db).unwrap();
    assert_eq!(fifth.added, 0);
    assert_eq!(fifth.updated, 1);
    assert_eq!(fifth.content_changed, 1);
    assert_eq!(fifth.changed_samples.len(), 1);
    let rows = db.list_files().unwrap();
    assert!(!rows[0].missing);
}

#[test]
fn scan_skips_analysis_when_hash_unchanged() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("one.wav");
    std::fs::write(&file_path, b"one").unwrap();

    let db = SourceDatabase::open(dir.path()).unwrap();
    let first = scan_once(&db).unwrap();
    assert_eq!(first.content_changed, 1);

    std::thread::sleep(Duration::from_millis(2));
    std::fs::write(&file_path, b"one").unwrap();

    let second = scan_once(&db).unwrap();
    assert_eq!(second.updated, 1);
    assert_eq!(second.content_changed, 0);
    assert!(second.changed_samples.is_empty());
}

#[test]
fn scan_ignores_non_wav_and_counts_nested() {
    let dir = tempdir().unwrap();
    let nested = dir.path().join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(dir.path().join("one.wav"), b"one").unwrap();
    std::fs::write(nested.join("two.wav"), b"two").unwrap();
    std::fs::write(dir.path().join("ignore.txt"), b"text").unwrap();

    let db = SourceDatabase::open(dir.path()).unwrap();
    let stats = scan_once(&db).unwrap();
    assert_eq!(stats.added, 2);
    assert_eq!(stats.total_files, 2);
    let rows = db.list_files().unwrap();
    assert_eq!(rows.len(), 2);
}

#[test]
fn scan_in_background_finishes() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("one.wav"), b"one").unwrap();
    let handle = scan_in_background(dir.path().to_path_buf());
    let stats = handle.join().unwrap().unwrap();
    assert_eq!(stats.added, 1);
}

#[test]
fn scan_with_progress_respects_cancel_flag() {
    use std::sync::atomic::AtomicBool;

    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("one.wav"), b"one").unwrap();
    let db = SourceDatabase::open(dir.path()).unwrap();

    let cancel = AtomicBool::new(true);
    let mut progress_called = false;
    let result = scan_with_progress(&db, ScanMode::Quick, Some(&cancel), &mut |_, _| {
        progress_called = true;
    });
    assert!(matches!(result, Err(ScanError::Canceled)));
    assert!(!progress_called);
}

#[test]
fn hard_rescan_prunes_missing_rows() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("one.wav");
    std::fs::write(&file_path, b"one").unwrap();
    let db = SourceDatabase::open(dir.path()).unwrap();
    scan_once(&db).unwrap();

    std::fs::remove_file(&file_path).unwrap();
    scan_once(&db).unwrap();
    let rows = db.list_files().unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].missing);

    let stats = hard_rescan(&db).unwrap();
    assert_eq!(stats.missing, 1);
    let rows = db.list_files().unwrap();
    assert!(rows.is_empty());
}

#[test]
fn scan_detects_missing_paths_without_double_counting() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("one.wav");
    std::fs::write(&file_path, b"one").unwrap();

    let db = SourceDatabase::open(dir.path()).unwrap();
    scan_once(&db).unwrap();

    std::fs::remove_file(&file_path).unwrap();
    let first = scan_once(&db).unwrap();
    assert_eq!(first.missing, 1);

    let second = scan_once(&db).unwrap();
    assert_eq!(second.missing, 0);
}

#[test]
fn scan_detects_changed_content_hash() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("one.wav");
    std::fs::write(&file_path, b"one").unwrap();

    let db = SourceDatabase::open(dir.path()).unwrap();
    scan_once(&db).unwrap();

    std::fs::write(&file_path, b"two").unwrap();
    let stats = scan_once(&db).unwrap();
    assert_eq!(stats.content_changed, 1);
    assert_eq!(stats.changed_samples.len(), 1);
}

#[test]
fn scan_detects_rename_and_preserves_tag() {
    let dir = tempdir().unwrap();
    let first_path = dir.path().join("one.wav");
    let second_path = dir.path().join("two.wav");
    std::fs::write(&first_path, b"one").unwrap();

    let db = SourceDatabase::open(dir.path()).unwrap();
    scan_once(&db).unwrap();
    db.set_tag(Path::new("one.wav"), Rating::KEEP_1).unwrap();

    std::fs::rename(&first_path, &second_path).unwrap();
    let stats = scan_once(&db).unwrap();

    assert_eq!(stats.missing, 0);
    assert_eq!(stats.added, 0);
    assert_eq!(stats.content_changed, 0);
    assert_eq!(stats.updated, 1);

    let rows = db.list_files().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].relative_path, PathBuf::from("two.wav"));
    assert_eq!(rows[0].tag, Rating::KEEP_1);
    assert!(!rows[0].missing);
}

#[test]
fn quick_scan_defers_hash_for_large_file() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("large.wav");
    std::fs::write(&file_path, vec![0u8; 9 * 1024 * 1024]).unwrap();

    let db = SourceDatabase::open(dir.path()).unwrap();
    let stats = scan_once(&db).unwrap();
    assert_eq!(stats.hashes_pending, 1);
    assert_eq!(stats.hashes_computed, 0);

    let rows = db.list_files().unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].content_hash.is_none());
}

#[test]
fn quick_scan_reconciles_large_rename_and_preserves_tag() {
    let dir = tempdir().unwrap();
    let first_path = dir.path().join("one.wav");
    let second_path = dir.path().join("two.wav");
    std::fs::write(&first_path, vec![0u8; 9 * 1024 * 1024]).unwrap();

    let db = SourceDatabase::open(dir.path()).unwrap();
    hard_rescan(&db).unwrap();
    db.set_tag(Path::new("one.wav"), Rating::KEEP_1).unwrap();

    std::fs::rename(&first_path, &second_path).unwrap();
    let stats = scan_once(&db).unwrap();
    assert_eq!(stats.renames_reconciled, 1);
    assert_eq!(stats.hashes_pending, 1);

    let rows = db.list_files().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].relative_path, PathBuf::from("two.wav"));
    assert_eq!(rows[0].tag, Rating::KEEP_1);
    assert!(!rows[0].missing);
    assert!(rows[0].content_hash.is_none());

    let deep_stats = super::super::scan_hash::deep_hash_scan(&db, None).unwrap();
    assert_eq!(deep_stats.hashes_computed, 1);
    assert_eq!(deep_stats.renames_reconciled, 0);

    let rows = db.list_files().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].relative_path, PathBuf::from("two.wav"));
    assert_eq!(rows[0].tag, Rating::KEEP_1);
    assert!(!rows[0].missing);
    assert!(rows[0].content_hash.is_some());
}

#[cfg(unix)]
#[test]
fn quick_scan_avoids_ambiguous_large_rename() {
    let dir = tempdir().unwrap();
    let first_path = dir.path().join("one.wav");
    let second_path = dir.path().join("two.wav");
    let third_path = dir.path().join("three.wav");
    let payload = vec![0u8; 9 * 1024 * 1024];
    std::fs::write(&first_path, &payload).unwrap();
    std::fs::write(&second_path, &payload).unwrap();

    let timestamp = 1_700_000_000i64;
    set_file_times(&first_path, timestamp, 0);
    set_file_times(&second_path, timestamp, 0);

    let db = SourceDatabase::open(dir.path()).unwrap();
    hard_rescan(&db).unwrap();
    db.set_tag(Path::new("one.wav"), Rating::KEEP_1).unwrap();

    std::fs::remove_file(&first_path).unwrap();
    std::fs::remove_file(&second_path).unwrap();
    std::fs::write(&third_path, &payload).unwrap();
    set_file_times(&third_path, timestamp, 0);

    let stats = scan_once(&db).unwrap();
    assert_eq!(stats.renames_reconciled, 0);
    assert_eq!(stats.added, 1);
    assert_eq!(stats.missing, 2);

    let rows = db.list_files().unwrap();
    assert_eq!(rows.len(), 3);
    let mut keep_row = None;
    let mut new_row = None;
    for row in &rows {
        if row.relative_path == PathBuf::from("one.wav") {
            keep_row = Some(row);
        }
        if row.relative_path == PathBuf::from("three.wav") {
            new_row = Some(row);
        }
    }
    let keep_row = keep_row.unwrap();
    let new_row = new_row.unwrap();
    assert!(keep_row.missing);
    assert_eq!(keep_row.tag, Rating::KEEP_1);
    assert!(!new_row.missing);
    assert_eq!(new_row.tag, Rating::NEUTRAL);
}

#[test]
fn hard_rescan_prunes_missing_files_with_tags() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("one.wav");
    std::fs::write(&file_path, b"one").unwrap();

    let db = SourceDatabase::open(dir.path()).unwrap();
    scan_once(&db).unwrap();
    db.set_tag(Path::new("one.wav"), Rating::KEEP_1).unwrap();

    std::fs::remove_file(&file_path).unwrap();
    scan_once(&db).unwrap();

    let stats = hard_rescan(&db).unwrap();
    assert_eq!(stats.missing, 1);
    let rows = db.list_files().unwrap();
    assert!(rows.is_empty());
}

#[test]
fn hard_rescan_prunes_missing_without_touching_existing() {
    let dir = tempdir().unwrap();
    let keep_path = dir.path().join("keep.wav");
    let remove_path = dir.path().join("remove.wav");
    std::fs::write(&keep_path, b"keep").unwrap();
    std::fs::write(&remove_path, b"remove").unwrap();

    let db = SourceDatabase::open(dir.path()).unwrap();
    scan_once(&db).unwrap();

    std::fs::remove_file(&remove_path).unwrap();
    let stats = hard_rescan(&db).unwrap();
    assert_eq!(stats.missing, 1);

    let rows = db.list_files().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].relative_path, PathBuf::from("keep.wav"));
}

#[cfg(unix)]
#[test]
fn scan_tolerates_vanishing_nested_directories() {
    let dir = tempdir().unwrap();
    let one = dir.path().join("one.wav");
    std::fs::write(&one, b"one").unwrap();

    let vanishing = dir.path().join("vanishing");
    std::fs::create_dir_all(&vanishing).unwrap();
    std::fs::write(vanishing.join("two.wav"), b"two").unwrap();

    let vanishing_for_thread = vanishing.clone();
    let killer = std::thread::spawn(move || {
        for _ in 0..200 {
            let _ = std::fs::remove_dir_all(&vanishing_for_thread);
            std::thread::sleep(Duration::from_millis(1));
        }
    });

    let db = SourceDatabase::open(dir.path()).unwrap();
    let stats = scan_once(&db).unwrap();
    assert!(stats.total_files >= 1);

    let rows = db.list_files().unwrap();
    assert!(
        rows.iter()
            .any(|row| row.relative_path == PathBuf::from("one.wav"))
    );

    let _ = killer.join();
}

#[cfg(unix)]
#[test]
fn scan_skips_symlink_directories() {
    use std::os::unix::fs as unix_fs;

    let dir = tempdir().unwrap();
    let nested = dir.path().join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(nested.join("two.wav"), b"two").unwrap();
    std::fs::write(dir.path().join("one.wav"), b"one").unwrap();

    let link = dir.path().join("nested_link");
    unix_fs::symlink(&nested, &link).unwrap();

    let db = SourceDatabase::open(dir.path()).unwrap();
    let stats = scan_once(&db).unwrap();
    assert_eq!(stats.total_files, 2);
    assert_eq!(stats.added, 2);
}

#[cfg(unix)]
#[test]
fn scan_skips_symlink_files() {
    use std::os::unix::fs as unix_fs;

    let dir = tempdir().unwrap();
    let target = dir.path().join("one.wav");
    std::fs::write(&target, b"one").unwrap();
    let link = dir.path().join("one_link.wav");
    unix_fs::symlink(&target, &link).unwrap();

    let db = SourceDatabase::open(dir.path()).unwrap();
    let stats = scan_once(&db).unwrap();
    assert_eq!(stats.total_files, 1);
    assert_eq!(stats.added, 1);
}
