use std::path::{Path, PathBuf};

use wavecrate_library::sample_sources::{
    SOURCE_FORMAT_POLICY_VERSION, SourceIndexClassification, SourceIndexDiagnostic,
    SourceIndexEntry,
};

use super::*;
use crate::sample_sources::scanner::scan_fs::{
    force_file_metadata_failure, force_file_type_failure,
};
use crate::sample_sources::scanner::sync_paths;

#[test]
fn full_scan_persists_typed_index_only_entries_across_restart() {
    let directory = tempdir().unwrap();
    std::fs::write(directory.path().join("supported.wav"), b"wav").unwrap();
    std::fs::write(directory.path().join("unsupported.flac"), b"flac").unwrap();
    std::fs::write(directory.path().join("notes.txt"), b"notes").unwrap();
    std::fs::write(directory.path().join("._sidecar.flac"), b"sidecar").unwrap();

    let database = SourceDatabase::open_for_scan(directory.path()).unwrap();
    scan_once(&database).unwrap();
    assert_eq!(database.list_files().unwrap().len(), 1);
    assert_eq!(
        typed_paths(&database),
        vec![
            (
                PathBuf::from("notes.txt"),
                SourceIndexClassification::UnsupportedNonAudio,
            ),
            (
                PathBuf::from("unsupported.flac"),
                SourceIndexClassification::UnsupportedAudio,
            ),
        ]
    );
    assert!(
        database
            .set_tag(Path::new("notes.txt"), Rating::KEEP_1)
            .is_err()
    );
    drop(database);

    let reopened = SourceDatabase::open_for_scan(directory.path()).unwrap();
    assert_eq!(
        typed_paths(&reopened),
        vec![
            (
                PathBuf::from("notes.txt"),
                SourceIndexClassification::UnsupportedNonAudio,
            ),
            (
                PathBuf::from("unsupported.flac"),
                SourceIndexClassification::UnsupportedAudio,
            ),
        ]
    );
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn non_unicode_supported_paths_are_isolated_and_converge_in_full_and_targeted_scans() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let directory = tempdir().unwrap();
    let raw_name = OsString::from_vec(b"raw-\xFF.wav".to_vec());
    let renamed_name = OsString::from_vec(b"renamed-\xFE.wav".to_vec());
    let raw_path = PathBuf::from(&raw_name);
    let renamed_path = PathBuf::from(&renamed_name);
    std::fs::write(directory.path().join("ordinary.wav"), b"ordinary").unwrap();

    let database = SourceDatabase::open_for_scan(directory.path()).unwrap();
    scan_once(&database).unwrap();
    assert_eq!(database.list_files().unwrap().len(), 1);
    assert!(database.list_source_index_entries().unwrap().is_empty());

    std::fs::write(directory.path().join(&raw_name), b"raw").unwrap();
    sync_paths(&database, std::slice::from_ref(&raw_path)).unwrap();
    let entry = database.list_source_index_entries().unwrap().remove(0);
    assert_eq!(entry.relative_path, raw_path);
    assert_eq!(
        entry.classification,
        SourceIndexClassification::Inaccessible
    );
    assert_eq!(
        entry.diagnostic,
        Some(SourceIndexDiagnostic::NonUnicodePath)
    );
    assert_eq!(entry.file_size, Some(3));

    std::fs::write(directory.path().join(&raw_name), b"raw-modified").unwrap();
    sync_paths(&database, std::slice::from_ref(&raw_path)).unwrap();
    assert_eq!(
        database.list_source_index_entries().unwrap()[0].file_size,
        Some(12)
    );
    assert_eq!(database.list_files().unwrap().len(), 1);

    std::fs::rename(
        directory.path().join(&raw_name),
        directory.path().join(&renamed_name),
    )
    .unwrap();
    sync_paths(&database, &[raw_path.clone(), renamed_path.clone()]).unwrap();
    assert_eq!(
        database.list_source_index_entries().unwrap()[0].relative_path,
        renamed_path
    );

    std::fs::remove_file(directory.path().join(&renamed_name)).unwrap();
    sync_paths(&database, std::slice::from_ref(&renamed_path)).unwrap();
    assert!(database.list_source_index_entries().unwrap().is_empty());
    assert_eq!(database.list_files().unwrap().len(), 1);

    std::fs::write(directory.path().join(&raw_name), b"raw-again").unwrap();
    scan_once(&database).unwrap();
    assert_eq!(database.list_files().unwrap().len(), 1);
    assert_eq!(
        database.list_source_index_entries().unwrap()[0].relative_path,
        raw_path
    );
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn targeted_sync_preserves_non_unicode_unsupported_classifications() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let directory = tempdir().unwrap();
    let unsupported_audio = PathBuf::from(OsString::from_vec(b"raw-\xFF.flac".to_vec()));
    let unsupported_non_audio = PathBuf::from(OsString::from_vec(b"notes-\xFE.txt".to_vec()));
    std::fs::write(directory.path().join("ordinary.wav"), b"ordinary").unwrap();

    let database = SourceDatabase::open_for_scan(directory.path()).unwrap();
    scan_once(&database).unwrap();
    std::fs::write(directory.path().join(&unsupported_audio), b"audio").unwrap();
    std::fs::write(directory.path().join(&unsupported_non_audio), b"notes").unwrap();
    sync_paths(
        &database,
        &[unsupported_audio.clone(), unsupported_non_audio.clone()],
    )
    .unwrap();

    let entries = database.list_source_index_entries().unwrap();
    assert_eq!(entries.len(), 2);
    for (path, expected_classification) in [
        (
            unsupported_audio,
            SourceIndexClassification::UnsupportedAudio,
        ),
        (
            unsupported_non_audio,
            SourceIndexClassification::UnsupportedNonAudio,
        ),
    ] {
        let entry = entries
            .iter()
            .find(|entry| entry.relative_path == path)
            .expect("targeted non-Unicode index entry");
        assert_eq!(entry.classification, expected_classification);
        assert_eq!(entry.diagnostic, None);
    }
    assert_eq!(database.list_files().unwrap().len(), 1);
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn lossless_raw_path_key_does_not_alias_a_unicode_sample_path() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let directory = tempdir().unwrap();
    let raw_path = PathBuf::from_iter([
        OsString::from_vec(b"raw-\xFF".to_vec()),
        OsString::from("sample.wav"),
    ]);
    let unicode_path = PathBuf::from("~wavecrate-nu~7261772dff/sample.wav");
    std::fs::create_dir_all(directory.path().join(&raw_path).parent().unwrap()).unwrap();
    std::fs::create_dir_all(directory.path().join(&unicode_path).parent().unwrap()).unwrap();
    std::fs::write(directory.path().join(&raw_path), b"raw").unwrap();
    std::fs::write(directory.path().join(&unicode_path), b"unicode").unwrap();

    let database = SourceDatabase::open_for_scan(directory.path()).unwrap();
    scan_once(&database).unwrap();

    let manifest = database.list_manifest_entries().unwrap();
    assert_eq!(manifest.len(), 1);
    assert_eq!(manifest[0].relative_path, unicode_path);
    let index_entries = database.list_source_index_entries().unwrap();
    assert_eq!(index_entries.len(), 1);
    assert_eq!(index_entries[0].relative_path, raw_path);
    assert_eq!(
        index_entries[0].diagnostic,
        Some(SourceIndexDiagnostic::NonUnicodePath)
    );
}

#[test]
fn full_scan_reconciles_index_only_change_move_and_delete() {
    let directory = tempdir().unwrap();
    let original = directory.path().join("notes.txt");
    std::fs::write(&original, b"one").unwrap();
    let database = SourceDatabase::open_for_scan(directory.path()).unwrap();
    scan_once(&database).unwrap();
    let first = database.list_source_index_entries().unwrap().remove(0);

    std::fs::write(&original, b"longer").unwrap();
    scan_once(&database).unwrap();
    let changed = database.list_source_index_entries().unwrap().remove(0);
    assert_eq!(changed.file_size, Some(6));
    assert_eq!(changed.classification, first.classification);

    let moved = directory.path().join("moved.txt");
    std::fs::rename(&original, &moved).unwrap();
    scan_once(&database).unwrap();
    assert_eq!(
        typed_paths(&database),
        vec![(
            PathBuf::from("moved.txt"),
            SourceIndexClassification::UnsupportedNonAudio,
        )]
    );

    std::fs::remove_file(moved).unwrap();
    scan_once(&database).unwrap();
    assert!(database.list_source_index_entries().unwrap().is_empty());
}

#[test]
fn targeted_sync_uses_the_same_index_only_classification_and_reconciliation() {
    let directory = tempdir().unwrap();
    let nested = directory.path().join("nested");
    std::fs::create_dir(&nested).unwrap();
    let database = SourceDatabase::open_for_scan(directory.path()).unwrap();
    scan_once(&database).unwrap();

    std::fs::write(nested.join("loop.mp3"), b"mp3").unwrap();
    std::fs::write(nested.join("notes.md"), b"notes").unwrap();
    sync_paths(&database, &[PathBuf::from("nested")]).unwrap();
    assert_eq!(
        typed_paths(&database),
        vec![
            (
                PathBuf::from("nested/loop.mp3"),
                SourceIndexClassification::UnsupportedAudio,
            ),
            (
                PathBuf::from("nested/notes.md"),
                SourceIndexClassification::UnsupportedNonAudio,
            ),
        ]
    );

    std::fs::remove_file(nested.join("loop.mp3")).unwrap();
    std::fs::rename(nested.join("notes.md"), nested.join("moved.md")).unwrap();
    sync_paths(&database, &[PathBuf::from("nested")]).unwrap();
    let entries = database.list_source_index_entries().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].relative_path, Path::new("nested/moved.md"));

    std::fs::write(nested.join("moved.md"), b"longer").unwrap();
    sync_paths(&database, &[PathBuf::from("nested/moved.md")]).unwrap();
    let entries = database.list_source_index_entries().unwrap();
    assert_eq!(entries[0].file_size, Some(6));

    std::fs::remove_file(nested.join("moved.md")).unwrap();
    sync_paths(&database, &[PathBuf::from("nested/moved.md")]).unwrap();
    assert!(database.list_source_index_entries().unwrap().is_empty());
}

#[test]
fn targeted_index_delta_is_bound_to_the_next_source_revision() {
    let directory = tempdir().unwrap();
    let database = SourceDatabase::open_for_scan(directory.path()).unwrap();
    scan_once(&database).unwrap();
    let initial_revision = database.get_revision().unwrap();

    let path = directory.path().join("visible.flac");
    std::fs::write(&path, b"one").unwrap();
    let created = sync_paths(&database, &[PathBuf::from("visible.flac")]).unwrap();
    assert!(created.committed_delta.is_empty());
    assert_eq!(
        created.committed_source_index_delta.revision,
        initial_revision + 1
    );
    assert_eq!(
        created.committed_source_index_delta.revision,
        created.committed_delta.revision
    );
    assert_eq!(
        created.committed_delta.revision,
        database.get_revision().unwrap()
    );
    assert_eq!(
        created.committed_source_index_delta.upserted_entries.len(),
        1
    );
    assert!(
        created
            .committed_source_index_delta
            .removed_paths
            .is_empty()
    );

    std::fs::write(&path, b"updated").unwrap();
    let updated = sync_paths(&database, &[PathBuf::from("visible.flac")]).unwrap();
    assert!(updated.committed_delta.is_empty());
    assert_eq!(
        updated.committed_source_index_delta.revision,
        updated.committed_delta.revision
    );
    assert_eq!(
        updated.committed_source_index_delta.upserted_entries[0].file_size,
        Some(7)
    );

    std::fs::remove_file(path).unwrap();
    let deleted = sync_paths(&database, &[PathBuf::from("visible.flac")]).unwrap();
    assert!(deleted.committed_delta.is_empty());
    assert_eq!(
        deleted.committed_source_index_delta.revision,
        deleted.committed_delta.revision
    );
    assert_eq!(
        deleted.committed_source_index_delta.removed_paths,
        vec![PathBuf::from("visible.flac")]
    );
    assert!(
        deleted
            .committed_source_index_delta
            .upserted_entries
            .is_empty()
    );
}

#[test]
fn uncertain_subtree_does_not_false_delete_index_only_rows() {
    use crate::sample_sources::scanner::scan_fs::force_directory_read_failure;

    let directory = tempdir().unwrap();
    let protected = directory.path().join("protected");
    std::fs::create_dir(&protected).unwrap();
    std::fs::write(protected.join("notes.txt"), b"notes").unwrap();
    let database = SourceDatabase::open_for_scan(directory.path()).unwrap();
    scan_once(&database).unwrap();

    std::fs::remove_file(protected.join("notes.txt")).unwrap();
    let failure = force_directory_read_failure(&protected);
    assert!(matches!(
        scan_once(&database),
        Err(ScanError::Incomplete { .. })
    ));
    assert_eq!(
        database.list_source_index_entries().unwrap()[0].relative_path,
        Path::new("protected/notes.txt")
    );

    drop(failure);
    scan_once(&database).unwrap();
    assert!(database.list_source_index_entries().unwrap().is_empty());
}

#[test]
fn inaccessible_observation_is_typed_without_deleting_a_prior_index_row() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("notes.txt");
    std::fs::write(&path, b"notes").unwrap();
    let database = SourceDatabase::open_for_scan(directory.path()).unwrap();
    scan_once(&database).unwrap();
    assert_eq!(
        database.list_source_index_entries().unwrap()[0].classification,
        SourceIndexClassification::UnsupportedNonAudio
    );

    let failure = force_file_metadata_failure(&path);
    let ScanError::Incomplete { .. } = scan_once(&database).unwrap_err() else {
        panic!("unavailable metadata must leave a retryable scan");
    };
    let inaccessible = database.list_source_index_entries().unwrap().remove(0);
    assert_eq!(
        inaccessible.classification,
        SourceIndexClassification::Inaccessible
    );
    assert_eq!(
        inaccessible.diagnostic,
        Some(SourceIndexDiagnostic::MetadataUnavailable)
    );

    drop(failure);
    scan_once(&database).unwrap();
    let recovered = database.list_source_index_entries().unwrap().remove(0);
    assert_eq!(
        recovered.classification,
        SourceIndexClassification::UnsupportedNonAudio
    );
    assert_eq!(recovered.diagnostic, None);
}

#[test]
fn full_scan_moves_an_inaccessible_supported_file_out_of_the_live_manifest_until_recovery() {
    let directory = tempdir().unwrap();
    let relative = Path::new("supported.wav");
    let path = directory.path().join(relative);
    std::fs::write(&path, b"wav").unwrap();
    let database = SourceDatabase::open_for_scan(directory.path()).unwrap();
    scan_once(&database).unwrap();
    database.set_tag(relative, Rating::KEEP_1).unwrap();

    let failure = force_file_metadata_failure(&path);
    let ScanError::Incomplete { .. } = scan_once(&database).unwrap_err() else {
        panic!("inaccessible supported audio must leave a retryable scan");
    };
    let unavailable = database.entry_for_path(relative).unwrap().unwrap();
    assert!(unavailable.missing);
    assert_eq!(unavailable.tag, Rating::KEEP_1);
    assert_eq!(
        database.list_source_index_entries().unwrap(),
        vec![SourceIndexEntry {
            relative_path: relative.to_path_buf(),
            classification: SourceIndexClassification::Inaccessible,
            file_size: None,
            modified_ns: None,
            file_identity: None,
            diagnostic: Some(SourceIndexDiagnostic::MetadataUnavailable),
            format_policy_version: SOURCE_FORMAT_POLICY_VERSION,
        }]
    );

    drop(failure);
    scan_once(&database).unwrap();
    let recovered = database.entry_for_path(relative).unwrap().unwrap();
    assert!(!recovered.missing);
    assert_eq!(recovered.tag, Rating::KEEP_1);
    assert!(database.list_source_index_entries().unwrap().is_empty());
}

#[test]
fn targeted_sync_persists_and_recovers_a_supported_file_availability_diagnostic() {
    let directory = tempdir().unwrap();
    let relative = Path::new("supported.wav");
    let path = directory.path().join(relative);
    std::fs::write(&path, b"wav").unwrap();
    let database = SourceDatabase::open_for_scan(directory.path()).unwrap();
    scan_once(&database).unwrap();

    let failure = force_file_metadata_failure(&path);
    let ScanError::Incomplete { .. } =
        sync_paths(&database, &[relative.to_path_buf()]).unwrap_err()
    else {
        panic!("targeted unavailability must leave a retryable scan");
    };
    assert!(database.entry_for_path(relative).unwrap().unwrap().missing);
    assert_eq!(
        database.list_source_index_entries().unwrap()[0].classification,
        SourceIndexClassification::Inaccessible
    );

    drop(failure);
    sync_paths(&database, &[relative.to_path_buf()]).unwrap();
    assert!(!database.entry_for_path(relative).unwrap().unwrap().missing);
    assert!(database.list_source_index_entries().unwrap().is_empty());
}

#[test]
fn failed_type_probes_never_persist_internal_database_or_appledouble_paths() {
    let directory = tempdir().unwrap();
    let sidecar_relative = PathBuf::from("._sidecar.flac");
    let database_relative = PathBuf::from(".wavecrate.db");
    let sidecar = directory.path().join(&sidecar_relative);
    let database_path = directory.path().join(&database_relative);
    std::fs::write(&sidecar, b"sidecar").unwrap();
    let database = SourceDatabase::open_for_scan(directory.path()).unwrap();

    let full_sidecar_failure = force_file_type_failure(&sidecar);
    let full_database_failure = force_file_type_failure(&database_path);
    assert!(matches!(
        scan_once(&database),
        Err(ScanError::Incomplete { .. })
    ));
    assert!(database.list_source_index_entries().unwrap().is_empty());
    drop((full_sidecar_failure, full_database_failure));

    let targeted_sidecar_failure = force_file_type_failure(&sidecar);
    let targeted_database_failure = force_file_type_failure(&database_path);
    assert!(matches!(
        sync_paths(
            &database,
            &[sidecar_relative.clone(), database_relative.clone()],
        ),
        Err(ScanError::Incomplete { .. })
    ));
    assert!(database.list_source_index_entries().unwrap().is_empty());
    drop((targeted_sidecar_failure, targeted_database_failure));
}

#[test]
fn failed_type_probes_preserve_descendants_below_reserved_name_directories() {
    let directory = tempdir().unwrap();
    let reserved_directories = [
        PathBuf::from("nested/.wavecrate.db"),
        PathBuf::from("._assets"),
    ];
    for reserved in &reserved_directories {
        std::fs::create_dir_all(directory.path().join(reserved)).unwrap();
        std::fs::write(directory.path().join(reserved).join("sample.wav"), b"wav").unwrap();
        std::fs::write(directory.path().join(reserved).join("notes.txt"), b"notes").unwrap();
    }
    let database = SourceDatabase::open_for_scan(directory.path()).unwrap();
    scan_once(&database).unwrap();
    let expected_manifest = database.list_manifest_entries().unwrap();
    let expected_index = database.list_source_index_entries().unwrap();

    let full_failures = reserved_directories
        .iter()
        .map(|path| force_file_type_failure(&directory.path().join(path)))
        .collect::<Vec<_>>();
    assert!(matches!(
        scan_once(&database),
        Err(ScanError::Incomplete { .. })
    ));
    assert_eq!(database.list_manifest_entries().unwrap(), expected_manifest);
    assert_eq!(
        database.list_source_index_entries().unwrap(),
        expected_index
    );
    drop(full_failures);

    let targeted_failures = reserved_directories
        .iter()
        .map(|path| force_file_type_failure(&directory.path().join(path)))
        .collect::<Vec<_>>();
    assert!(matches!(
        sync_paths(&database, &reserved_directories),
        Err(ScanError::Incomplete { .. })
    ));
    assert_eq!(database.list_manifest_entries().unwrap(), expected_manifest);
    assert_eq!(
        database.list_source_index_entries().unwrap(),
        expected_index
    );
    drop(targeted_failures);
}

#[test]
fn supported_scan_promotes_a_legacy_index_only_row_without_metadata_inheritance() {
    let directory = tempdir().unwrap();
    let path = Path::new("promoted.wav");
    std::fs::write(directory.path().join(path), b"sample").unwrap();
    let database = SourceDatabase::open_for_scan(directory.path()).unwrap();
    let mut batch = database.write_batch().unwrap();
    batch
        .upsert_source_index_entry(&SourceIndexEntry {
            relative_path: path.to_path_buf(),
            classification: SourceIndexClassification::UnsupportedAudio,
            file_size: Some(6),
            modified_ns: Some(1),
            file_identity: None,
            diagnostic: None,
            format_policy_version: SOURCE_FORMAT_POLICY_VERSION.saturating_sub(1),
        })
        .unwrap();
    batch.commit_auxiliary_state().unwrap();

    scan_once(&database).unwrap();

    assert!(database.list_source_index_entries().unwrap().is_empty());
    let promoted = database.entry_for_path(path).unwrap().unwrap();
    assert_eq!(promoted.tag, Rating::NEUTRAL);
    assert!(!promoted.looped);
    assert!(!promoted.locked);
    assert!(promoted.normal_tags.is_empty());
}

fn typed_paths(database: &SourceDatabase) -> Vec<(PathBuf, SourceIndexClassification)> {
    database
        .list_source_index_entries()
        .unwrap()
        .into_iter()
        .map(|entry| (entry.relative_path, entry.classification))
        .collect()
}
