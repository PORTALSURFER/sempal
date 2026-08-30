use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use cap_primitives::ambient_authority;
#[cfg(windows)]
use cap_primitives::fs::OpenOptionsExt;
#[cfg(any(windows, test))]
use cap_primitives::fs::{DirOptions, create_dir};
use cap_primitives::fs::{FollowSymlinks, OpenOptions, open, open_ambient_dir, open_dir_nofollow};
#[cfg(test)]
use cap_primitives::fs::{hard_link, remove_file};

use crate::app_dirs::{ProfileOwnershipError, WritableProfileGuard};
use crate::filesystem_identity::{
    stable_filesystem_identity, stable_filesystem_identity_from_open_file,
};
use crate::timestamps::system_time_to_unix_nanos;

use super::super::{DatabaseRootBindingGuard, SourceDatabase};

/// Result of a descriptor-bound staged publication.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StagedFinalization {
    /// The staged object now owns the target pathname and no staged cleanup is required.
    Published,
    /// The staged object remains at its journaled pathname and must be cleaned separately.
    NeedsCleanup,
}

/// Narrow boundary for the only operations that may publish or discard staged recovery data.
pub(super) trait StagedFileFinalizer {
    fn publish(
        &self,
        root: &RecoveryRoot,
        staged_relative: &Path,
        staged: &OpenedFile,
        target_relative: &Path,
    ) -> Result<StagedFinalization, String>;

    fn cleanup(
        &self,
        root: &RecoveryRoot,
        staged_relative: &Path,
        staged: &OpenedFile,
    ) -> Result<(), String>;
}

/// Production descriptor-bound finalization. Windows has the required handle APIs; other
/// platforms intentionally fail closed until an equivalent primitive is available.
pub(super) struct PlatformStagedFileFinalizer;

impl StagedFileFinalizer for PlatformStagedFileFinalizer {
    fn publish(
        &self,
        root: &RecoveryRoot,
        staged_relative: &Path,
        staged: &OpenedFile,
        target_relative: &Path,
    ) -> Result<StagedFinalization, String> {
        #[cfg(windows)]
        {
            root.ensure_parent(target_relative)?;
            let target_parent =
                root.open_directory(target_relative.parent().unwrap_or_else(|| Path::new(".")))?;
            let target_name = target_relative.file_name().ok_or_else(|| {
                format!(
                    "invalid target recovery path: {}",
                    target_relative.display()
                )
            })?;
            windows_finalization::rename_no_replace(
                &staged.capability,
                &target_parent,
                target_name,
            )
            .map_err(|error| {
                format!(
                    "failed to finalize {} to {} without replacement: {error}",
                    root.path.join(staged_relative).display(),
                    root.path.join(target_relative).display()
                )
            })?;
            return Ok(StagedFinalization::Published);
        }

        #[cfg(not(windows))]
        {
            let _ = (root, staged_relative, staged, target_relative);
            Err("descriptor-bound staged finalization is unsupported on this platform; retaining staged data and journal".to_string())
        }
    }

    fn cleanup(
        &self,
        _root: &RecoveryRoot,
        _staged_relative: &Path,
        staged: &OpenedFile,
    ) -> Result<(), String> {
        #[cfg(windows)]
        {
            windows_finalization::delete_by_handle(&staged.capability).map_err(|error| {
                format!("failed to clean staged recovery file through its opened handle: {error}")
            })?;
            return Ok(());
        }

        #[cfg(not(windows))]
        {
            let _ = staged;
            Err("descriptor-bound staged cleanup is unsupported on this platform; retaining staged data and journal".to_string())
        }
    }
}

#[cfg(test)]
pub(super) struct TestStagedFileFinalizer;

#[cfg(test)]
impl StagedFileFinalizer for TestStagedFileFinalizer {
    fn publish(
        &self,
        root: &RecoveryRoot,
        staged_relative: &Path,
        _staged: &OpenedFile,
        target_relative: &Path,
    ) -> Result<StagedFinalization, String> {
        root.ensure_parent(target_relative)?;
        let staged_parent =
            root.open_directory(staged_relative.parent().unwrap_or_else(|| Path::new(".")))?;
        let target_parent =
            root.open_directory(target_relative.parent().unwrap_or_else(|| Path::new(".")))?;
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
        .map_err(|error| format!("test staged publication failed: {error}"))?;
        Ok(StagedFinalization::NeedsCleanup)
    }

    fn cleanup(
        &self,
        root: &RecoveryRoot,
        staged_relative: &Path,
        _staged: &OpenedFile,
    ) -> Result<(), String> {
        let parent =
            root.open_directory(staged_relative.parent().unwrap_or_else(|| Path::new(".")))?;
        let name = staged_relative.file_name().ok_or_else(|| {
            format!(
                "invalid staged recovery path: {}",
                staged_relative.display()
            )
        })?;
        remove_file(&parent, Path::new(name))
            .map_err(|error| format!("test staged cleanup failed: {error}"))
    }
}

#[cfg(windows)]
mod windows_finalization {
    use std::mem::offset_of;
    use std::os::windows::{ffi::OsStrExt, io::AsRawHandle};

    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        FILE_DISPOSITION_INFO, FILE_RENAME_INFO, FileDispositionInfo, FileRenameInfo,
        SetFileInformationByHandle,
    };

    pub(super) fn rename_no_replace(
        staged: &std::fs::File,
        target_parent: &std::fs::File,
        target_name: &std::ffi::OsStr,
    ) -> Result<(), String> {
        let target_name: Vec<u16> = target_name.encode_wide().collect();
        let name_bytes = target_name
            .len()
            .checked_mul(std::mem::size_of::<u16>())
            .ok_or_else(|| "target name is too long".to_string())?;
        let size = offset_of!(FILE_RENAME_INFO, FileName)
            .checked_add(name_bytes)
            .ok_or_else(|| "rename information is too large".to_string())?;
        let word_size = std::mem::size_of::<usize>();
        let word_count = size
            .checked_add(word_size - 1)
            .ok_or_else(|| "rename information alignment overflowed".to_string())?
            / word_size;
        let mut buffer = vec![0usize; word_count];
        let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
        unsafe {
            (*info).Anonymous.ReplaceIfExists = false;
            (*info).RootDirectory = HANDLE(target_parent.as_raw_handle());
            (*info).FileNameLength = name_bytes as u32;
            std::ptr::copy_nonoverlapping(
                target_name.as_ptr(),
                (*info).FileName.as_mut_ptr(),
                target_name.len(),
            );
            SetFileInformationByHandle(
                HANDLE(staged.as_raw_handle()),
                FileRenameInfo,
                info.cast(),
                size as u32,
            )
            .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    pub(super) fn delete_by_handle(staged: &std::fs::File) -> Result<(), String> {
        let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
        unsafe {
            SetFileInformationByHandle(
                HANDLE(staged.as_raw_handle()),
                FileDispositionInfo,
                (&disposition as *const FILE_DISPOSITION_INFO).cast(),
                std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
            )
            .map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

/// An already-open source root retained for the complete recovery of one row.
///
/// All relative filesystem operations below are resolved from this descriptor. The
/// descriptor is deliberately not converted back into a path between validation and
/// mutation, so replacement of the named root cannot redirect recovery elsewhere.
pub(super) struct RecoveryRoot {
    path: PathBuf,
    capability: fs::File,
    identity: String,
}

/// Descriptor-derived facts for one regular file.
#[derive(Debug)]
pub(super) struct OpenedFile {
    capability: fs::File,
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
            identity: actual_identity,
        })
    }

    pub(super) fn revalidate_named_root(&self) -> Result<(), String> {
        let capability = open_root_nofollow(&self.path).map_err(|error| {
            format!(
                "failed to revalidate recovery root {} without following symlinks: {error}",
                self.path.display()
            )
        })?;
        let actual_identity = root_identity_from_open_file(&capability).map_err(|error| {
            format!(
                "failed to inspect recovery root {} during revalidation: {error}",
                self.path.display()
            )
        })?;
        if actual_identity != self.identity {
            return Err(format!(
                "recovery root {} was replaced during recovery (expected {}, found {})",
                self.path.display(),
                self.identity,
                actual_identity
            ));
        }
        Ok(())
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
        #[cfg(windows)]
        options
            .access_mode(
                (windows::Win32::Storage::FileSystem::FILE_GENERIC_READ
                    | windows::Win32::Storage::FileSystem::DELETE)
                    .0,
            )
            .share_mode(
                (windows::Win32::Storage::FileSystem::FILE_SHARE_READ
                    | windows::Win32::Storage::FileSystem::FILE_SHARE_WRITE)
                    .0,
            );
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
        let modified_ns = system_time_to_unix_nanos(metadata.modified().map_err(|error| {
            format!(
                "missing modified time for {}: {error}",
                self.path.join(relative).display()
            )
        })?);
        Ok(Some(OpenedFile {
            capability: file,
            identity,
            file_size: metadata.len(),
            modified_ns,
        }))
    }

    #[cfg(any(windows, test))]
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

    pub(super) fn bind_database_root(&self) -> Result<BoundDatabaseRoot, String> {
        #[cfg(windows)]
        {
            let capability = windows_database_root_binding(&self.path, &self.identity)?;
            let path = windows_final_path(&capability)?;
            validate_database_root_alias(&capability, &path, &self.identity)?;
            return Ok(BoundDatabaseRoot {
                path,
                _capability: capability,
                identity: self.identity.clone(),
            });
        }

        #[cfg(not(windows))]
        {
            let capability = self.capability.try_clone().map_err(|error| {
                format!("failed to retain source-root capability for database recovery: {error}")
            })?;
            let path = database_root_alias(&capability, &self.path, &self.identity)?;
            Ok(BoundDatabaseRoot {
                path,
                _capability: capability,
                identity: self.identity.clone(),
            })
        }
    }

    #[cfg(any(windows, test))]
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

pub(crate) struct BoundDatabaseRoot {
    path: PathBuf,
    _capability: fs::File,
    identity: String,
}

impl BoundDatabaseRoot {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn for_profile_guard(
        profile_guard: &WritableProfileGuard,
    ) -> Result<Self, ProfileOwnershipError> {
        let expected_identity = profile_guard.profile_root_identity();

        #[cfg(windows)]
        let capability =
            windows_database_root_binding(profile_guard.profile_root(), expected_identity)
                .map_err(|error| profile_binding_error(profile_guard, error))?;

        #[cfg(not(windows))]
        let capability = profile_guard.profile_root_dir()?.into_std_file();

        #[cfg(windows)]
        let path = windows_final_path(&capability)
            .map_err(|error| profile_binding_error(profile_guard, error))?;

        #[cfg(not(windows))]
        let path =
            database_root_alias(&capability, profile_guard.profile_root(), expected_identity)
                .map_err(|error| profile_binding_error(profile_guard, error))?;

        validate_database_root_alias(&capability, &path, expected_identity)
            .map_err(|error| profile_binding_error(profile_guard, error))?;
        Ok(Self {
            path,
            _capability: capability,
            identity: expected_identity.to_owned(),
        })
    }

    pub(crate) fn validate_profile_guard(
        &self,
        profile_guard: &WritableProfileGuard,
    ) -> Result<(), ProfileOwnershipError> {
        profile_guard.validate_current()?;
        validate_database_root_alias(
            &self._capability,
            &self.path,
            profile_guard.profile_root_identity(),
        )
        .map_err(|error| profile_binding_error(profile_guard, error))?;
        if self.identity != profile_guard.profile_root_identity() {
            return Err(ProfileOwnershipError::ProfileRootReplaced {
                path: profile_guard.profile_root().to_path_buf(),
            });
        }
        Ok(())
    }
}

impl DatabaseRootBindingGuard for BoundDatabaseRoot {}

fn profile_binding_error(
    profile_guard: &WritableProfileGuard,
    error: String,
) -> ProfileOwnershipError {
    ProfileOwnershipError::Io {
        path: profile_guard.profile_root().to_path_buf(),
        source: io::Error::other(error),
    }
}

#[cfg(not(windows))]
fn database_root_alias(
    capability: &fs::File,
    _named_path: &Path,
    expected_identity: &str,
) -> Result<PathBuf, String> {
    #[cfg(target_os = "linux")]
    let alias = {
        use std::os::unix::io::AsRawFd;
        PathBuf::from(format!("/proc/self/fd/{}", capability.as_raw_fd()))
    };

    #[cfg(target_os = "macos")]
    let alias = {
        use std::os::unix::fs::MetadataExt;
        let metadata = capability.metadata().map_err(|error| {
            format!("failed to inspect source-root capability for /.vol binding: {error}")
        })?;
        PathBuf::from(format!("/.vol/{}/{}", metadata.dev(), metadata.ino()))
    };

    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        let _ = (capability, _named_path, expected_identity);
        return Err(
            "source database recovery requires a supported descriptor-bound root namespace"
                .to_string(),
        );
    }

    validate_database_root_alias(capability, &alias, expected_identity)?;
    Ok(alias)
}

fn validate_database_root_alias(
    capability: &fs::File,
    alias: &Path,
    expected_identity: &str,
) -> Result<(), String> {
    let metadata = fs::metadata(alias).map_err(|error| {
        format!(
            "source database root binding namespace is unavailable at {}: {error}",
            alias.display()
        )
    })?;
    if !metadata.is_dir() {
        return Err(format!(
            "source database root binding namespace is not a directory: {}",
            alias.display()
        ));
    }
    let capability_identity = root_identity_from_open_file(capability)
        .map_err(|error| format!("failed to inspect bound source-root capability: {error}"))?;
    let alias_identity = stable_filesystem_identity(alias, &metadata).ok_or_else(|| {
        format!(
            "stable identity is unavailable for source database binding namespace {}",
            alias.display()
        )
    })?;
    if capability_identity != expected_identity || alias_identity != expected_identity {
        return Err(format!(
            "source database root binding identity mismatch at {} (expected {}, capability {}, alias {})",
            alias.display(),
            expected_identity,
            capability_identity,
            alias_identity
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn windows_database_root_binding(path: &Path, expected_identity: &str) -> Result<fs::File, String> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
        FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    let mut options = fs::OpenOptions::new();
    options
        .read(true)
        .access_mode(FILE_READ_ATTRIBUTES.0)
        .share_mode((FILE_SHARE_READ | FILE_SHARE_WRITE).0)
        .custom_flags((FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT).0);
    let capability = options.open(path).map_err(|error| {
        format!(
            "failed to open source root with a delete-sharing barrier for database recovery {}: {error}",
            path.display()
        )
    })?;
    let actual_identity = root_identity_from_open_file(&capability).map_err(|error| {
        format!(
            "failed to inspect source root opened for descriptor-bound database recovery {}: {error}",
            path.display()
        )
    })?;
    if actual_identity != expected_identity {
        return Err(format!(
            "source root identity changed while opening descriptor-bound database namespace {} (expected {}, found {})",
            path.display(),
            expected_identity,
            actual_identity
        ));
    }
    Ok(capability)
}

#[cfg(windows)]
fn windows_final_path(capability: &fs::File) -> Result<PathBuf, String> {
    use std::os::windows::{ffi::OsStringExt, io::AsRawHandle};
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{GetFinalPathNameByHandleW, VOLUME_NAME_DOS};

    let mut buffer = vec![0u16; 256];
    loop {
        let length = unsafe {
            GetFinalPathNameByHandleW(
                HANDLE(capability.as_raw_handle()),
                &mut buffer,
                VOLUME_NAME_DOS,
            )
        };
        if length == 0 {
            return Err(format!(
                "failed to derive final source-root namespace from its open descriptor: {}",
                std::io::Error::last_os_error()
            ));
        }
        let length = length as usize;
        if length < buffer.len() {
            return Ok(PathBuf::from(std::ffi::OsString::from_wide(
                &buffer[..length],
            )));
        }
        buffer.resize(length + 1, 0);
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
        identity: String::new(),
    };
    let file = root
        .open_file(relative)
        .map_err(io::Error::other)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "file is missing"))?;
    Ok(file.identity)
}

impl OpenedFile {
    pub(super) fn is_still_same_object(&self) -> bool {
        stable_filesystem_identity_from_open_file(&self.capability).as_deref()
            == Some(self.identity.as_str())
    }
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
    fn open(&self, root: &RecoveryRoot) -> Result<SourceDatabase, String>;
}

pub(super) struct SourceDatabaseRecoveryAccess;

impl RecoverySourceDatabases for SourceDatabaseRecoveryAccess {
    fn open(&self, root: &RecoveryRoot) -> Result<SourceDatabase, String> {
        root.revalidate_named_root()?;
        let binding = root.bind_database_root()?;
        let bound_path = binding.path().to_path_buf();
        let database =
            SourceDatabase::open_for_source_write_with_database_root(&root.path, &bound_path)
                .map_err(|error| format!("Failed to open source DB for recovery: {error}"))?;
        root.revalidate_named_root()?;
        Ok(database.retain_database_root_binding(Box::new(binding)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(windows))]
    #[test]
    fn unsupported_platform_finalization_retains_staged_file_and_journal_boundary() {
        let temp = tempfile::tempdir().expect("temporary recovery root");
        std::fs::write(temp.path().join("staged.tmp"), b"staged").expect("staged file");
        let identity = capture_root_identity(temp.path()).expect("root identity");
        let root = RecoveryRoot::open(temp.path(), Some(&identity)).expect("recovery root");
        let staged = root
            .open_file(Path::new("staged.tmp"))
            .expect("open staged")
            .expect("staged exists");
        let finalizer = PlatformStagedFileFinalizer;

        let publish = finalizer.publish(
            &root,
            Path::new("staged.tmp"),
            &staged,
            Path::new("published.wav"),
        );
        assert!(
            publish
                .unwrap_err()
                .contains("unsupported on this platform")
        );
        assert!(temp.path().join("staged.tmp").exists());
        assert!(!temp.path().join("published.wav").exists());

        let cleanup = finalizer.cleanup(&root, Path::new("staged.tmp"), &staged);
        assert!(
            cleanup
                .unwrap_err()
                .contains("unsupported on this platform")
        );
        assert!(temp.path().join("staged.tmp").exists());
    }

    #[test]
    fn database_root_alias_validation_rejects_unavailable_and_mismatched_namespaces() {
        let temp = tempfile::tempdir().expect("temporary recovery root");
        let other = tempfile::tempdir().expect("other root");
        let identity = capture_root_identity(temp.path()).expect("root identity");
        let root = RecoveryRoot::open(temp.path(), Some(&identity)).expect("recovery root");
        let capability = root.capability.try_clone().expect("root capability");

        let unavailable = validate_database_root_alias(
            &capability,
            &temp.path().join("missing-namespace"),
            &identity,
        )
        .unwrap_err();
        assert!(unavailable.contains("namespace is unavailable"));

        let mismatch =
            validate_database_root_alias(&capability, other.path(), &identity).unwrap_err();
        assert!(mismatch.contains("identity mismatch"));
    }

    #[cfg(unix)]
    #[test]
    fn source_root_replacement_is_rejected_until_the_original_namespace_is_restored() {
        let temp = tempfile::tempdir().expect("temporary source parent");
        let root_path = temp.path().join("source");
        let parked_path = temp.path().join("source-parked");
        std::fs::create_dir(&root_path).expect("source root");
        let identity = capture_root_identity(&root_path).expect("root identity");
        let root = RecoveryRoot::open(&root_path, Some(&identity)).expect("recovery root");

        std::fs::rename(&root_path, &parked_path).expect("park original source root");
        std::fs::create_dir(&root_path).expect("replacement source root");
        let replaced = root.revalidate_named_root().unwrap_err();
        assert!(replaced.contains("was replaced during recovery"));

        std::fs::remove_dir(&root_path).expect("remove replacement root");
        std::fs::rename(&parked_path, &root_path).expect("restore original source root");
        root.revalidate_named_root().expect("restored source root");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn source_database_open_remains_bound_after_named_root_replacement() {
        let temp = tempfile::tempdir().expect("temporary source parent");
        let root_path = temp.path().join("source");
        let parked_path = temp.path().join("source-parked");
        std::fs::create_dir(&root_path).expect("source root");
        let identity = capture_root_identity(&root_path).expect("root identity");
        let root = RecoveryRoot::open(&root_path, Some(&identity)).expect("recovery root");

        let database = SourceDatabaseRecoveryAccess
            .open(&root)
            .expect("descriptor-bound source database");
        database
            .set_metadata("test.bound_database", "original")
            .expect("write bound database");

        std::fs::rename(&root_path, &parked_path).expect("park original source root");
        std::fs::create_dir(&root_path).expect("replacement source root");
        let replacement =
            SourceDatabase::open_for_source_write(&root_path).expect("replacement source database");

        assert_eq!(
            database
                .get_metadata("test.bound_database")
                .expect("read bound database")
                .as_deref(),
            Some("original")
        );
        assert_eq!(
            replacement
                .get_metadata("test.bound_database")
                .expect("read replacement database"),
            None
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_finalization_uses_opened_handle_and_retains_collision() {
        let temp = tempfile::tempdir().expect("temporary recovery root");
        std::fs::write(temp.path().join("staged.tmp"), b"staged").expect("staged file");
        let identity = capture_root_identity(temp.path()).expect("root identity");
        let root = RecoveryRoot::open(temp.path(), Some(&identity)).expect("recovery root");
        let staged = root
            .open_file(Path::new("staged.tmp"))
            .expect("open staged")
            .expect("staged exists");
        let finalizer = PlatformStagedFileFinalizer;

        std::fs::write(temp.path().join("collision.wav"), b"existing").expect("collision file");
        let collision = finalizer.publish(
            &root,
            Path::new("staged.tmp"),
            &staged,
            Path::new("collision.wav"),
        );
        assert!(collision.is_err());
        assert_eq!(
            std::fs::read(temp.path().join("staged.tmp")).unwrap(),
            b"staged"
        );
        assert_eq!(
            std::fs::read(temp.path().join("collision.wav")).unwrap(),
            b"existing"
        );

        let published = finalizer.publish(
            &root,
            Path::new("staged.tmp"),
            &staged,
            Path::new("published.wav"),
        );
        assert_eq!(published.unwrap(), StagedFinalization::Published);
        assert!(!temp.path().join("staged.tmp").exists());
        assert_eq!(
            std::fs::read(temp.path().join("published.wav")).unwrap(),
            b"staged"
        );
    }
}
