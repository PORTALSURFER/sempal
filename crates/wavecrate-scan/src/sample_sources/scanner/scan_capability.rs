use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt, ambient_authority};
use cap_std::fs::{Dir, OpenOptions};
use wavecrate_library::filesystem_identity::{
    stable_filesystem_identity, stable_filesystem_identity_from_open_file,
};

use super::scan::ScanError;

/// A source-root directory capability used for no-follow content access.
pub(super) struct SourceRootCapability {
    root: PathBuf,
    dir: Dir,
    generation: String,
}

/// The current relationship between a manifest path and a retained file descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SourcePathBinding {
    Matches,
    Retire,
    Changed,
}

impl SourceRootCapability {
    pub(super) fn open(root: &Path) -> Result<Self, ScanError> {
        let metadata = fs::metadata(root).map_err(|source| ScanError::Io {
            path: root.to_path_buf(),
            source,
        })?;
        if !metadata.file_type().is_dir() {
            return Err(ScanError::InvalidRoot(root.to_path_buf()));
        }
        let dir =
            Dir::open_ambient_dir(root, ambient_authority()).map_err(|source| ScanError::Io {
                path: root.to_path_buf(),
                source,
            })?;
        let retained_file = dir
            .try_clone()
            .map(|dir| dir.into_std_file())
            .map_err(|source| ScanError::Io {
                path: root.to_path_buf(),
                source,
            })?;
        let Some(generation) = stable_filesystem_identity_from_open_file(&retained_file) else {
            return Err(ScanError::StaleRootGeneration {
                root: root.to_path_buf(),
            });
        };
        Ok(Self {
            root: root.to_path_buf(),
            dir,
            generation,
        })
    }

    pub(super) fn clone_root_dir(&self) -> Result<Dir, ScanError> {
        self.dir.try_clone().map_err(|source| ScanError::Io {
            path: self.root.clone(),
            source,
        })
    }

    /// Revalidate that the configured ambient path still names this retained root.
    pub(super) fn ensure_current_generation(&self) -> Result<(), ScanError> {
        let Ok(metadata) = fs::metadata(&self.root) else {
            return Err(ScanError::StaleRootGeneration {
                root: self.root.clone(),
            });
        };
        let current = stable_filesystem_identity(&self.root, &metadata);
        if current.as_deref() != Some(self.generation.as_str()) {
            return Err(ScanError::StaleRootGeneration {
                root: self.root.clone(),
            });
        }
        Ok(())
    }

    /// Open a regular file beneath the source root without following any path component.
    ///
    /// `None` means the path disappeared, became a link/reparse point, or no longer names a
    /// regular file. Other failures remain uncertain and are returned to the caller.
    pub(super) fn open_regular_file(
        &self,
        relative_path: &Path,
    ) -> Result<Option<fs::File>, ScanError> {
        let Some((parent, name)) = self.open_parent(relative_path)? else {
            return Ok(None);
        };
        let current = match open_file_nofollow(&parent, &name) {
            Ok(file) => file,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                if parent
                    .symlink_metadata(&name)
                    .is_ok_and(|metadata| metadata.is_symlink())
                {
                    return Ok(None);
                }
                return Err(ScanError::Io {
                    path: self.root.join(relative_path),
                    source,
                });
            }
        };
        let metadata = current.metadata().map_err(|source| ScanError::Io {
            path: self.root.join(relative_path),
            source,
        })?;
        if metadata.is_file() {
            Ok(Some(current))
        } else {
            Ok(None)
        }
    }

    /// Reopen a manifest path without following links and compare it with a retained descriptor.
    pub(super) fn path_binding(
        &self,
        relative_path: &Path,
        expected: &fs::File,
    ) -> Result<SourcePathBinding, ScanError> {
        let current = match self.open_regular_file(relative_path) {
            Ok(Some(current)) => current,
            Ok(None) => return Ok(SourcePathBinding::Retire),
            Err(ScanError::Io { .. }) => return Ok(SourcePathBinding::Changed),
            Err(error) => return Err(error),
        };
        let matches = same_open_file(expected, &current).map_err(|source| ScanError::Io {
            path: self.root.join(relative_path),
            source,
        })?;
        Ok(if matches {
            SourcePathBinding::Matches
        } else {
            SourcePathBinding::Changed
        })
    }

    /// Report whether the final entry still exists without resolving links or replaced ancestors.
    pub(super) fn entry_exists_nofollow(&self, relative_path: &Path) -> Result<bool, ScanError> {
        let Some((parent, name)) = self.open_parent(relative_path)? else {
            return Ok(false);
        };
        match parent.symlink_metadata(&name) {
            Ok(_) => Ok(true),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(source) => Err(ScanError::Io {
                path: self.root.join(relative_path),
                source,
            }),
        }
    }

    fn open_parent(&self, relative_path: &Path) -> Result<Option<(Dir, PathBuf)>, ScanError> {
        let Some(name) = relative_path.file_name() else {
            return Ok(None);
        };
        let mut dir = self.dir.try_clone().map_err(|source| ScanError::Io {
            path: self.root.clone(),
            source,
        })?;
        let mut traversed = PathBuf::new();
        for component in relative_path
            .parent()
            .into_iter()
            .flat_map(Path::components)
        {
            let part = match component {
                Component::Normal(part) => part,
                Component::CurDir => continue,
                _ => return Err(ScanError::InvalidRoot(self.root.join(relative_path))),
            };
            traversed.push(part);
            dir = match dir.open_dir_nofollow(part) {
                Ok(dir) => dir,
                Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(source) => {
                    if dir
                        .symlink_metadata(part)
                        .is_ok_and(|metadata| metadata.is_symlink())
                    {
                        return Ok(None);
                    }
                    return Err(ScanError::Io {
                        path: self.root.join(&traversed),
                        source,
                    });
                }
            };
        }
        Ok(Some((dir, PathBuf::from(name))))
    }
}

fn open_file_nofollow(parent: &Dir, name: &Path) -> std::io::Result<fs::File> {
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    parent.open_with(name, &options).map(|file| file.into_std())
}

#[cfg(unix)]
fn same_open_file(left: &fs::File, right: &fs::File) -> std::io::Result<bool> {
    use std::os::unix::fs::MetadataExt;

    let left = left.metadata()?;
    let right = right.metadata()?;
    Ok(left.dev() == right.dev() && left.ino() == right.ino())
}

#[cfg(windows)]
fn same_open_file(left: &fs::File, right: &fs::File) -> std::io::Result<bool> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle},
    };

    let information = |file: &fs::File| {
        let mut information = BY_HANDLE_FILE_INFORMATION::default();
        unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut information) }
            .map_err(std::io::Error::other)?;
        Ok::<_, std::io::Error>((
            information.dwVolumeSerialNumber,
            information.nFileIndexHigh,
            information.nFileIndexLow,
        ))
    };
    Ok(information(left)? == information(right)?)
}

#[cfg(not(any(unix, windows)))]
fn same_open_file(_left: &fs::File, _right: &fs::File) -> std::io::Result<bool> {
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn nofollow_open_rejects_file_and_ancestor_symlinks() {
        use std::os::unix::fs::symlink;

        let source = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("outside.wav"), b"outside").unwrap();
        symlink(
            outside.path().join("outside.wav"),
            source.path().join("linked.wav"),
        )
        .unwrap();
        symlink(outside.path(), source.path().join("linked-dir")).unwrap();
        let capability = SourceRootCapability::open(source.path()).unwrap();

        assert!(
            capability
                .open_regular_file(Path::new("linked.wav"))
                .unwrap()
                .is_none()
        );
        assert!(
            capability
                .open_regular_file(Path::new("linked-dir/outside.wav"))
                .unwrap()
                .is_none()
        );
    }

    #[cfg(windows)]
    #[test]
    fn nofollow_open_rejects_windows_file_and_directory_links_when_supported() {
        use std::os::windows::fs::{symlink_dir, symlink_file};

        let source = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("outside.wav"), b"outside").unwrap();
        let file_link_supported = symlink_file(
            outside.path().join("outside.wav"),
            source.path().join("linked.wav"),
        )
        .is_ok();
        let directory_link_supported =
            symlink_dir(outside.path(), source.path().join("linked-dir")).is_ok();
        if !file_link_supported && !directory_link_supported {
            return;
        }
        let capability = SourceRootCapability::open(source.path()).unwrap();

        if file_link_supported {
            assert!(
                capability
                    .open_regular_file(Path::new("linked.wav"))
                    .unwrap()
                    .is_none()
            );
        }
        if directory_link_supported {
            assert!(
                capability
                    .open_regular_file(Path::new("linked-dir/outside.wav"))
                    .unwrap()
                    .is_none()
            );
        }
    }
}
