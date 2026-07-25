use super::*;
use crate::sample_sources::scanner::sync_paths;

#[test]
fn authoritative_scans_converge_large_deletion_churn_after_restart() {
    const FILES: usize = 256;

    let dir = tempdir().unwrap();
    for index in 0..FILES {
        std::fs::write(
            dir.path().join(format!("deleted-{index:04}.wav")),
            format!("sample-{index}"),
        )
        .unwrap();
    }
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    let initial = scan_once(&db).unwrap();
    assert_eq!(
        initial
            .pending_rename_diagnostics
            .as_ref()
            .unwrap()
            .authoritative_generation,
        1
    );

    for index in 0..FILES {
        std::fs::remove_file(dir.path().join(format!("deleted-{index:04}.wav"))).unwrap();
    }
    let deleted = scan_once(&db).unwrap();
    let deleted_diagnostics = deleted.pending_rename_diagnostics.as_ref().unwrap();
    assert_eq!(deleted_diagnostics.candidate_count, FILES);
    assert_eq!(deleted_diagnostics.oldest_staged_generation, Some(2));
    assert!(deleted_diagnostics.oldest_candidate_age_seconds.is_some());
    assert_eq!(deleted.pending_renames_pruned, 0);

    drop(db);
    let reopened = SourceDatabase::open_for_scan(dir.path()).unwrap();
    let grace = scan_once(&reopened).unwrap();
    assert_eq!(
        grace
            .pending_rename_diagnostics
            .as_ref()
            .unwrap()
            .candidate_count,
        FILES
    );
    assert_eq!(grace.pending_renames_pruned, 0);

    let converged = scan_once(&reopened).unwrap();
    let diagnostics = converged.pending_rename_diagnostics.as_ref().unwrap();
    assert_eq!(converged.pending_renames_pruned, FILES);
    assert_eq!(diagnostics.candidate_count, 0);
    assert_eq!(diagnostics.authoritative_generation, 4);
    assert!(reopened.list_pending_renames().unwrap().is_empty());
}

#[test]
fn targeted_batches_do_not_age_pending_metadata() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("deleted.wav");
    std::fs::write(&file, b"metadata").unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();
    db.set_tag(Path::new("deleted.wav"), Rating::KEEP_1)
        .unwrap();

    std::fs::remove_file(file).unwrap();
    let deleted = sync_paths(&db, &[Path::new("deleted.wav").to_path_buf()]).unwrap();
    assert_eq!(deleted.missing, 1);
    let generation = db
        .pending_rename_diagnostics()
        .unwrap()
        .authoritative_generation;

    for index in 0..8 {
        let missing = Path::new(&format!("unrelated-{index}.wav")).to_path_buf();
        sync_paths(&db, &[missing]).unwrap();
    }

    let diagnostics = db.pending_rename_diagnostics().unwrap();
    assert_eq!(diagnostics.authoritative_generation, generation);
    assert_eq!(diagnostics.candidate_count, 1);
    let pending = db.list_pending_renames().unwrap();
    assert_eq!(pending[0].metadata.tag, Rating::KEEP_1);
}

#[test]
fn offline_source_does_not_age_and_recovers_rename_after_reconnect() {
    let parent = tempdir().unwrap();
    let source = parent.path().join("source");
    let offline = parent.path().join("offline");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("old.wav"), b"retained metadata").unwrap();
    let db = SourceDatabase::open_for_scan(&source).unwrap();
    scan_once(&db).unwrap();
    db.set_tag(Path::new("old.wav"), Rating::KEEP_1).unwrap();

    std::fs::remove_file(source.join("old.wav")).unwrap();
    sync_paths(&db, &[Path::new("old.wav").to_path_buf()]).unwrap();
    let generation_before = db
        .pending_rename_diagnostics()
        .unwrap()
        .authoritative_generation;

    std::fs::rename(&source, &offline).unwrap();
    std::fs::write(offline.join("new.wav"), b"retained metadata").unwrap();
    assert!(scan_once(&db).is_err());
    std::fs::rename(&offline, &source).unwrap();

    assert_eq!(
        db.pending_rename_diagnostics()
            .unwrap()
            .authoritative_generation,
        generation_before
    );
    let recovered = scan_once(&db).unwrap();
    assert_eq!(recovered.renames_reconciled, 1);
    assert_eq!(
        db.entry_for_path(Path::new("new.wav"))
            .unwrap()
            .unwrap()
            .tag,
        Rating::KEEP_1
    );
}
