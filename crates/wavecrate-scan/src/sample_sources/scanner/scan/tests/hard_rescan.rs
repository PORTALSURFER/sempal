use super::*;
use crate::sample_sources::scanner::scan_fs::force_directory_entry_failure;

#[test]
fn hard_rescan_prunes_missing_rows() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("one.wav");
    std::fs::write(&file_path, b"one").unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();

    std::fs::remove_file(&file_path).unwrap();
    scan_once(&db).unwrap();
    assert!(db.list_files().unwrap().is_empty());

    let stats = hard_rescan(&db).unwrap();
    assert_eq!(stats.missing, 0);
    let rows = db.list_files().unwrap();
    assert!(rows.is_empty());
    assert!(db.list_pending_renames().unwrap().is_empty());
}

#[test]
fn hard_rescan_prunes_missing_files_with_tags() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("one.wav");
    std::fs::write(&file_path, b"one").unwrap();

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();
    db.set_tag(Path::new("one.wav"), Rating::KEEP_1).unwrap();

    std::fs::remove_file(&file_path).unwrap();
    scan_once(&db).unwrap();

    let stats = hard_rescan(&db).unwrap();
    assert_eq!(stats.missing, 0);
    let rows = db.list_files().unwrap();
    assert!(rows.is_empty());
    assert!(db.list_pending_renames().unwrap().is_empty());
}

#[test]
fn hard_rescan_keeps_pending_rename_metadata_for_an_unreadable_subtree() {
    let dir = tempdir().unwrap();
    let protected = dir.path().join("protected");
    std::fs::create_dir(&protected).unwrap();
    let file_path = protected.join("kick.wav");
    std::fs::write(&file_path, b"kick").unwrap();
    std::fs::write(protected.join("notes.txt"), b"notes").unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();
    let tracked = db
        .entry_for_path(Path::new("protected/kick.wav"))
        .unwrap()
        .unwrap();
    let mut batch = db.write_batch().unwrap();
    batch.stage_pending_rename(&tracked).unwrap();
    batch.commit().unwrap();
    let generation_before = db
        .pending_rename_diagnostics()
        .unwrap()
        .authoritative_generation;

    std::fs::remove_file(file_path).unwrap();
    let failure = force_directory_entry_failure(&protected);
    let result = hard_rescan(&db);
    let ScanError::Incomplete { committed, .. } = result.unwrap_err() else {
        panic!("hard rescan must leave an unreadable subtree retryable");
    };
    assert!(committed.committed_delta.deleted.is_empty());
    assert!(
        db.list_pending_renames()
            .unwrap()
            .iter()
            .any(|entry| entry.relative_path == Path::new("protected/kick.wav"))
    );
    assert_eq!(
        db.pending_rename_diagnostics()
            .unwrap()
            .authoritative_generation,
        generation_before,
        "partial enumeration must not advance retention eligibility"
    );

    drop(failure);
    let recovered = hard_rescan(&db).unwrap();
    assert_eq!(recovered.missing, 1);
    assert!(db.list_pending_renames().unwrap().is_empty());
}

#[test]
fn hard_rescan_prunes_missing_without_touching_existing() {
    let dir = tempdir().unwrap();
    let keep_path = dir.path().join("keep.wav");
    let remove_path = dir.path().join("remove.wav");
    std::fs::write(&keep_path, b"keep").unwrap();
    std::fs::write(&remove_path, b"remove").unwrap();

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();

    std::fs::remove_file(&remove_path).unwrap();
    let stats = hard_rescan(&db).unwrap();
    assert_eq!(stats.missing, 1);

    let rows = db.list_files().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].relative_path, Path::new("keep.wav"));
    assert!(db.list_pending_renames().unwrap().is_empty());
}
