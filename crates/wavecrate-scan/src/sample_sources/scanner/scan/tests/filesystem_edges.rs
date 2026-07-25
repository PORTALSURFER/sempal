use super::*;
use crate::sample_sources::scanner::scan_fs::force_directory_identity;
use crate::sample_sources::scanner::{DirectoryRepeatKind, SourceTreeDiagnostic};
use std::path::PathBuf;

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

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    let stats = scan_once(&db).unwrap();
    assert!(stats.total_files >= 1);

    let rows = db.list_files().unwrap();
    assert!(
        rows.iter()
            .any(|row| row.relative_path == Path::new("one.wav"))
    );

    let _ = killer.join();
}

#[test]
fn full_scan_rejects_a_root_swap_before_the_first_checkpoint() {
    let parent = tempdir().unwrap();
    let root = parent.path().join("source");
    std::fs::create_dir(&root).unwrap();
    for index in 0..64 {
        std::fs::write(
            root.join(format!("sample-{index}.wav")),
            format!("old-{index}"),
        )
        .unwrap();
    }
    let db = SourceDatabase::open_for_scan(&root).unwrap();
    let mut swapped = false;
    let result = scan_with_progress(&db, ScanMode::Quick, None, &mut |count, _| {
        if count == 64 && !swapped {
            swapped = true;
            let old_root = parent.path().join("old-source");
            std::fs::rename(&root, &old_root).unwrap();
            std::fs::create_dir(&root).unwrap();
            for index in 0..64 {
                std::fs::write(
                    root.join(format!("sample-{index}.wav")),
                    format!("new-{index}"),
                )
                .unwrap();
            }
        }
    });

    assert!(matches!(result, Err(ScanError::StaleRootGeneration { .. })));
    assert!(db.list_files().unwrap().is_empty());
}

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

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    let stats = scan_once(&db).unwrap();
    assert_eq!(stats.total_files, 2);
    assert_eq!(stats.added, 2);
}

#[test]
fn full_scan_stops_an_injected_directory_identity_cycle() {
    let dir = tempdir().unwrap();
    let cycle = dir.path().join("cycle");
    std::fs::create_dir(&cycle).unwrap();
    std::fs::write(dir.path().join("root.wav"), b"root").unwrap();
    std::fs::write(cycle.join("never-reached.wav"), b"cycle").unwrap();

    let _root_identity = force_directory_identity(dir.path(), Some("root-identity"));
    let _cycle_identity = force_directory_identity(&cycle, Some("root-identity"));
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    let ScanError::Incomplete { committed, .. } = scan_once(&db).unwrap_err() else {
        panic!("an injected repeated subtree must return the committed partial result");
    };
    let stats = *committed;
    let snapshot = stats.source_tree_snapshot.expect("source tree snapshot");

    assert_eq!(stats.total_files, 1);
    assert!(!snapshot.is_complete());
    assert!(!snapshot.directories.contains(&PathBuf::from("cycle")));
    assert!(snapshot.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        SourceTreeDiagnostic::RepeatedDirectory {
            kind: DirectoryRepeatKind::Cycle,
            path,
            ..
        } if path == std::path::Path::new("cycle")
    )));
}

#[test]
fn full_scan_preserves_manifest_when_root_identity_is_unavailable() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("keep.wav"), b"keep").unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();

    let _root_identity = force_directory_identity(dir.path(), None);
    let ScanError::Incomplete { .. } = scan_once(&db).unwrap_err() else {
        panic!("an unavailable root identity must return an incomplete scan");
    };
    assert_eq!(db.count_files().unwrap(), 1);
}

#[test]
fn scan_skips_symlink_files() {
    use std::os::unix::fs as unix_fs;

    let dir = tempdir().unwrap();
    let target = dir.path().join("one.wav");
    std::fs::write(&target, b"one").unwrap();
    let link = dir.path().join("one_link.wav");
    unix_fs::symlink(&target, &link).unwrap();

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    let stats = scan_once(&db).unwrap();
    assert_eq!(stats.total_files, 1);
    assert_eq!(stats.added, 1);
    assert!(db.list_source_index_entries().unwrap().is_empty());
}

#[test]
fn scan_skips_appledouble_sidecar_files() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("kick.wav"), b"kick").unwrap();
    std::fs::write(dir.path().join("._kick.wav"), b"sidecar").unwrap();
    let nested = dir.path().join("drums");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(nested.join("snare.wav"), b"snare").unwrap();
    std::fs::write(nested.join("._snare.wav"), b"sidecar").unwrap();

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    let stats = scan_once(&db).unwrap();
    let paths = db
        .list_files()
        .unwrap()
        .into_iter()
        .map(|entry| entry.relative_path)
        .collect::<Vec<_>>();

    assert_eq!(stats.total_files, 2);
    assert_eq!(stats.added, 2);
    assert_eq!(
        paths,
        vec![PathBuf::from("drums/snare.wav"), PathBuf::from("kick.wav")]
    );
    assert!(db.list_source_index_entries().unwrap().is_empty());
}

#[test]
fn full_scan_captures_browser_layout_in_the_authoritative_traversal() {
    let dir = tempdir().unwrap();
    let nested = dir.path().join("nested");
    let empty = dir.path().join("empty");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::create_dir_all(&empty).unwrap();
    std::fs::write(nested.join("kick.wav"), b"kick").unwrap();
    std::fs::write(nested.join("notes.txt"), b"notes").unwrap();

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    let stats = scan_once(&db).unwrap();
    let snapshot = stats.source_tree_snapshot.expect("source tree snapshot");

    assert!(snapshot.is_complete());
    assert!(snapshot.directories.contains(&PathBuf::new()));
    assert!(snapshot.directories.contains(&PathBuf::from("empty")));
    assert!(snapshot.directories.contains(&PathBuf::from("nested")));
    let notes = snapshot
        .other_files
        .iter()
        .find(|file| file.relative_path == Path::new("nested/notes.txt"))
        .expect("notes layout entry");
    assert_eq!(notes.file_size, 5);
}

#[test]
fn browser_layout_includes_hidden_entries_by_default() {
    let dir = tempdir().unwrap();
    let hidden = dir.path().join(".hidden");
    std::fs::create_dir_all(&hidden).unwrap();
    std::fs::write(hidden.join("kick.wav"), b"kick").unwrap();
    std::fs::write(hidden.join("notes.txt"), b"notes").unwrap();

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    let stats = scan_once(&db).unwrap();
    let snapshot = stats.source_tree_snapshot.expect("source tree snapshot");

    assert!(snapshot.directories.contains(&PathBuf::from(".hidden")));
    assert!(snapshot.other_files.iter().any(|file| {
        file.relative_path == Path::new(".hidden/notes.txt") && file.file_size == 5
    }));
    assert!(
        db.entry_for_path(Path::new(".hidden/kick.wav"))
            .unwrap()
            .is_some()
    );
}

#[test]
fn configured_hidden_directory_exclusion_removes_hidden_browser_entries() {
    let dir = tempdir().unwrap();
    let hidden = dir.path().join(".hidden");
    std::fs::create_dir_all(&hidden).unwrap();
    std::fs::write(hidden.join("kick.wav"), b"kick").unwrap();
    std::fs::write(hidden.join("notes.txt"), b"notes").unwrap();

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    db.set_source_traversal_policy(
        wavecrate_library::sample_sources::SourceTraversalPolicy::exclude_hidden_directories(),
    )
    .unwrap();
    let stats = scan_once(&db).unwrap();
    let snapshot = stats.source_tree_snapshot.expect("source tree snapshot");

    assert!(!snapshot.directories.contains(&PathBuf::from(".hidden")));
    assert!(
        snapshot
            .other_files
            .iter()
            .all(|file| !file.relative_path.starts_with(".hidden"))
    );
    assert!(db.list_files().unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn browser_layout_snapshot_does_not_include_symlink_entries() {
    use std::os::unix::fs as unix_fs;

    let dir = tempdir().unwrap();
    let outside = tempdir().unwrap();
    std::fs::write(outside.path().join("outside.txt"), b"outside").unwrap();
    unix_fs::symlink(outside.path(), dir.path().join("outside-link")).unwrap();
    std::fs::write(dir.path().join("real.txt"), b"real").unwrap();
    unix_fs::symlink(
        dir.path().join("real.txt"),
        dir.path().join("file-link.txt"),
    )
    .unwrap();

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    let snapshot = scan_once(&db)
        .unwrap()
        .source_tree_snapshot
        .expect("source tree snapshot");

    assert!(snapshot.directories.contains(&PathBuf::new()));
    assert!(
        snapshot
            .directories
            .iter()
            .all(|path| path != Path::new("outside-link"))
    );
    assert!(
        snapshot
            .other_files
            .iter()
            .any(|file| file.relative_path == Path::new("real.txt"))
    );
    assert!(snapshot.other_files.iter().all(|file| {
        file.relative_path != Path::new("file-link.txt")
            && file.relative_path != Path::new("outside-link/outside.txt")
    }));
}
