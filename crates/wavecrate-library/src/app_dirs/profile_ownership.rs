//! Cross-process writable ownership for one Wavecrate persistence profile.

use std::{
    fs::File,
    io::{self, Write},
    path::{Component, Path, PathBuf},
};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt, ambient_authority};
use cap_std::fs::{Dir, OpenOptions};
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
    /// Opening, validating, or writing the profile ownership boundary failed.
    #[error("profile ownership filesystem operation failed at {path}: {source}")]
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
    /// The retained profile root no longer matches the path used to acquire ownership.
    #[error("profile root was replaced after ownership acquisition: {path}")]
    ProfileRootReplaced {
        /// Profile root path whose identity changed.
        path: PathBuf,
    },
    /// The profile-owner lock entry no longer matches the acquired lock file.
    #[error("profile ownership lock was replaced after acquisition: {path}")]
    ProfileOwnerLockReplaced {
        /// Profile lock path whose identity changed.
        path: PathBuf,
    },
    /// The host could not provide a stable identity for the retained capability.
    #[error("profile ownership identity unavailable at {path}")]
    IdentityUnavailable {
        /// Profile path whose identity could not be established.
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
    profile_identity: String,
    lock_path: PathBuf,
    lock_identity: String,
    root_dir: Dir,
    _lock_file: File,
}

impl WritableProfileGuard {
    /// Resolve the current profile and acquire its nonblocking writable-owner lock.
    pub fn acquire_current() -> Result<Self, ProfileOwnershipError> {
        let profile_root = super::resolution::resolve_persistence()
            .map_err(|error| ProfileOwnershipError::AppDirectory(error.to_string()))?
            .app_root;
        let lock_path = profile_root.join(PROFILE_OWNER_LOCK_FILE_NAME);
        #[cfg(all(not(unix), not(windows)))]
        return Err(ProfileOwnershipError::Unsupported { path: lock_path });

        let root_dir =
            open_profile_root(&profile_root).map_err(|source| ProfileOwnershipError::Io {
                path: profile_root.clone(),
                source,
            })?;
        let profile_identity = identity_for_root(&profile_root, &root_dir)?;
        let lock_file = acquire_lock(&root_dir, &lock_path)?;
        let lock_identity = identity_for_lock(&lock_path, &lock_file)?;
        let guard = Self {
            profile_root,
            profile_identity,
            lock_path,
            lock_identity,
            root_dir,
            _lock_file: lock_file,
        };
        guard.validate_current()?;
        Ok(guard)
    }

    /// Return the profile root protected by this guard.
    pub fn profile_root(&self) -> &Path {
        &self.profile_root
    }

    /// Clone this acquired profile capability for another profile-owned participant.
    ///
    /// The clone retains the same lock identity and open lock descriptor. It therefore does not
    /// acquire a second profile lock, and the profile remains owned until every participant clone
    /// has been dropped.
    pub fn try_clone(&self) -> Result<Self, ProfileOwnershipError> {
        self.validate_current()?;
        let root_dir = self
            .root_dir
            .try_clone()
            .map_err(|source| ProfileOwnershipError::Io {
                path: self.profile_root.clone(),
                source,
            })?;
        let lock_file =
            self._lock_file
                .try_clone()
                .map_err(|source| ProfileOwnershipError::Io {
                    path: self.lock_path.clone(),
                    source,
                })?;
        Ok(Self {
            profile_root: self.profile_root.clone(),
            profile_identity: self.profile_identity.clone(),
            lock_path: self.lock_path.clone(),
            lock_identity: self.lock_identity.clone(),
            root_dir,
            _lock_file: lock_file,
        })
    }

    /// Revalidate the acquired root and lock identities without following links.
    ///
    /// A successful check means the configured profile path still names the same root and
    /// lock entries that were acquired. The retained directory capability remains the authority
    /// for child access; this check detects a path replacement so the owner can fail closed.
    pub fn validate_current(&self) -> Result<(), ProfileOwnershipError> {
        let current_root = open_profile_root(&self.profile_root).map_err(|_| {
            ProfileOwnershipError::ProfileRootReplaced {
                path: self.profile_root.clone(),
            }
        })?;
        let current_identity = identity_from_dir(&current_root).ok_or_else(|| {
            ProfileOwnershipError::ProfileRootReplaced {
                path: self.profile_root.clone(),
            }
        })?;
        if current_identity != self.profile_identity {
            return Err(ProfileOwnershipError::ProfileRootReplaced {
                path: self.profile_root.clone(),
            });
        }

        let current_lock = open_lock_file(&current_root, false).map_err(|_| {
            ProfileOwnershipError::ProfileOwnerLockReplaced {
                path: self.lock_path.clone(),
            }
        })?;
        if !is_regular_file(&current_lock).unwrap_or(false) {
            return Err(ProfileOwnershipError::ProfileOwnerLockReplaced {
                path: self.lock_path.clone(),
            });
        }
        let current_lock_identity =
            identity_from_file(&self.lock_path, &current_lock).ok_or_else(|| {
                ProfileOwnershipError::ProfileOwnerLockReplaced {
                    path: self.lock_path.clone(),
                }
            })?;
        if current_lock_identity != self.lock_identity {
            return Err(ProfileOwnershipError::ProfileOwnerLockReplaced {
                path: self.lock_path.clone(),
            });
        }
        Ok(())
    }

    /// Clone the retained no-follow profile-root capability for a production participant.
    pub fn profile_root_dir(&self) -> Result<Dir, ProfileOwnershipError> {
        self.validate_current()?;
        self.root_dir
            .try_clone()
            .map_err(|source| ProfileOwnershipError::Io {
                path: self.profile_root.clone(),
                source,
            })
    }

    /// Create or open one direct child directory relative to the retained profile capability.
    pub fn open_child_dir(&self, child: &Path) -> Result<Dir, ProfileOwnershipError> {
        let child_name =
            single_normal_component(child).map_err(|source| ProfileOwnershipError::Io {
                path: self.profile_root.join(child),
                source,
            })?;
        self.validate_current()?;
        match self.root_dir.create_dir(child_name) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(source) => {
                return Err(ProfileOwnershipError::Io {
                    path: self.profile_root.join(child),
                    source,
                });
            }
        }
        let child_dir = self
            .root_dir
            .open_dir_nofollow(child_name)
            .map_err(|source| ProfileOwnershipError::Io {
                path: self.profile_root.join(child),
                source,
            })?;
        self.validate_current()?;
        Ok(child_dir)
    }
}

fn single_normal_component(path: &Path) -> io::Result<&Path> {
    let mut components = path.components();
    let Some(Component::Normal(name)) = components.next() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "profile child must be a single normal component",
        ));
    };
    if components.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "profile child must be a single normal component",
        ));
    }
    Ok(Path::new(name))
}

fn open_profile_root(path: &Path) -> io::Result<Dir> {
    let mut components = path.components();
    #[cfg(unix)]
    {
        if !matches!(components.next(), Some(Component::RootDir)) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "profile root must be absolute",
            ));
        }
        let mut dir = Dir::open_ambient_dir(Path::new("/"), ambient_authority())?;
        for component in components {
            let Component::Normal(name) = component else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "profile root contains a non-normal component",
                ));
            };
            dir = dir.open_dir_nofollow(name)?;
        }
        Ok(dir)
    }
    #[cfg(windows)]
    {
        let Some(Component::Prefix(prefix)) = components.next() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "profile root must have a Windows path prefix",
            ));
        };
        if !matches!(components.next(), Some(Component::RootDir)) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "profile root must be absolute",
            ));
        }
        let mut anchor = PathBuf::from(prefix.as_os_str());
        anchor.push(std::path::MAIN_SEPARATOR_STR);
        let mut dir = Dir::open_ambient_dir(anchor, ambient_authority())?;
        for component in components {
            let Component::Normal(name) = component else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "profile root contains a non-normal component",
                ));
            };
            dir = dir.open_dir_nofollow(name)?;
        }
        return Ok(dir);
    }
    #[cfg(all(not(unix), not(windows)))]
    {
        let _ = (path, components);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "no verified profile-root capability on this platform",
        ))
    }
}

fn identity_for_root(path: &Path, dir: &Dir) -> Result<String, ProfileOwnershipError> {
    identity_from_dir(dir).ok_or_else(|| ProfileOwnershipError::IdentityUnavailable {
        path: path.to_path_buf(),
    })
}

fn identity_for_lock(path: &Path, file: &File) -> Result<String, ProfileOwnershipError> {
    identity_from_file(path, file).ok_or_else(|| ProfileOwnershipError::IdentityUnavailable {
        path: path.to_path_buf(),
    })
}

fn identity_from_dir(dir: &Dir) -> Option<String> {
    let retained_file = dir.try_clone().ok()?.into_std_file();
    crate::filesystem_identity::stable_filesystem_identity_from_open_file(&retained_file)
}

fn identity_from_file(_path: &Path, file: &File) -> Option<String> {
    crate::filesystem_identity::stable_filesystem_identity_from_open_file(file)
}

fn is_regular_file(file: &File) -> io::Result<bool> {
    let metadata = file.metadata()?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

        return Ok(
            metadata.is_file() && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 == 0
        );
    }
    #[cfg(not(windows))]
    {
        Ok(metadata.is_file() && !metadata.is_symlink())
    }
}

fn open_lock_file(root_dir: &Dir, create: bool) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create(create)
        .follow(FollowSymlinks::No);
    root_dir
        .open_with(Path::new(PROFILE_OWNER_LOCK_FILE_NAME), &options)
        .map(|file| file.into_std())
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
fn acquire_lock(root_dir: &Dir, path: &Path) -> Result<File, ProfileOwnershipError> {
    use std::os::fd::AsRawFd;

    let file = open_lock_file(root_dir, true).map_err(|source| ProfileOwnershipError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if !is_regular_file(&file).map_err(|source| ProfileOwnershipError::Io {
        path: path.to_path_buf(),
        source,
    })? {
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
fn acquire_lock(root_dir: &Dir, path: &Path) -> Result<File, ProfileOwnershipError> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::{ERROR_LOCK_VIOLATION, HANDLE, WIN32_ERROR};
    use windows::Win32::Storage::FileSystem::{
        LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY, LockFileEx,
    };
    use windows::Win32::System::IO::OVERLAPPED;

    let file = open_lock_file(root_dir, true).map_err(|source| ProfileOwnershipError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if !is_regular_file(&file).map_err(|source| ProfileOwnershipError::Io {
        path: path.to_path_buf(),
        source,
    })? {
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
fn acquire_lock(_root_dir: &Dir, path: &Path) -> Result<File, ProfileOwnershipError> {
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
    fn retained_root_capability_and_identity_detect_profile_root_replacement() {
        let _runtime = test_runtime();
        let base = tempdir().expect("profile base");
        let _base_guard = ConfigBaseGuard::set(base.path().to_path_buf());
        let _profile_guard = PersistenceProfileGuard::named("ownership-root-replaced");
        let guard = WritableProfileGuard::acquire_current().expect("profile holder");
        let root = guard.profile_root().to_path_buf();
        let journal = guard
            .open_child_dir(Path::new("operation_journal"))
            .expect("journal capability");

        let displaced = base.path().join("displaced-profile-root");
        fs::rename(&root, &displaced).expect("displace acquired root");
        fs::create_dir(&root).expect("replacement profile root");
        fs::create_dir(root.join("operation_journal")).expect("replacement journal");
        fs::write(
            root.join("operation_journal").join("replacement-marker"),
            b"replacement",
        )
        .expect("replacement marker");

        assert!(matches!(
            guard.validate_current(),
            Err(ProfileOwnershipError::ProfileRootReplaced { path }) if path == root
        ));
        assert!(
            !journal
                .entries()
                .expect("retained journal entries")
                .filter_map(Result::ok)
                .any(|entry| entry.file_name() == "replacement-marker")
        );
    }

    #[cfg(unix)]
    #[test]
    fn retained_lock_identity_detects_profile_lock_replacement() {
        let _runtime = test_runtime();
        let base = tempdir().expect("profile base");
        let _base_guard = ConfigBaseGuard::set(base.path().to_path_buf());
        let _profile_guard = PersistenceProfileGuard::named("ownership-lock-replaced");
        let guard = WritableProfileGuard::acquire_current().expect("profile holder");
        let lock_path = guard.profile_root().join(PROFILE_OWNER_LOCK_FILE_NAME);
        let displaced = guard.profile_root().join("profile-owner.lock.old");
        fs::rename(&lock_path, &displaced).expect("displace acquired lock entry");
        fs::write(&lock_path, b"replacement").expect("replacement lock entry");

        assert!(matches!(
            guard.validate_current(),
            Err(ProfileOwnershipError::ProfileOwnerLockReplaced { path }) if path == lock_path
        ));
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
