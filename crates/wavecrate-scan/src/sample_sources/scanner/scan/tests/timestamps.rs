use super::*;
use crate::sample_sources::scanner::sync_paths;
use std::path::Path;

#[cfg(any(unix, windows))]
#[test]
fn pre_epoch_timestamps_persist_for_full_rescan_and_targeted_sync() {
    let directory = tempdir().unwrap();
    let supported = Path::new("supported.wav");
    let index_only = Path::new("notes.txt");
    let supported_path = directory.path().join(supported);
    let index_only_path = directory.path().join(index_only);
    std::fs::write(&supported_path, b"wav").unwrap();
    std::fs::write(&index_only_path, b"notes").unwrap();

    let initial_mtime = filetime::FileTime::from_unix_time(-1, 0);
    filetime::set_file_mtime(&supported_path, initial_mtime).unwrap();
    filetime::set_file_mtime(&index_only_path, initial_mtime).unwrap();

    let database = SourceDatabase::open_for_scan(directory.path()).unwrap();
    scan_once(&database).unwrap();
    assert_persisted_timestamp(&database, supported, index_only, -1_000_000_000);

    let no_change = scan_once(&database).unwrap();
    assert_eq!(no_change.content_changed, 0);
    assert_persisted_timestamp(&database, supported, index_only, -1_000_000_000);

    let targeted_mtime = filetime::FileTime::from_unix_time(-2, 0);
    filetime::set_file_mtime(&supported_path, targeted_mtime).unwrap();
    filetime::set_file_mtime(&index_only_path, targeted_mtime).unwrap();
    sync_paths(
        &database,
        &[supported.to_path_buf(), index_only.to_path_buf()],
    )
    .unwrap();
    assert_persisted_timestamp(&database, supported, index_only, -2_000_000_000);
}

#[cfg(any(unix, windows))]
fn assert_persisted_timestamp(
    database: &SourceDatabase,
    supported: &Path,
    index_only: &Path,
    expected_modified_ns: i64,
) {
    assert_eq!(
        database
            .entry_for_path(supported)
            .unwrap()
            .expect("supported file manifest entry")
            .modified_ns,
        expected_modified_ns
    );
    assert_eq!(
        database
            .list_source_index_entries()
            .unwrap()
            .into_iter()
            .find(|entry| entry.relative_path == index_only)
            .expect("index-only file entry")
            .modified_ns,
        Some(expected_modified_ns)
    );
}

#[cfg(not(any(unix, windows)))]
#[test]
fn pre_epoch_filetime_integration_is_explicitly_unsupported() {
    assert!(!cfg!(unix));
}
