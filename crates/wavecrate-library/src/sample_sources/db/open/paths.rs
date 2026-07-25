#[cfg(target_os = "macos")]
use std::path::Component;
use std::{
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
};

use super::super::{DB_FILE_NAME, LEGACY_DB_FILE_NAME, SourceDbError};

const SYMLINK_REASON: &str = "source database files and sidecars must not be symlinks";
const OUTSIDE_ROOT_REASON: &str = "source database path resolves outside the database root";
const NOT_FILE_REASON: &str = "source database paths must be regular files";

pub(super) fn read_only_db_path(database_root: &Path) -> Result<Option<PathBuf>, SourceDbError> {
    let Some(policy) = SourceDbPathPolicy::for_read_only(database_root)? else {
        return Ok(None);
    };
    let db_path = database_root.join(DB_FILE_NAME);
    if policy.regular_file_status(&db_path)? == PathStatus::RegularFile {
        policy.validate_sidecars(&db_path)?;
        return Ok(Some(db_path));
    }
    let legacy_path = database_root.join(LEGACY_DB_FILE_NAME);
    if policy.regular_file_status(&legacy_path)? == PathStatus::RegularFile {
        policy.validate_sidecars(&legacy_path)?;
        return Ok(Some(legacy_path));
    }
    Ok(None)
}

pub(super) fn prepare_writable_db_path(database_root: &Path) -> Result<PathBuf, SourceDbError> {
    let policy = SourceDbPathPolicy::for_writable(database_root)?;
    prepare_writable_db_path_with_policy(database_root, &policy)
}

pub(super) fn prepare_writable_db_path_in_existing_root(
    database_root: &Path,
) -> Result<PathBuf, SourceDbError> {
    let policy = SourceDbPathPolicy::from_existing_root(database_root)?;
    prepare_writable_db_path_with_policy(database_root, &policy)
}

fn prepare_writable_db_path_with_policy(
    database_root: &Path,
    policy: &SourceDbPathPolicy,
) -> Result<PathBuf, SourceDbError> {
    let db_path = database_root.join(DB_FILE_NAME);
    if policy.regular_file_status(&db_path)? == PathStatus::RegularFile {
        policy.validate_sidecars(&db_path)?;
        return Ok(db_path);
    }
    let legacy_path = database_root.join(LEGACY_DB_FILE_NAME);
    if policy.regular_file_status(&legacy_path)? == PathStatus::RegularFile {
        policy.validate_legacy_migration_paths(&legacy_path, &db_path)?;
        migrate_legacy_source_db(&legacy_path, &db_path)?;
    }
    policy.validate_creatable_path(&db_path)?;
    policy.validate_sidecars(&db_path)?;
    Ok(db_path)
}

fn migrate_legacy_source_db(from: &Path, to: &Path) -> Result<(), SourceDbError> {
    fs::rename(from, to).map_err(|source| SourceDbError::RenameLegacyDatabase {
        from: from.to_path_buf(),
        to: to.to_path_buf(),
        source,
    })?;
    for suffix in ["-wal", "-shm"] {
        let legacy_sidecar = sqlite_sidecar_path(from, suffix);
        if path_exists_without_following(&legacy_sidecar)? {
            let current_sidecar = sqlite_sidecar_path(to, suffix);
            fs::rename(&legacy_sidecar, &current_sidecar).map_err(|source| {
                SourceDbError::RenameLegacyDatabase {
                    from: legacy_sidecar,
                    to: current_sidecar,
                    source,
                }
            })?;
        }
    }
    Ok(())
}

fn sqlite_sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut name = OsString::from(path.as_os_str());
    name.push(suffix);
    PathBuf::from(name)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PathStatus {
    Missing,
    RegularFile,
}

struct SourceDbPathPolicy {
    canonical_root: PathBuf,
}

impl SourceDbPathPolicy {
    fn for_read_only(database_root: &Path) -> Result<Option<Self>, SourceDbError> {
        if !path_exists_without_following(database_root)? {
            return Ok(None);
        }
        Ok(Some(Self::from_existing_root(database_root)?))
    }

    fn for_writable(database_root: &Path) -> Result<Self, SourceDbError> {
        fs::create_dir_all(database_root).map_err(|source| SourceDbError::CreateDir {
            path: database_root.to_path_buf(),
            source,
        })?;
        Self::from_existing_root(database_root)
    }

    fn from_existing_root(database_root: &Path) -> Result<Self, SourceDbError> {
        let canonical_root = canonicalize_for_policy(database_root).map_err(|source| {
            SourceDbError::ResolveSourceDatabasePath {
                path: database_root.to_path_buf(),
                source,
            }
        })?;
        Ok(Self { canonical_root })
    }

    fn regular_file_status(&self, path: &Path) -> Result<PathStatus, SourceDbError> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(source) if source.kind() == io::ErrorKind::NotFound => {
                return Ok(PathStatus::Missing);
            }
            Err(source) => {
                return Err(SourceDbError::InspectSourceDatabasePath {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            return Err(SourceDbError::UnsafeSourceDatabasePath {
                path: path.to_path_buf(),
                reason: SYMLINK_REASON,
            });
        }
        if !file_type.is_file() {
            return Err(SourceDbError::UnsafeSourceDatabasePath {
                path: path.to_path_buf(),
                reason: NOT_FILE_REASON,
            });
        }
        self.validate_existing_path_contained(path)?;
        Ok(PathStatus::RegularFile)
    }

    fn validate_legacy_migration_paths(
        &self,
        legacy_path: &Path,
        current_path: &Path,
    ) -> Result<(), SourceDbError> {
        self.validate_existing_path_contained(legacy_path)?;
        self.validate_creatable_path(current_path)?;
        self.validate_sidecars(legacy_path)?;
        self.validate_sidecars(current_path)?;
        Ok(())
    }

    fn validate_sidecars(&self, db_path: &Path) -> Result<(), SourceDbError> {
        for suffix in ["-wal", "-shm"] {
            let sidecar = sqlite_sidecar_path(db_path, suffix);
            self.regular_file_status(&sidecar)?;
        }
        Ok(())
    }

    fn validate_creatable_path(&self, path: &Path) -> Result<(), SourceDbError> {
        if self.regular_file_status(path)? == PathStatus::RegularFile {
            return Ok(());
        }
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let canonical_parent = canonicalize_for_policy(parent).map_err(|source| {
            SourceDbError::ResolveSourceDatabasePath {
                path: parent.to_path_buf(),
                source,
            }
        })?;
        self.ensure_contained(path, &canonical_parent)
    }

    fn validate_existing_path_contained(&self, path: &Path) -> Result<(), SourceDbError> {
        let canonical_path = canonicalize_for_policy(path).map_err(|source| {
            SourceDbError::ResolveSourceDatabasePath {
                path: path.to_path_buf(),
                source,
            }
        })?;
        self.ensure_contained(path, &canonical_path)
    }

    fn ensure_contained(
        &self,
        original_path: &Path,
        canonical_path: &Path,
    ) -> Result<(), SourceDbError> {
        if canonical_path.starts_with(&self.canonical_root) {
            return Ok(());
        }
        Err(SourceDbError::UnsafeSourceDatabasePath {
            path: original_path.to_path_buf(),
            reason: OUTSIDE_ROOT_REASON,
        })
    }
}

fn canonicalize_for_policy(path: &Path) -> io::Result<PathBuf> {
    match fs::canonicalize(path) {
        Ok(path) => Ok(path),
        Err(_error) if is_existing_macos_vol_namespace(path) => Ok(path.to_path_buf()),
        Err(error) => Err(error),
    }
}

fn is_existing_macos_vol_namespace(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        let mut components = path.components();
        let is_vol = matches!(components.next(), Some(Component::RootDir))
            && matches!(components.next(), Some(Component::Normal(name)) if name == ".vol");
        return is_vol && fs::symlink_metadata(path).is_ok();
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        false
    }
}

fn path_exists_without_following(path: &Path) -> Result<bool, SourceDbError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(SourceDbError::InspectSourceDatabasePath {
            path: path.to_path_buf(),
            source,
        }),
    }
}
