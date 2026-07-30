use std::{
    collections::hash_map::DefaultHasher,
    fs::{self, File, OpenOptions},
    hash::{Hash, Hasher},
    io,
    path::{Path, PathBuf},
};
use wavecrate::sample_sources::{SourceFileEvidence, capture_source_file_evidence};

#[derive(Clone, Debug, PartialEq)]
pub(in crate::native_app) struct TrashMoveOutcome {
    pub source: PathBuf,
    pub result: TrashMoveResult,
    /// Evidence captured immediately after the worker's filesystem attempt.
    pub evidence: SourceFileEvidence,
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::native_app) enum TrashMoveResult {
    Moved { destination: PathBuf },
    Missing,
    Failed { error: String },
}

pub(super) fn move_paths_to_configured_trash(
    paths: &[PathBuf],
    trash_folder: Option<&Path>,
) -> Vec<TrashMoveOutcome> {
    paths
        .iter()
        .map(|path| move_path_to_configured_trash(path, trash_folder))
        .collect()
}

pub(super) fn move_path_to_configured_trash(
    path: &Path,
    trash_folder: Option<&Path>,
) -> TrashMoveOutcome {
    let source_path = path.to_path_buf();
    let result = move_path_to_configured_trash_inner(path, trash_folder);
    let evidence = capture_source_file_evidence(&source_path);
    TrashMoveOutcome {
        source: source_path,
        result,
        evidence,
    }
}

fn move_path_to_configured_trash_inner(
    path: &Path,
    trash_folder: Option<&Path>,
) -> TrashMoveResult {
    match path_entry_exists(path) {
        Ok(true) => {}
        Ok(false) => return TrashMoveResult::Missing,
        Err(error) => {
            return TrashMoveResult::Failed {
                error: format!("Trash source is unavailable: {error}"),
            };
        }
    }
    let moved = (|| {
        let trash_folder = trash_folder.ok_or_else(|| {
            String::from("Set a trash folder in Settings > General before deleting files")
        })?;
        fs::create_dir_all(trash_folder)
            .map_err(|err| format!("Create trash folder failed: {err}"))?;
        let trash_folder = trash_folder
            .canonicalize()
            .map_err(|err| format!("Trash folder is unavailable: {err}"))?;
        let source = path
            .canonicalize()
            .map_err(|err| format!("Trash source is unavailable: {err}"))?;
        if source.starts_with(&trash_folder) {
            return Err(String::from(
                "Selected item is already inside the trash folder",
            ));
        }
        if trash_folder.starts_with(&source) {
            return Err(String::from(
                "Trash folder cannot be inside the item being deleted",
            ));
        }
        let reservation = reserve_trash_path(&trash_folder, &source)?;
        move_path(&source, reservation.destination())?;
        Ok(reservation.destination().to_path_buf())
    })();
    match moved {
        Ok(destination) => TrashMoveResult::Moved { destination },
        Err(error) => TrashMoveResult::Failed { error },
    }
}

struct TrashDestinationReservation {
    destination: PathBuf,
    marker: PathBuf,
    file: Option<File>,
}

impl TrashDestinationReservation {
    fn destination(&self) -> &Path {
        &self.destination
    }
}

impl Drop for TrashDestinationReservation {
    fn drop(&mut self) {
        drop(self.file.take());
        let _ = fs::remove_file(&self.marker);
    }
}

fn reserve_trash_path(
    trash_folder: &Path,
    source: &Path,
) -> Result<TrashDestinationReservation, String> {
    let file_name = source
        .file_name()
        .ok_or_else(|| format!("Trash move failed: {} has no file name", source.display()))?;
    let stem = source
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| file_name.to_string_lossy().to_string());
    let extension = source.extension().map(|extension| extension.to_os_string());
    for index in 1..10_000 {
        let name = if index == 1 {
            file_name.to_os_string()
        } else {
            let mut name = format!("{stem} {index}");
            if let Some(extension) = &extension {
                name.push('.');
                name.push_str(&extension.to_string_lossy());
            }
            name.into()
        };
        let candidate = trash_folder.join(name);
        match path_entry_exists(&candidate) {
            Ok(true) => continue,
            Ok(false) => {}
            Err(error) => {
                return Err(format!(
                    "Inspect trash destination {} failed: {error}",
                    candidate.display()
                ));
            }
        }
        let marker = trash_reservation_marker(trash_folder, &candidate);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&marker)
        {
            Ok(file) => {
                match path_entry_exists(&candidate) {
                    Ok(true) => {
                        drop(file);
                        let _ = fs::remove_file(marker);
                        continue;
                    }
                    Ok(false) => {}
                    Err(error) => {
                        drop(file);
                        let _ = fs::remove_file(marker);
                        return Err(format!(
                            "Inspect trash destination {} failed: {error}",
                            candidate.display()
                        ));
                    }
                }
                return Ok(TrashDestinationReservation {
                    destination: candidate,
                    marker,
                    file: Some(file),
                });
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("Reserve trash destination failed: {error}")),
        }
    }
    Err(String::from(
        "Trash folder contains too many matching names",
    ))
}

fn path_entry_exists(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn trash_reservation_marker(trash_folder: &Path, candidate: &Path) -> PathBuf {
    trash_reservation_marker_with_case_policy(
        trash_folder,
        candidate,
        cfg!(any(target_os = "windows", target_os = "macos")),
    )
}

fn trash_reservation_marker_with_case_policy(
    trash_folder: &Path,
    candidate: &Path,
    case_insensitive: bool,
) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    if case_insensitive {
        candidate.to_string_lossy().to_lowercase().hash(&mut hasher);
    } else {
        candidate.hash(&mut hasher);
    }
    trash_folder.join(format!(
        ".wavecrate-trash-reservation-{:016x}",
        hasher.finish()
    ))
}

fn move_path(source: &Path, destination: &Path) -> Result<(), String> {
    match rename_no_replace(source, destination) {
        Ok(()) => Ok(()),
        Err(rename_error) => move_path_after_no_replace_error(source, destination, rename_error),
    }
}

fn move_path_after_no_replace_error(
    source: &Path,
    destination: &Path,
    rename_error: io::Error,
) -> Result<(), String> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let rename_error = if no_replace_rename_is_unsupported(&rename_error) {
        match rename_via_owned_placeholder(source, destination) {
            Ok(()) => return Ok(()),
            Err(error) => error,
        }
    } else {
        rename_error
    };

    if rename_error.kind() == io::ErrorKind::CrossesDevices {
        return fallback_move_path(source, destination, &rename_error);
    }
    let kind = if source.is_dir() { "folder" } else { "file" };
    Err(format!("Move {kind} to trash failed: {rename_error}"))
}

#[cfg(target_os = "windows")]
fn windows_wide_path(path: &Path) -> io::Result<Vec<u16>> {
    use std::os::windows::ffi::OsStrExt;

    const LEGACY_MAX_PATH: usize = 248;
    const SEP: u16 = b'\\' as u16;
    const ALT_SEP: u16 = b'/' as u16;
    const QUERY: u16 = b'?' as u16;
    const COLON: u16 = b':' as u16;
    const DOT: u16 = b'.' as u16;
    const VERBATIM_PREFIX: &[u16] = &[SEP, SEP, QUERY, SEP];
    const NT_PREFIX: &[u16] = &[SEP, QUERY, QUERY, SEP];
    const UNC_PREFIX: &[u16] = &[
        SEP,
        SEP,
        QUERY,
        SEP,
        b'U' as u16,
        b'N' as u16,
        b'C' as u16,
        SEP,
    ];

    let mut wide = path.as_os_str().encode_wide().collect::<Vec<_>>();
    if wide.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "trash move path contains an interior NUL code unit",
        ));
    }
    if wide.is_empty() || wide.starts_with(VERBATIM_PREFIX) || wide.starts_with(NT_PREFIX) {
        wide.push(0);
        return Ok(wide);
    }
    if wide.len() + 1 < LEGACY_MAX_PATH
        && (matches!(wide.as_slice(), [drive, COLON, SEP | ALT_SEP, ..] if *drive != SEP && *drive != ALT_SEP)
            || matches!(wide.as_slice(), [SEP | ALT_SEP, SEP | ALT_SEP, ..]))
    {
        wide.push(0);
        return Ok(wide);
    }

    let absolute = std::path::absolute(path)?;
    let absolute = absolute.as_os_str().encode_wide().collect::<Vec<_>>();
    let (prefix, skip) = match absolute.as_slice() {
        [_, COLON, SEP, ..] => (VERBATIM_PREFIX, 0),
        [SEP, SEP, DOT, SEP, ..] => (VERBATIM_PREFIX, 4),
        [SEP, SEP, QUERY, SEP, ..] | [SEP, QUERY, QUERY, SEP, ..] => (&[][..], 0),
        [SEP, SEP, ..] => (UNC_PREFIX, 2),
        _ => (&[][..], 0),
    };
    wide.clear();
    wide.reserve(prefix.len() + absolute.len() + 1);
    wide.extend_from_slice(prefix);
    wide.extend_from_slice(&absolute[skip..]);
    wide.push(0);
    Ok(wide)
}

#[cfg(target_os = "windows")]
fn rename_no_replace(source: &Path, destination: &Path) -> io::Result<()> {
    use windows::{
        Win32::{
            Foundation::{ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS, ERROR_NOT_SAME_DEVICE},
            Storage::FileSystem::{MOVE_FILE_FLAGS, MoveFileExW},
        },
        core::{HRESULT, PCWSTR},
    };

    let source = windows_wide_path(source)?;
    let destination = windows_wide_path(destination)?;
    let result = unsafe {
        MoveFileExW(
            PCWSTR(source.as_ptr()),
            PCWSTR(destination.as_ptr()),
            MOVE_FILE_FLAGS(0),
        )
    };
    result.map_err(|error| {
        let code = error.code();
        if code == HRESULT::from_win32(ERROR_ALREADY_EXISTS.0)
            || code == HRESULT::from_win32(ERROR_FILE_EXISTS.0)
        {
            io::Error::from(io::ErrorKind::AlreadyExists)
        } else if code == HRESULT::from_win32(ERROR_NOT_SAME_DEVICE.0) {
            io::Error::from(io::ErrorKind::CrossesDevices)
        } else {
            io::Error::other(error)
        }
    })
}

#[cfg(target_os = "macos")]
fn rename_no_replace(source: &Path, destination: &Path) -> io::Result<()> {
    let source = path_to_c_string(source)?;
    let destination = path_to_c_string(destination)?;
    let result =
        unsafe { libc::renamex_np(source.as_ptr(), destination.as_ptr(), libc::RENAME_EXCL) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn rename_no_replace(source: &Path, destination: &Path) -> io::Result<()> {
    let source = path_to_c_string(source)?;
    let destination = path_to_c_string(destination)?;
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            destination.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn no_replace_rename_is_unsupported(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(libc::ENOSYS | libc::EINVAL | libc::EOPNOTSUPP)
    )
}

#[cfg(any(target_os = "linux", target_os = "android", all(test, unix)))]
fn rename_via_owned_placeholder(source: &Path, destination: &Path) -> io::Result<()> {
    let is_directory = source.is_dir();
    let placeholder = if is_directory {
        fs::create_dir(destination)?;
        File::open(destination)?
    } else {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)?
    };
    match fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(error) => {
            cleanup_owned_placeholder(destination, is_directory, &placeholder);
            Err(error)
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "android", all(test, unix)))]
fn cleanup_owned_placeholder(destination: &Path, is_directory: bool, placeholder: &File) {
    use std::os::unix::fs::MetadataExt;

    let Ok(expected) = placeholder.metadata() else {
        return;
    };
    let Ok(actual) = fs::symlink_metadata(destination) else {
        return;
    };
    if expected.dev() != actual.dev() || expected.ino() != actual.ino() {
        return;
    }
    let cleanup = if is_directory {
        fs::remove_dir(destination)
    } else {
        fs::remove_file(destination)
    };
    if let Err(error) = cleanup
        && error.kind() != io::ErrorKind::NotFound
    {
        tracing::warn!(
            path = %destination.display(),
            error = %error,
            "Failed to remove reserved trash rename placeholder"
        );
    }
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "android"))]
fn path_to_c_string(path: &Path) -> io::Result<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt;

    std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "trash move path contains an interior NUL byte",
        )
    })
}

#[cfg(not(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "linux",
    target_os = "android"
)))]
fn rename_no_replace(_source: &Path, _destination: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "atomic no-replace rename is unavailable on this platform",
    ))
}

fn fallback_move_path(
    source: &Path,
    destination: &Path,
    rename_error: &io::Error,
) -> Result<(), String> {
    let is_directory = source.is_dir();
    let copy_result = if is_directory {
        copy_dir_all(source, destination)
    } else {
        copy_file_exclusive(source, destination)
    };
    if let Err(error) = copy_result {
        let kind = if is_directory { "folder" } else { "file" };
        return Err(format!(
            "Move {kind} to trash failed: {rename_error}; fallback failed: {error}"
        ));
    }
    let cleanup_result = if is_directory {
        fs::remove_dir_all(source)
    } else {
        fs::remove_file(source)
    };
    if let Err(error) = cleanup_result {
        let kind = if is_directory { "folder" } else { "file" };
        return Err(format!(
            "Move {kind} to trash copied successfully, but source cleanup failed: {error}; the trash copy was preserved"
        ));
    }
    Ok(())
}

fn copy_dir_all(source: &Path, destination: &Path) -> std::io::Result<()> {
    let mut directory_permissions = Vec::new();
    let root_permissions = fs::metadata(source)?.permissions();
    fs::create_dir(destination)?;
    let copy_result =
        copy_dir_contents(source, destination, &mut directory_permissions).and_then(|()| {
            directory_permissions.push((destination.to_path_buf(), root_permissions));
            for (path, permissions) in directory_permissions {
                fs::set_permissions(path, permissions)?;
            }
            Ok(())
        });
    if copy_result.is_err() {
        cleanup_partial_directory(destination);
    }
    copy_result
}

fn copy_dir_tree(
    source: &Path,
    destination: &Path,
    directory_permissions: &mut Vec<(PathBuf, fs::Permissions)>,
) -> io::Result<()> {
    let permissions = fs::metadata(source)?.permissions();
    fs::create_dir(destination)?;
    copy_dir_contents(source, destination, directory_permissions)?;
    directory_permissions.push((destination.to_path_buf(), permissions));
    Ok(())
}

fn copy_dir_contents(
    source: &Path,
    destination: &Path,
    directory_permissions: &mut Vec<(PathBuf, fs::Permissions)>,
) -> io::Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_tree(&entry.path(), &target, directory_permissions)?;
        } else {
            copy_file_exclusive(&entry.path(), &target)?;
        }
    }
    Ok(())
}

fn cleanup_partial_directory(destination: &Path) {
    make_tree_removable(destination);
    if let Err(error) = fs::remove_dir_all(destination)
        && error.kind() != io::ErrorKind::NotFound
    {
        tracing::warn!(
            path = %destination.display(),
            error = %error,
            "Failed to remove partial trash fallback copy"
        );
    }
}

#[cfg(unix)]
fn make_tree_removable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if !metadata.file_type().is_dir() {
        return;
    }
    let mut permissions = metadata.permissions();
    permissions.set_mode(permissions.mode() | 0o700);
    let _ = fs::set_permissions(path, permissions);
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            make_tree_removable(&entry.path());
        }
    }
}

#[cfg(windows)]
fn make_tree_removable(path: &Path) {
    clear_readonly_permissions(path);
}

#[cfg(not(any(unix, windows)))]
fn make_tree_removable(_path: &Path) {}

#[cfg(windows)]
fn clear_readonly_permissions(path: &Path) {
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let child = entry.path();
            if entry.file_type().is_ok_and(|file_type| file_type.is_dir()) {
                clear_readonly_permissions(&child);
            }
            if let Ok(metadata) = fs::metadata(&child) {
                let mut permissions = metadata.permissions();
                if permissions.readonly() {
                    permissions.set_readonly(false);
                    let _ = fs::set_permissions(&child, permissions);
                }
            }
        }
    }
    if let Ok(metadata) = fs::metadata(path) {
        let mut permissions = metadata.permissions();
        if permissions.readonly() {
            permissions.set_readonly(false);
            let _ = fs::set_permissions(path, permissions);
        }
    }
}

fn copy_file_exclusive(source: &Path, destination: &Path) -> io::Result<()> {
    let input = File::open(source)?;
    let permissions = input.metadata()?.permissions();
    copy_open_file_exclusive(input, permissions, destination)
}

fn copy_open_file_exclusive(
    mut input: File,
    permissions: fs::Permissions,
    destination: &Path,
) -> io::Result<()> {
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    let copy_result = (|| {
        io::copy(&mut input, &mut output)?;
        output.sync_all()?;
        fs::set_permissions(destination, permissions)
    })();
    if copy_result.is_err() {
        drop(output);
        let _ = fs::remove_file(destination);
    }
    copy_result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use tempfile::tempdir;

    #[test]
    fn trash_destination_numbers_existing_names() {
        let temp = tempdir().unwrap();
        let trash = temp.path().join("trash");
        let source_root = temp.path().join("source");
        fs::create_dir_all(&trash).unwrap();
        fs::create_dir_all(&source_root).unwrap();
        let source = source_root.join("kick.wav");
        fs::write(&source, b"wav").unwrap();
        fs::write(trash.join("kick.wav"), b"old").unwrap();
        fs::write(trash.join("kick 2.wav"), b"old").unwrap();

        let destination = reserve_trash_path(&trash, &source).unwrap();

        assert_eq!(destination.destination(), trash.join("kick 3.wav"));
    }

    #[test]
    fn reservation_drop_closes_and_removes_marker() {
        let temp = tempdir().unwrap();
        let trash = temp.path().join("trash");
        let source = temp.path().join("kick.wav");
        fs::create_dir(&trash).unwrap();
        fs::write(&source, b"kick").unwrap();
        let reservation = reserve_trash_path(&trash, &source).unwrap();
        let marker = reservation.marker.clone();
        assert!(marker.exists());

        drop(reservation);

        assert!(!marker.exists());
    }

    #[cfg(unix)]
    #[test]
    fn trash_move_numbers_past_a_broken_symlink() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let trash = temp.path().join("trash");
        let source_root = temp.path().join("source");
        fs::create_dir(&trash).unwrap();
        fs::create_dir(&source_root).unwrap();
        let source = source_root.join("kick.wav");
        fs::write(&source, b"kick").unwrap();
        let broken_link = trash.join("kick.wav");
        symlink(trash.join("missing.wav"), &broken_link).unwrap();
        assert!(!broken_link.exists());

        let outcome = move_path_to_configured_trash(&source, Some(&trash));
        let moved_path = trash.canonicalize().unwrap().join("kick 2.wav");

        assert_eq!(
            outcome.result,
            TrashMoveResult::Moved {
                destination: moved_path.clone()
            }
        );
        assert_eq!(fs::read(moved_path).unwrap(), b"kick");
        assert!(fs::symlink_metadata(broken_link).is_ok());
    }

    #[test]
    fn trash_worker_captures_post_move_source_evidence() {
        let temp = tempdir().unwrap();
        let trash = temp.path().join("trash");
        let source = temp.path().join("kick.wav");
        fs::create_dir(&trash).unwrap();
        fs::write(&source, b"kick").unwrap();

        let outcome = move_path_to_configured_trash(&source, Some(&trash));

        assert!(matches!(outcome.result, TrashMoveResult::Moved { .. }));
        assert_eq!(outcome.evidence, SourceFileEvidence::Missing);
    }

    #[cfg(unix)]
    #[test]
    fn broken_source_symlink_is_reported_as_failed_instead_of_missing() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let trash = temp.path().join("trash");
        let source = temp.path().join("kick.wav");
        fs::create_dir(&trash).unwrap();
        symlink(temp.path().join("missing.wav"), &source).unwrap();
        assert!(!source.exists());

        let outcome = move_path_to_configured_trash(&source, Some(&trash));

        assert!(matches!(
            outcome.result,
            TrashMoveResult::Failed { ref error }
                if error.starts_with("Trash source is unavailable:")
        ));
        assert!(fs::symlink_metadata(source).is_ok());
    }

    #[test]
    fn case_insensitive_reservation_markers_merge_case_only_destinations() {
        let trash = Path::new("/trash");
        let upper = trash.join("Kick.wav");
        let lower = trash.join("kick.wav");

        assert_eq!(
            trash_reservation_marker_with_case_policy(trash, &upper, true),
            trash_reservation_marker_with_case_policy(trash, &lower, true)
        );
        assert_ne!(
            trash_reservation_marker_with_case_policy(trash, &upper, false),
            trash_reservation_marker_with_case_policy(trash, &lower, false)
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_move_paths_preserve_short_and_extend_long_names() {
        fn decoded(path: &Path) -> String {
            let wide = windows_wide_path(path).unwrap();
            String::from_utf16(wide.strip_suffix(&[0]).unwrap()).unwrap()
        }

        let short = Path::new(r"C:\packs\kick.wav");
        assert_eq!(decoded(short), short.to_string_lossy());

        let tail = format!(r"{}kick.wav", "nested\\".repeat(50));
        let long_drive = PathBuf::from(format!(r"C:\packs\{tail}"));
        assert_eq!(decoded(&long_drive), format!(r"\\?\C:\packs\{tail}"));

        let long_unc = PathBuf::from(format!(r"\\server\share\{tail}"));
        assert_eq!(decoded(&long_unc), format!(r"\\?\UNC\server\share\{tail}"));

        let verbatim = PathBuf::from(format!(r"\\?\C:\packs\{tail}"));
        assert_eq!(decoded(&verbatim), verbatim.to_string_lossy());
    }

    #[cfg(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "linux",
        target_os = "android"
    ))]
    #[test]
    fn no_replace_rename_preserves_an_existing_destination() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source.wav");
        let destination = temp.path().join("destination.wav");
        fs::write(&source, b"source").unwrap();
        fs::write(&destination, b"existing").unwrap();

        let error = rename_no_replace(&source, &destination).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&source).unwrap(), b"source");
        assert_eq!(fs::read(&destination).unwrap(), b"existing");
    }

    #[cfg(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "linux",
        target_os = "android"
    ))]
    #[test]
    fn no_replace_rename_moves_into_an_absent_destination() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source.wav");
        let destination = temp.path().join("destination.wav");
        fs::write(&source, b"source").unwrap();

        rename_no_replace(&source, &destination).unwrap();

        assert!(!source.exists());
        assert_eq!(fs::read(&destination).unwrap(), b"source");
    }

    #[test]
    fn non_cross_device_rename_errors_do_not_start_copy_fallback() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source.wav");
        let destination = temp.path().join("destination.wav");
        fs::write(&source, b"source").unwrap();

        let error = move_path_after_no_replace_error(
            &source,
            &destination,
            io::Error::from(io::ErrorKind::PermissionDenied),
        )
        .unwrap_err();

        assert!(error.contains("Move file to trash failed"));
        assert_eq!(fs::read(&source).unwrap(), b"source");
        assert!(!destination.exists());
    }

    #[cfg(unix)]
    #[test]
    fn owned_placeholders_enable_same_volume_file_and_directory_renames() {
        let temp = tempdir().unwrap();
        let source_file = temp.path().join("source.wav");
        let destination_file = temp.path().join("trash.wav");
        let source_dir = temp.path().join("source-folder");
        let destination_dir = temp.path().join("trash-folder");
        fs::write(&source_file, b"source").unwrap();
        fs::create_dir(&source_dir).unwrap();
        fs::write(source_dir.join("child.wav"), b"child").unwrap();

        rename_via_owned_placeholder(&source_file, &destination_file).unwrap();
        rename_via_owned_placeholder(&source_dir, &destination_dir).unwrap();

        assert!(!source_file.exists());
        assert_eq!(fs::read(destination_file).unwrap(), b"source");
        assert!(!source_dir.exists());
        assert_eq!(
            fs::read(destination_dir.join("child.wav")).unwrap(),
            b"child"
        );
    }

    #[cfg(unix)]
    #[test]
    fn owned_placeholder_cleanup_preserves_a_replaced_destination() {
        let temp = tempdir().unwrap();
        let destination = temp.path().join("trash.wav");
        let placeholder = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
            .unwrap();
        fs::remove_file(&destination).unwrap();
        fs::write(&destination, b"external").unwrap();

        cleanup_owned_placeholder(&destination, false, &placeholder);

        assert_eq!(fs::read(destination).unwrap(), b"external");
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn unsupported_atomic_rename_uses_owned_same_volume_placeholder() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source.wav");
        let destination = temp.path().join("trash.wav");
        fs::write(&source, b"source").unwrap();

        move_path_after_no_replace_error(
            &source,
            &destination,
            io::Error::from_raw_os_error(libc::ENOSYS),
        )
        .unwrap();

        assert!(!source.exists());
        assert_eq!(fs::read(destination).unwrap(), b"source");
    }

    #[cfg(unix)]
    #[test]
    fn reservation_marker_stays_bounded_for_long_valid_file_names() {
        let temp = tempdir().unwrap();
        let trash = temp.path().join("trash");
        let source_root = temp.path().join("source");
        fs::create_dir(&trash).unwrap();
        fs::create_dir(&source_root).unwrap();
        let source = source_root.join(format!("{}.wav", "x".repeat(245)));
        fs::write(&source, b"long name").unwrap();

        let reservation = reserve_trash_path(&trash, &source).unwrap();

        assert_eq!(reservation.destination().file_name(), source.file_name());
        assert!(
            reservation
                .marker
                .file_name()
                .unwrap()
                .as_encoded_bytes()
                .len()
                < 100
        );
    }

    #[test]
    fn recursive_copy_preserves_nested_files() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        let destination = temp.path().join("destination");
        fs::create_dir_all(source.join("nested")).unwrap();
        fs::write(source.join("root.wav"), b"root").unwrap();
        fs::write(source.join("nested").join("child.wav"), b"child").unwrap();

        copy_dir_all(&source, &destination).unwrap();

        assert_eq!(fs::read(destination.join("root.wav")).unwrap(), b"root");
        assert_eq!(
            fs::read(destination.join("nested").join("child.wav")).unwrap(),
            b"child"
        );
    }

    #[cfg(unix)]
    #[test]
    fn fallback_copies_preserve_file_and_directory_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        let destination = temp.path().join("destination");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("private.wav"), b"private").unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o750)).unwrap();
        fs::set_permissions(
            source.join("private.wav"),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();

        copy_dir_all(&source, &destination).unwrap();

        assert_eq!(
            fs::metadata(&destination).unwrap().permissions().mode() & 0o777,
            0o750
        );
        assert_eq!(
            fs::metadata(destination.join("private.wav"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    #[test]
    fn completed_copy_survives_when_source_path_disappears_after_open() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source.wav");
        let destination = temp.path().join("trash.wav");
        fs::write(&source, b"only copy").unwrap();
        let input = File::open(&source).unwrap();
        let permissions = input.metadata().unwrap().permissions();
        fs::remove_file(&source).unwrap();

        copy_open_file_exclusive(input, permissions, &destination).unwrap();

        assert_eq!(fs::read(destination).unwrap(), b"only copy");
    }

    #[test]
    fn batch_retains_successes_when_a_later_source_is_missing() {
        let temp = tempdir().unwrap();
        let trash = temp.path().join("trash");
        let source_root = temp.path().join("source");
        fs::create_dir_all(&source_root).unwrap();
        let first = source_root.join("first.wav");
        let missing = source_root.join("missing.wav");
        fs::write(&first, b"first").unwrap();

        let outcomes =
            move_paths_to_configured_trash(&[first.clone(), missing.clone()], Some(&trash));

        assert_eq!(outcomes.len(), 2);
        assert!(matches!(outcomes[0].result, TrashMoveResult::Moved { .. }));
        assert_eq!(outcomes[1].result, TrashMoveResult::Missing);
        assert!(!first.exists());
        assert_eq!(fs::read(trash.join("first.wav")).unwrap(), b"first");
    }

    #[test]
    fn concurrent_same_name_trash_moves_preserve_both_payloads() {
        let temp = tempdir().unwrap();
        let trash = temp.path().join("trash");
        let left_root = temp.path().join("left");
        let right_root = temp.path().join("right");
        fs::create_dir_all(&left_root).unwrap();
        fs::create_dir_all(&right_root).unwrap();
        let left = left_root.join("kick.wav");
        let right = right_root.join("kick.wav");
        fs::write(&left, b"left").unwrap();
        fs::write(&right, b"right").unwrap();
        let barrier = Arc::new(Barrier::new(2));

        let handles = [left, right].map(|source| {
            let barrier = Arc::clone(&barrier);
            let trash = trash.clone();
            std::thread::spawn(move || {
                barrier.wait();
                move_path_to_configured_trash(&source, Some(&trash))
            })
        });
        let outcomes = handles.map(|handle| handle.join().unwrap());

        assert!(
            outcomes
                .iter()
                .all(|outcome| matches!(outcome.result, TrashMoveResult::Moved { .. }))
        );
        let mut payloads = fs::read_dir(&trash)
            .unwrap()
            .map(|entry| fs::read(entry.unwrap().path()).unwrap())
            .collect::<Vec<_>>();
        payloads.sort();
        assert_eq!(payloads, vec![b"left".to_vec(), b"right".to_vec()]);
    }

    #[cfg(unix)]
    #[test]
    fn failed_fallback_copy_removes_partial_destination() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        let destination = temp.path().join("destination");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("copied.wav"), b"copied").unwrap();
        symlink(source.join("missing.wav"), source.join("broken.wav")).unwrap();

        let error = fallback_move_path(
            &source,
            &destination,
            &io::Error::from(io::ErrorKind::CrossesDevices),
        )
        .unwrap_err();

        assert!(error.contains("fallback failed"));
        assert!(source.exists());
        assert!(!destination.exists());
    }

    #[cfg(unix)]
    #[test]
    fn partial_cleanup_removes_restrictive_copied_subdirectories() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempdir().unwrap();
        let destination = temp.path().join("partial");
        let restricted = destination.join("restricted");
        fs::create_dir_all(&restricted).unwrap();
        fs::write(restricted.join("copied.wav"), b"copied").unwrap();
        fs::set_permissions(&restricted, fs::Permissions::from_mode(0o500)).unwrap();

        cleanup_partial_directory(&destination);

        assert!(!destination.exists());
    }

    #[test]
    fn fallback_does_not_remove_destination_created_by_another_actor() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source.wav");
        let destination = temp.path().join("destination.wav");
        fs::write(&source, b"source").unwrap();
        fs::write(&destination, b"external").unwrap();

        let error = fallback_move_path(
            &source,
            &destination,
            &io::Error::from(io::ErrorKind::CrossesDevices),
        )
        .unwrap_err();

        assert!(error.contains("fallback failed"));
        assert_eq!(fs::read(&destination).unwrap(), b"external");
        assert_eq!(fs::read(&source).unwrap(), b"source");
    }

    #[test]
    fn directory_fallback_does_not_remove_destination_created_by_another_actor() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        let destination = temp.path().join("destination");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("source.wav"), b"source").unwrap();
        fs::create_dir(&destination).unwrap();
        fs::write(destination.join("external.wav"), b"external").unwrap();

        let error = fallback_move_path(
            &source,
            &destination,
            &io::Error::from(io::ErrorKind::CrossesDevices),
        )
        .unwrap_err();

        assert!(error.contains("fallback failed"));
        assert_eq!(
            fs::read(destination.join("external.wav")).unwrap(),
            b"external"
        );
        assert_eq!(fs::read(source.join("source.wav")).unwrap(), b"source");
    }
}
