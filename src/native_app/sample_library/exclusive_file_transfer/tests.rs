use super::*;
use tempfile::tempdir;

#[test]
fn copy_commit_preserves_an_existing_destination() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("source.wav");
    let destination = temp.path().join("destination.wav");
    fs::write(&source, b"source").unwrap();
    fs::write(&destination, b"existing").unwrap();

    let error = copy_file_no_replace(&source, &destination).unwrap_err();

    assert_eq!(error.kind(), ErrorKind::AlreadyExists);
    assert_eq!(fs::read(&source).unwrap(), b"source");
    assert_eq!(fs::read(&destination).unwrap(), b"existing");
}

#[test]
fn unique_copy_retries_a_collision_injected_before_commit() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("source.wav");
    let destination = temp.path().join("destination.wav");
    fs::write(&source, b"source").unwrap();

    let committed =
        copy_file_to_unique_destination_with(&source, &destination, |index, candidate| {
            if index == 0 {
                fs::write(candidate, b"late owner").unwrap();
            }
        })
        .unwrap();

    assert_eq!(
        committed.path(),
        temp.path().join("destination_copy001.wav")
    );
    assert_eq!(fs::read(&destination).unwrap(), b"late owner");
    assert_eq!(fs::read(committed.path()).unwrap(), b"source");
}

#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "linux",
    target_os = "android"
))]
#[test]
fn same_filesystem_move_preserves_an_existing_destination() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("source.wav");
    let destination = temp.path().join("destination.wav");
    fs::write(&source, b"source").unwrap();
    fs::write(&destination, b"late owner").unwrap();

    let error = move_file_no_replace(&source, &destination).unwrap_err();

    assert_eq!(error.kind(), ErrorKind::AlreadyExists);
    assert_eq!(fs::read(&source).unwrap(), b"source");
    assert_eq!(fs::read(&destination).unwrap(), b"late owner");
}

#[test]
fn ownership_cleanup_preserves_a_replaced_destination() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("source.wav");
    let destination = temp.path().join("destination.wav");
    fs::write(&source, b"source").unwrap();
    let committed = copy_file_no_replace(&source, &destination).unwrap();
    fs::remove_file(&destination).unwrap();
    fs::write(&destination, b"replacement").unwrap();

    assert!(!committed.remove_if_owned().unwrap());
    assert_eq!(fs::read(&destination).unwrap(), b"replacement");
}

#[test]
fn ownership_cleanup_preserves_replacement_after_the_initial_check() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("source.wav");
    let destination = temp.path().join("destination.wav");
    fs::write(&source, b"source").unwrap();
    let committed = copy_file_no_replace(&source, &destination).unwrap();

    let removed = committed
        .remove_if_owned_with(|| {
            fs::remove_file(&destination).unwrap();
            fs::write(&destination, b"late replacement").unwrap();
        })
        .unwrap();

    assert!(!removed);
    assert_eq!(fs::read(&destination).unwrap(), b"late replacement");
    assert!(fs::read_dir(temp.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".wavecrate-cleanup-")
    }));
}

#[test]
fn ownership_cleanup_preserves_a_replacement_directory_after_the_initial_check() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("source.wav");
    let destination = temp.path().join("destination.wav");
    fs::write(&source, b"source").unwrap();
    let committed = copy_file_no_replace(&source, &destination).unwrap();

    let removed = committed
        .remove_if_owned_with(|| {
            fs::remove_file(&destination).unwrap();
            fs::create_dir(&destination).unwrap();
            fs::write(destination.join("unrelated.wav"), b"unrelated").unwrap();
        })
        .unwrap();

    assert!(!removed);
    assert_eq!(
        fs::read(destination.join("unrelated.wav")).unwrap(),
        b"unrelated"
    );
    assert!(fs::read_dir(temp.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".wavecrate-cleanup-")
    }));
}

#[cfg(unix)]
#[test]
fn ownership_cleanup_preserves_a_replacement_symlink_to_the_owned_file() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let source = temp.path().join("source.wav");
    let destination = temp.path().join("destination.wav");
    let owned_alias = temp.path().join("owned-alias.wav");
    fs::write(&source, b"source").unwrap();
    let committed = copy_file_no_replace(&source, &destination).unwrap();
    fs::hard_link(&destination, &owned_alias).unwrap();

    let removed = committed
        .remove_if_owned_with(|| {
            fs::remove_file(&destination).unwrap();
            symlink(&owned_alias, &destination).unwrap();
        })
        .unwrap();

    assert!(!removed);
    assert!(
        fs::symlink_metadata(&destination)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read(&destination).unwrap(), b"source");
}

#[cfg(unix)]
#[test]
fn native_move_rejects_a_symlink_source_without_moving_its_target() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let target = temp.path().join("target.wav");
    let source = temp.path().join("source.wav");
    let destination = temp.path().join("destination.wav");
    fs::write(&target, b"target").unwrap();
    symlink(&target, &source).unwrap();

    let error = move_file_no_replace(&source, &destination).unwrap_err();

    assert_eq!(error.kind(), ErrorKind::InvalidInput);
    assert!(
        fs::symlink_metadata(&source)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read(&target).unwrap(), b"target");
    assert!(!destination.exists());
}

#[test]
fn cross_device_fallback_preserves_a_late_destination() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("source.wav");
    let destination = temp.path().join("destination.wav");
    fs::write(&source, b"source").unwrap();
    fs::write(&destination, b"late owner").unwrap();

    let error = move_file_after_rename_error(
        &source,
        &destination,
        io::Error::from(ErrorKind::CrossesDevices),
    )
    .unwrap_err();

    assert_eq!(error.kind(), ErrorKind::AlreadyExists);
    assert_eq!(fs::read(&source).unwrap(), b"source");
    assert_eq!(fs::read(&destination).unwrap(), b"late owner");
}

#[test]
fn cross_device_fallback_commits_before_removing_the_source() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("source.wav");
    let destination = temp.path().join("destination.wav");
    fs::write(&source, b"source").unwrap();

    let committed = move_file_after_rename_error(
        &source,
        &destination,
        io::Error::from(ErrorKind::CrossesDevices),
    )
    .unwrap();

    assert_eq!(committed.path(), destination);
    assert!(!source.exists());
    assert_eq!(fs::read(destination).unwrap(), b"source");
}

#[test]
fn rename_fallback_accepts_unconditional_error_kinds() {
    assert!(rename_requires_copy_fallback(&io::Error::from(
        ErrorKind::CrossesDevices,
    )));
    assert!(rename_requires_copy_fallback(&io::Error::from(
        ErrorKind::Unsupported,
    )));
}

#[cfg(any(target_os = "linux", target_os = "android"))]
#[test]
fn rename_fallback_accepts_linux_raw_errnos() {
    for errno in [libc::ENOSYS, libc::EINVAL, libc::EOPNOTSUPP] {
        let error = io::Error::from_raw_os_error(errno);
        assert!(rename_requires_copy_fallback(&error));
    }
}

#[test]
fn rename_fallback_rejects_unrelated_errors() {
    assert!(!rename_requires_copy_fallback(&io::Error::from(
        ErrorKind::AlreadyExists,
    )));
    assert!(!rename_requires_copy_fallback(&io::Error::from(
        ErrorKind::PermissionDenied,
    )));
    let unrelated_raw_error = io::Error::from_raw_os_error(i32::MAX);
    assert!(!rename_requires_copy_fallback(&unrelated_raw_error));
}
