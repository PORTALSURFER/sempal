use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use cap_primitives::ambient_authority;
use cap_primitives::fs::{
    DirOptions, FollowSymlinks, OpenOptions, create_dir, hard_link, open, open_ambient_dir,
    open_dir_nofollow, remove_file,
};

use crate::filesystem_identity::stable_filesystem_identity_from_open_file;

use super::super::SourceDatabase;

/// An already-open source root retained for the complete recovery of one row.
///
/// All relative filesystem operations below are resolved from this descriptor. The
/// descriptor is deliberately not converted back into a path between validation and
/// mutation, so replacement of the named root cannot redirect recovery elsewhere.
pub(super) struct RecoveryRoot {
    path: PathBuf,
    capability: fs::File,
}

/// Descriptor-derived facts for one regular file.
#[derive(Debug)]
pub(super) struct OpenedFile {
    pub(super) identity: String,
    pub(super) file_size: u64,
    pub(super) modified_ns: i64,
}

impl RecoveryRoot {
    pub(super) fn open_if_available(
        path: &Path,
        expected_identity: Option<&str>,
    ) -> Result<Option<Self>, String> {
        match fs::symlink_metadata(path) {
            Ok(_) => Self::open(path, expected_identity).map(Some),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!(
                "failed to inspect recovery root {}: {error}",
                path.display()
            )),
        }
    }

    pub(super) fn open(path: &Path, expected_identity: Option<&str>) -> Result<Self, String> {
        let expected_identity = expected_identity.ok_or_else(|| {
            format!(
                "refusing file-op recovery for {} because the root identity is missing",
                path.display()
            )
        })?;
        let capability = open_root_nofollow(path).map_err(|error| {
            format!(
                "failed to open recovery root {} without following symlinks: {error}",
                path.display()
            )
        })?;
        let actual_identity = root_identity_from_open_file(&capability).map_err(|error| {
            format!(
                "failed to inspect recovery root {} from its open descriptor: {error}",
                path.display()
            )
        })?;
        if actual_identity != expected_identity {
            return Err(format!(
                "refusing file-op recovery for {} because the root identity changed (expected {}, found {})",
                path.display(),
                expected_identity,
                actual_identity
            ));
        }
        Ok(Self {
            path: path.to_path_buf(),
            capability,
        })
    }

    pub(super) fn open_file(&self, relative: &Path) -> Result<Option<OpenedFile>, String> {
        let Some(parent) =
            self.open_directory_optional(relative.parent().unwrap_or_else(|| Path::new(".")))?
        else {
            return Ok(None);
        };
        let name = relative.file_name().ok_or_else(|| {
            format!(
                "invalid recovery file path relative to {}: {}",
                self.path.display(),
                relative.display()
            )
        })?;
        let mut options = OpenOptions::new();
        options.read(true)._cap_fs_ext_follow(FollowSymlinks::No);
        let file = match open(&parent, Path::new(name), &options) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(format!(
                    "failed to open {} without following symlinks: {error}",
                    self.path.join(relative).display()
                ));
            }
        };
        let metadata = file.metadata().map_err(|error| {
            format!(
                "failed to inspect {} from its open descriptor: {error}",
                self.path.join(relative).display()
            )
        })?;
        if !metadata.is_file() {
            return Err(format!(
                "refusing {} because it is not a regular file",
                self.path.join(relative).display()
            ));
        }
        let identity = stable_filesystem_identity_from_open_file(&file).ok_or_else(|| {
            format!(
                "failed to obtain a stable identity for {} from its open descriptor",
                self.path.join(relative).display()
            )
        })?;
        let modified_ns = metadata
            .modified()
            .map_err(|error| {
                format!(
                    "missing modified time for {}: {error}",
                    self.path.join(relative).display()
                )
            })?
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "file modified time is before epoch".to_string())?
            .as_nanos() as i64;
        Ok(Some(OpenedFile {
            identity,
            file_size: metadata.len(),
            modified_ns,
        }))
    }

    pub(super) fn ensure_parent(&self, relative: &Path) -> Result<(), String> {
        let parent = relative.parent().unwrap_or_else(|| Path::new("."));
        let mut directory = self.capability.try_clone().map_err(|error| {
            format!(
                "failed to retain {} capability: {error}",
                self.path.display()
            )
        })?;
        for component in relative_components(parent)? {
            let component_path = Path::new(component);
            directory = match open_dir_nofollow(&directory, component_path) {
                Ok(directory) => directory,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    create_dir(&directory, component_path, &DirOptions::new()).map_err(
                        |error| {
                            format!(
                                "failed to create {} without following symlinks: {error}",
                                self.path.join(parent).display()
                            )
                        },
                    )?;
                    open_dir_nofollow(&directory, component_path).map_err(|error| {
                        format!(
                            "failed to reopen {} without following symlinks: {error}",
                            self.path.join(parent).display()
                        )
                    })?
                }
                Err(error) => {
                    return Err(format!(
                        "failed to open ancestor {} without following symlinks: {error}",
                        self.path.join(parent).display()
                    ));
                }
            };
        }
        Ok(())
    }

    pub(super) fn hard_link_no_replace(
        &self,
        staged_relative: &Path,
        target_relative: &Path,
    ) -> Result<(), String> {
        let staged_parent =
            self.open_directory(staged_relative.parent().unwrap_or_else(|| Path::new(".")))?;
        let target_parent =
            self.open_directory(target_relative.parent().unwrap_or_else(|| Path::new(".")))?;
        let staged_name = staged_relative.file_name().ok_or_else(|| {
            format!(
                "invalid staged recovery path: {}",
                staged_relative.display()
            )
        })?;
        let target_name = target_relative.file_name().ok_or_else(|| {
            format!(
                "invalid target recovery path: {}",
                target_relative.display()
            )
        })?;
        hard_link(
            &staged_parent,
            Path::new(staged_name),
            &target_parent,
            Path::new(target_name),
        )
        .map_err(|error| {
            format!(
                "failed to finalize {} to {} without replacement: {error}",
                self.path.join(staged_relative).display(),
                self.path.join(target_relative).display()
            )
        })
    }

    pub(super) fn remove_file_nofollow(&self, relative: &Path) -> Result<(), String> {
        let parent = self.open_directory(relative.parent().unwrap_or_else(|| Path::new(".")))?;
        let name = relative
            .file_name()
            .ok_or_else(|| format!("invalid recovery file path: {}", relative.display()))?;
        remove_file(&parent, Path::new(name)).map_err(|error| {
            format!(
                "failed to remove {} without following symlinks: {error}",
                self.path.join(relative).display()
            )
        })
    }

    fn open_directory(&self, relative: &Path) -> Result<fs::File, String> {
        self.open_directory_optional(relative)?.ok_or_else(|| {
            format!(
                "recovery ancestor does not exist: {}",
                self.path.join(relative).display()
            )
        })
    }

    fn open_directory_optional(&self, relative: &Path) -> Result<Option<fs::File>, String> {
        let mut directory = self.capability.try_clone().map_err(|error| {
            format!(
                "failed to retain {} capability: {error}",
                self.path.display()
            )
        })?;
        for component in relative_components(relative)? {
            directory = match open_dir_nofollow(&directory, Path::new(component)) {
                Ok(directory) => directory,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(error) => {
                    return Err(format!(
                        "failed to open ancestor {} without following symlinks: {error}",
                        self.path.join(relative).display()
                    ));
                }
            };
        }
        Ok(Some(directory))
    }
}

pub(super) fn capture_root_identity(path: &Path) -> io::Result<String> {
    let root = open_root_nofollow(path)?;
    root_identity_from_open_file(&root).map_err(io::Error::other)
}

pub(super) fn capture_file_identity(root_path: &Path, relative: &Path) -> io::Result<String> {
    let capability = open_root_nofollow(root_path)?;
    let root = RecoveryRoot {
        path: root_path.to_path_buf(),
        capability,
    };
    let file = root
        .open_file(relative)
        .map_err(io::Error::other)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "file is missing"))?;
    Ok(file.identity)
}

fn open_root_nofollow(path: &Path) -> io::Result<fs::File> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("root is not a non-symlink directory: {}", path.display()),
        ));
    }
    if path == Path::new("/") {
        return open_ambient_dir(path, ambient_authority());
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let parent_capability = open_ambient_dir(parent, ambient_authority())?;
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("root has no final component: {}", path.display()),
        )
    })?;
    open_dir_nofollow(&parent_capability, Path::new(name))
}

fn root_identity_from_open_file(file: &fs::File) -> io::Result<String> {
    stable_filesystem_identity_from_open_file(file)
        .ok_or_else(|| io::Error::other("stable filesystem identity is unavailable"))
}

fn relative_components(path: &Path) -> Result<Vec<&OsStr>, String> {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(component) => components.push(component),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(format!(
                    "invalid non-relative recovery path: {}",
                    path.display()
                ));
            }
        }
    }
    Ok(components)
}

pub(super) trait RecoverySourceDatabases {
    fn open(&self, root: &Path) -> Result<SourceDatabase, String>;
}

pub(super) struct SourceDatabaseRecoveryAccess;

impl RecoverySourceDatabases for SourceDatabaseRecoveryAccess {
    fn open(&self, root: &Path) -> Result<SourceDatabase, String> {
        SourceDatabase::open_for_source_write(root)
            .map_err(|error| format!("Failed to open source DB for recovery: {error}"))
    }
}
