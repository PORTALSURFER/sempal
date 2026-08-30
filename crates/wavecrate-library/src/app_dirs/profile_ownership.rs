//! Cross-process writable ownership for one Wavecrate persistence profile.

use std::{
    fs::File,
    io::{self, Write},
    path::{Path, PathBuf},
};

use thiserror::Error;

const PROFILE_OWNER_LOCK_FILE_NAME: &str = "profile-owner.lock";

/// Errors returned while acquiring writable ownership of the current profile.
#[derive(Debug, Error)]
pub enum ProfileOwnershipError {
    /// Another process currently holds the profile ownership lock.
    #[error("profile is owned by another process: {path}")]
    ProfileOwnedByAnotherProcess {
        /// Profile lock path that could not be acquired.
        path: PathBuf,
    },
    /// Resolving the current profile root failed.
    #[error("profile root unavailable: {0}")]
    AppDirectory(String),
    /// Opening, validating, or writing the profile lock failed.
    #[error("profile ownership lock failed at {path}: {source}")]
    Io {
        /// Profile lock path involved in the failure.
        path: PathBuf,
        /// Underlying filesystem error.
        source: io::Error,
    },
    /// The profile lock path exists but is not a regular file.
    #[error("profile ownership lock is not a regular file: {path}")]
    NotRegularFile {
        /// Profile lock path that failed validation.
        path: PathBuf,
    },
    /// This platform has no verified nonblocking profile ownership primitive.
    #[error("profile ownership is unsupported on this platform: {path}")]
    Unsupported {
        /// Profile lock path that could not be protected.
        path: PathBuf,
    },
}

/// Process-wide writable ownership for one resolved Wavecrate profile.
///
/// The guard retains the open lock file for its entire lifetime. The lock path is
/// intentionally never removed or renamed; its contents are diagnostic only, while
/// the live descriptor lock is the authority.
#[derive(Debug)]
pub struct WritableProfileGuard {
    profile_root: PathBuf,
    _lock_path: PathBuf,
    _lock_file: File,
}

impl WritableProfileGuard {
    /// Resolve the current profile and acquire its nonblocking writable-owner lock.
    pub fn acquire_current() -> Result<Self, ProfileOwnershipError> {
        let profile_root = super::resolution::resolve_persistence()
            .map_err(|error| ProfileOwnershipError::AppDirectory(error.to_string()))?
            .app_root;
        let lock_path = profile_root.join(PROFILE_OWNER_LOCK_FILE_NAME);
        let lock_file = acquire_lock(&lock_path)?;
        Ok(Self {
            profile_root,
            _lock_path: lock_path,
            _lock_file: lock_file,
        })
    }

    /// Return the profile root protected by this guard.
    pub fn profile_root(&self) -> &Path {
        &self.profile_root
    }
}

fn write_diagnostic(mut file: File, path: &Path) -> Result<File, ProfileOwnershipError> {
    file.set_len(0)
        .and_then(|_| file.write_all(format!("version=1\npid={}\n", std::process::id()).as_bytes()))
        .and_then(|_| file.sync_all())
        .map_err(|source| ProfileOwnershipError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    Ok(file)
}

#[cfg(unix)]
fn acquire_lock(path: &Path) -> Result<File, ProfileOwnershipError> {
    use std::os::{fd::AsRawFd, unix::fs::OpenOptionsExt};

    let mut options = std::fs::OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let file = options
        .open(path)
        .map_err(|source| ProfileOwnershipError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    let metadata = file
        .metadata()
        .map_err(|source| ProfileOwnershipError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    if !metadata.is_file() {
        return Err(ProfileOwnershipError::NotRegularFile {
            path: path.to_path_buf(),
        });
    }

    // Advisory descriptor locking is released by the kernel when this process exits,
    // while the path remains available for diagnostics and future acquisition.
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result != 0 {
        let source = io::Error::last_os_error();
        if source.kind() == io::ErrorKind::WouldBlock {
            return Err(ProfileOwnershipError::ProfileOwnedByAnotherProcess {
                path: path.to_path_buf(),
            });
        }
        return Err(ProfileOwnershipError::Io {
            path: path.to_path_buf(),
            source,
        });
    }

    write_diagnostic(file, path)
}

#[cfg(windows)]
fn acquire_lock(path: &Path) -> Result<File, ProfileOwnershipError> {
    use std::os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    };
    use windows::Win32::Foundation::{
        ERROR_LOCK_VIOLATION, FILE_ATTRIBUTE_REPARSE_POINT, HANDLE, WIN32_ERROR,
    };
    use windows::Win32::Storage::FileSystem::{
        FILE_FLAG_OPEN_REPARSE_POINT, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY,
        LockFileEx,
    };
    use windows::Win32::System::IO::OVERLAPPED;

    let mut options = std::fs::OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create(true)
        .share_mode(0x00000001 | 0x00000002)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0);
    let file = options
        .open(path)
        .map_err(|source| ProfileOwnershipError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    let metadata = file
        .metadata()
        .map_err(|source| ProfileOwnershipError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(ProfileOwnershipError::NotRegularFile {
            path: path.to_path_buf(),
        });
    }

    let mut overlapped = OVERLAPPED::default();
    let result = unsafe {
        LockFileEx(
            HANDLE(file.as_raw_handle()),
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            None,
            u32::MAX,
            u32::MAX,
            &mut overlapped,
        )
    };
    if let Err(error) = result {
        if WIN32_ERROR::from_error(&error) == Some(ERROR_LOCK_VIOLATION) {
            return Err(ProfileOwnershipError::ProfileOwnedByAnotherProcess {
                path: path.to_path_buf(),
            });
        }
        return Err(ProfileOwnershipError::Io {
            path: path.to_path_buf(),
            source: io::Error::other(error.to_string()),
        });
    }

    write_diagnostic(file, path)
}

#[cfg(all(not(unix), not(windows)))]
fn acquire_lock(path: &Path) -> Result<File, ProfileOwnershipError> {
    Err(ProfileOwnershipError::Unsupported {
        path: path.to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app_dirs::{APP_DIR_NAME, ConfigBaseGuard, PROFILE_DIR_NAME, PersistenceProfileGuard},
        test_runtime::TestRuntimeGuard,
    };
    use std::fs;
    use tempfile::tempdir;

    fn test_runtime() -> TestRuntimeGuard {
        TestRuntimeGuard::acquire()
    }

    fn profile_root(base: &std::path::Path, profile: &str) -> PathBuf {
        base.join(APP_DIR_NAME).join(PROFILE_DIR_NAME).join(profile)
    }

    #[test]
    fn active_profile_owner_conflicts_without_replacing_lock_contents() {
        let _runtime = test_runtime();
        let base = tempdir().expect("profile base");
        let _base_guard = ConfigBaseGuard::set(base.path().to_path_buf());
        let _profile_guard = PersistenceProfileGuard::named("ownership-conflict");
        let holder = WritableProfileGuard::acquire_current().expect("profile holder");
        let lock_path = holder.profile_root().join(PROFILE_OWNER_LOCK_FILE_NAME);
        let contents = fs::read(&lock_path).expect("owner diagnostic");

        let error = WritableProfileGuard::acquire_current().expect_err("active owner conflict");

        assert!(matches!(
            error,
            ProfileOwnershipError::ProfileOwnedByAnotherProcess { path } if path == lock_path
        ));
        assert_eq!(
            fs::read(lock_path).expect("owner diagnostic after conflict"),
            contents
        );
    }

    #[test]
    fn dropping_profile_owner_allows_reopen() {
        let _runtime = test_runtime();
        let base = tempdir().expect("profile base");
        let _base_guard = ConfigBaseGuard::set(base.path().to_path_buf());
        let _profile_guard = PersistenceProfileGuard::named("ownership-reopen");
        let holder = WritableProfileGuard::acquire_current().expect("profile holder");
        let root = holder.profile_root().to_path_buf();
        drop(holder);

        let reopened = WritableProfileGuard::acquire_current().expect("reopen profile");

        assert_eq!(reopened.profile_root(), root);
    }

    #[test]
    fn stale_lock_contents_do_not_block_acquisition() {
        let _runtime = test_runtime();
        let base = tempdir().expect("profile base");
        let _base_guard = ConfigBaseGuard::set(base.path().to_path_buf());
        let _profile_guard = PersistenceProfileGuard::named("ownership-stale");
        let root = profile_root(base.path(), "ownership-stale");
        fs::create_dir_all(&root).expect("profile root");
        let lock_path = root.join(PROFILE_OWNER_LOCK_FILE_NAME);
        fs::write(&lock_path, b"pid=stale\nunknown=diagnostic\n").expect("stale diagnostic");

        let guard = WritableProfileGuard::acquire_current().expect("stale contents ignored");

        assert_eq!(guard.profile_root(), root);
        assert_eq!(
            fs::read(lock_path).expect("fresh owner diagnostic"),
            format!("version=1\npid={}\n", std::process::id()).as_bytes()
        );
    }

    #[test]
    fn distinct_profile_roots_are_independently_writable() {
        let _runtime = test_runtime();
        let base = tempdir().expect("profile base");
        let _base_guard = ConfigBaseGuard::set(base.path().to_path_buf());
        let first_profile = PersistenceProfileGuard::named("ownership-one");
        let first = WritableProfileGuard::acquire_current().expect("first profile holder");
        let first_root = first.profile_root().to_path_buf();
        drop(first_profile);

        let _second_profile = PersistenceProfileGuard::named("ownership-two");
        let second = WritableProfileGuard::acquire_current().expect("second profile holder");

        assert_ne!(first_root, second.profile_root());
        assert!(first_root.ends_with("ownership-one"));
        assert!(second.profile_root().ends_with("ownership-two"));
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_profile_lock_fails_closed_without_touching_target() {
        use std::os::unix::fs::symlink;

        let _runtime = test_runtime();
        let base = tempdir().expect("profile base");
        let _base_guard = ConfigBaseGuard::set(base.path().to_path_buf());
        let _profile_guard = PersistenceProfileGuard::named("ownership-symlink");
        let root = profile_root(base.path(), "ownership-symlink");
        fs::create_dir_all(&root).expect("profile root");
        let target = base.path().join("target");
        fs::write(&target, b"do not touch").expect("target");
        let lock_path = root.join(PROFILE_OWNER_LOCK_FILE_NAME);
        symlink(&target, &lock_path).expect("profile lock symlink");

        let result = WritableProfileGuard::acquire_current();

        assert!(result.is_err());
        assert_eq!(
            fs::read(&target).expect("target after failed acquire"),
            b"do not touch"
        );
        assert!(
            fs::symlink_metadata(lock_path)
                .expect("symlink after failed acquire")
                .file_type()
                .is_symlink()
        );
    }

    #[cfg(windows)]
    #[test]
    fn reparse_profile_lock_fails_closed_without_touching_target() {
        use std::os::windows::fs::symlink_file;

        let _runtime = test_runtime();
        let base = tempdir().expect("profile base");
        let _base_guard = ConfigBaseGuard::set(base.path().to_path_buf());
        let _profile_guard = PersistenceProfileGuard::named("ownership-reparse");
        let root = profile_root(base.path(), "ownership-reparse");
        fs::create_dir_all(&root).expect("profile root");
        let target = base.path().join("target");
        fs::write(&target, b"do not touch").expect("target");
        let lock_path = root.join(PROFILE_OWNER_LOCK_FILE_NAME);
        symlink_file(&target, &lock_path).expect("profile lock reparse point");

        let result = WritableProfileGuard::acquire_current();

        assert!(result.is_err());
        assert_eq!(
            fs::read(&target).expect("target after failed acquire"),
            b"do not touch"
        );
    }
}
