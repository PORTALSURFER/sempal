//! Stable filesystem-object identity helpers.

use std::{fs, path::Path};

/// Return a stable platform identity for the filesystem object described by `metadata`.
///
/// Callers should obtain `metadata` with either [`fs::metadata`] or
/// [`fs::symlink_metadata`] according to whether symlinks should be followed. The helper
/// preserves that choice when opening the object on Windows. Identity lookup is best-effort;
/// unsupported platforms and filesystem or permission failures return `None`.
pub fn stable_filesystem_identity(path: &Path, metadata: &fs::Metadata) -> Option<String> {
    stable_filesystem_identity_impl(path, metadata)
}

/// Return a stable identity for an object represented by an already-open file descriptor.
///
/// Directory traversal uses this form so identity is derived from the descriptor that was
/// opened with the scanner's no-follow capability, rather than from a second path lookup.
/// Unsupported platforms and identity lookup failures return `None`.
pub fn stable_filesystem_identity_from_open_file(file: &fs::File) -> Option<String> {
    stable_filesystem_identity_from_open_file_impl(file)
}

/// Return a platform marker that changes when the filesystem object is mutated.
///
/// This is intentionally separate from modified time because callers use it to fence
/// content reads even when another process restores the user-visible modified time.
/// Unsupported filesystems and lookup failures return `None`; callers must not treat a
/// missing marker as proof that a content snapshot remained stable.
pub fn filesystem_change_marker(path: &Path, metadata: &fs::Metadata) -> Option<String> {
    filesystem_change_marker_impl(path, metadata)
}

#[cfg(unix)]
fn stable_filesystem_identity_impl(_path: &Path, metadata: &fs::Metadata) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    use std::time::UNIX_EPOCH;

    // Device and inode alone are not sufficient: filesystems may immediately reuse an
    // inode after deletion. The creation timestamp remains stable across a rename and
    // distinguishes a replacement that inherits the same device/inode pair.
    let created = metadata.created().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some(format!(
        "unix:{}:{}:{}",
        metadata.dev(),
        metadata.ino(),
        created.as_nanos()
    ))
}

#[cfg(unix)]
fn stable_filesystem_identity_from_open_file_impl(file: &fs::File) -> Option<String> {
    let metadata = file.metadata().ok()?;
    stable_filesystem_identity_impl(Path::new(""), &metadata)
}

#[cfg(unix)]
fn filesystem_change_marker_impl(_path: &Path, metadata: &fs::Metadata) -> Option<String> {
    use std::os::unix::fs::MetadataExt;

    Some(format!(
        "unix:{}:{}",
        metadata.ctime(),
        metadata.ctime_nsec()
    ))
}

#[cfg(windows)]
fn stable_filesystem_identity_impl(path: &Path, metadata: &fs::Metadata) -> Option<String> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows::Win32::Storage::FileSystem::{
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE,
    };

    let mut options = fs::OpenOptions::new();
    options
        .access_mode(FILE_READ_ATTRIBUTES.0)
        .share_mode((FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE).0);
    if metadata.file_type().is_symlink() {
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0);
    }

    let file = options.open(path).ok()?;
    stable_windows_identity(&file)
}

#[cfg(windows)]
fn stable_filesystem_identity_from_open_file_impl(file: &fs::File) -> Option<String> {
    stable_windows_identity(file)
}

#[cfg(windows)]
fn stable_windows_identity(file: &fs::File) -> Option<String> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle},
    };

    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut information) }.ok()?;
    let file_index =
        (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow);
    let creation_time = (u64::from(information.ftCreationTime.dwHighDateTime) << 32)
        | u64::from(information.ftCreationTime.dwLowDateTime);
    Some(format!(
        "windows:{}:{}:{}",
        information.dwVolumeSerialNumber, file_index, creation_time
    ))
}

#[cfg(windows)]
fn filesystem_change_marker_impl(path: &Path, metadata: &fs::Metadata) -> Option<String> {
    use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
    use windows::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{
            FILE_BASIC_INFO, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
            FILE_SHARE_READ, FILE_SHARE_WRITE, FileBasicInfo, GetFileInformationByHandleEx,
        },
    };

    let mut options = fs::OpenOptions::new();
    options
        .access_mode(FILE_READ_ATTRIBUTES.0)
        .share_mode((FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE).0);
    if metadata.file_type().is_symlink() {
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0);
    }

    let file = options.open(path).ok()?;
    let mut information = FILE_BASIC_INFO::default();
    unsafe {
        GetFileInformationByHandleEx(
            HANDLE(file.as_raw_handle()),
            FileBasicInfo,
            (&mut information as *mut FILE_BASIC_INFO).cast(),
            std::mem::size_of::<FILE_BASIC_INFO>() as u32,
        )
    }
    .ok()?;
    (information.ChangeTime != 0).then(|| format!("windows:{}", information.ChangeTime))
}

#[cfg(not(any(unix, windows)))]
fn stable_filesystem_identity_impl(_path: &Path, _metadata: &fs::Metadata) -> Option<String> {
    None
}

#[cfg(not(any(unix, windows)))]
fn stable_filesystem_identity_from_open_file_impl(_file: &fs::File) -> Option<String> {
    None
}

#[cfg(not(any(unix, windows)))]
fn filesystem_change_marker_impl(_path: &Path, _metadata: &fs::Metadata) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hard_links_share_identity_but_distinct_files_do_not() {
        let temp = tempfile::tempdir().expect("create identity fixture");
        let original = temp.path().join("original.wav");
        let linked = temp.path().join("linked.wav");
        let distinct = temp.path().join("distinct.wav");
        fs::write(&original, b"original").expect("write original fixture");
        fs::hard_link(&original, &linked).expect("create hard-link fixture");
        fs::write(&distinct, b"distinct").expect("write distinct fixture");

        let identity = |path: &Path| {
            let metadata = fs::symlink_metadata(path).expect("read fixture metadata");
            stable_filesystem_identity(path, &metadata).expect("read fixture identity")
        };

        assert_eq!(identity(&original), identity(&linked));
        assert_ne!(identity(&original), identity(&distinct));
    }

    #[test]
    fn rename_preserves_identity() {
        let temp = tempfile::tempdir().expect("create identity fixture");
        let original = temp.path().join("original.wav");
        let renamed = temp.path().join("renamed.wav");
        fs::write(&original, b"original").expect("write original fixture");

        let original_metadata = fs::symlink_metadata(&original).expect("read original metadata");
        let original_identity = stable_filesystem_identity(&original, &original_metadata)
            .expect("read original identity");
        fs::rename(&original, &renamed).expect("rename fixture");
        let renamed_metadata = fs::symlink_metadata(&renamed).expect("read renamed metadata");
        let renamed_identity =
            stable_filesystem_identity(&renamed, &renamed_metadata).expect("read renamed identity");

        assert_eq!(original_identity, renamed_identity);
    }
}
