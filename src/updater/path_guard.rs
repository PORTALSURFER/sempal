//! Installation-path validation for updater writes.
//!
//! The updater refuses absolute, parent-traversing, or symlinked paths so a
//! release archive cannot escape the installation directory while applying an
//! update.

use std::{
    fs,
    io::ErrorKind,
    path::{Component, Path, PathBuf},
};

use super::UpdateError;

/// Canonical, validated updater install root used to resolve update payload paths.
#[derive(Debug, Clone)]
pub(crate) struct ValidatedInstallRoot {
    dir: PathBuf,
}

impl ValidatedInstallRoot {
    /// Create a validated install root from an existing installation directory.
    pub(crate) fn new(dir: &Path) -> Result<Self, UpdateError> {
        let dir = dir
            .canonicalize()
            .map_err(|err| UpdateError::Invalid(format!("Invalid install dir: {err}")))?;
        Ok(Self { dir })
    }

    /// Return the canonical install root.
    pub(crate) fn path(&self) -> &Path {
        &self.dir
    }

    /// Return a validated child path rooted under this installation directory.
    pub(crate) fn child_path(&self, name: &str) -> Result<PathBuf, UpdateError> {
        self.child_path_with_metadata(name, |path| fs::symlink_metadata(path))
    }

    fn child_path_with_metadata(
        &self,
        name: &str,
        symlink_metadata: impl Fn(&Path) -> std::io::Result<fs::Metadata>,
    ) -> Result<PathBuf, UpdateError> {
        let relative = sanitize_relative_path(name)?;
        let candidate = self.dir.join(relative);
        if !candidate.starts_with(&self.dir) {
            return Err(UpdateError::Invalid(format!(
                "Refusing to write outside install dir: {}",
                candidate.display()
            )));
        }
        ensure_no_symlink_path(&candidate, symlink_metadata)?;
        Ok(candidate)
    }
}

fn ensure_no_symlink_path(
    path: &Path,
    symlink_metadata: impl Fn(&Path) -> std::io::Result<fs::Metadata>,
) -> Result<(), UpdateError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            #[cfg(windows)]
            Component::Prefix(_) => {
                current.push(component.as_os_str());
                continue;
            }
            Component::RootDir => {
                current.push(component.as_os_str());
                continue;
            }
            _ => {
                current.push(component.as_os_str());
            }
        }
        match symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(UpdateError::Invalid(format!(
                        "Refusing to write through symlink: {}",
                        current.display()
                    )));
                }
            }
            Err(err) if err.kind() == ErrorKind::NotFound => break,
            Err(err) => {
                if allow_symlink_validation_errors() {
                    return Ok(());
                }
                return Err(UpdateError::Invalid(format!(
                    "Failed to validate update path for symlinks at {}: {err}",
                    current.display()
                )));
            }
        }
    }
    Ok(())
}

fn allow_symlink_validation_errors() -> bool {
    std::env::var("WAVECRATE_UPDATER_ALLOW_SYMLINK_ERRORS")
        .ok()
        .map(|value| {
            let value = value.trim().to_ascii_lowercase();
            matches!(value.as_str(), "1" | "true" | "yes")
        })
        .unwrap_or(false)
}

fn sanitize_relative_path(name: &str) -> Result<PathBuf, UpdateError> {
    let mut sanitized = PathBuf::new();
    let mut saw_component = false;
    for component in Path::new(name).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => {
                sanitized.push(part);
                saw_component = true;
            }
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(UpdateError::Invalid(format!("Invalid update path: {name}")));
            }
        }
    }
    if !saw_component {
        return Err(UpdateError::Invalid(format!("Invalid update path: {name}")));
    }
    Ok(sanitized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use tempfile::tempdir;
    use wavecrate_library::test_runtime::TestRuntimeGuard;

    #[test]
    fn ensure_child_path_rejects_parent_dir() {
        let dir = tempdir().expect("tempdir");
        let root = ValidatedInstallRoot::new(dir.path()).expect("install root");
        let err = root
            .child_path("../evil.txt")
            .expect_err("parent dir must fail");
        assert!(err.to_string().contains("Invalid update path"));
    }

    #[test]
    fn ensure_child_path_rejects_absolute_path() {
        let dir = tempdir().expect("tempdir");
        #[cfg(windows)]
        let name = "C:\\evil.txt";
        #[cfg(not(windows))]
        let name = "/tmp/evil.txt";
        let root = ValidatedInstallRoot::new(dir.path()).expect("install root");
        let err = root.child_path(name).expect_err("absolute path must fail");
        assert!(err.to_string().contains("Invalid update path"));
    }

    #[test]
    fn ensure_child_path_allows_relative_path() {
        let mut runtime = TestRuntimeGuard::acquire();
        runtime.set_var("WAVECRATE_UPDATER_ALLOW_SYMLINK_ERRORS", "1");
        let dir = tempdir().expect("tempdir");
        let root = ValidatedInstallRoot::new(dir.path()).expect("install root");
        let path = root.child_path("./ok/file.txt").expect("relative path");
        let canonical = dir.path().canonicalize().expect("canonical install dir");
        assert!(path.starts_with(&canonical));
        assert!(path.ends_with(Path::new("ok").join("file.txt")));
    }

    #[cfg(unix)]
    #[test]
    fn ensure_child_path_rejects_symlinked_component() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().expect("tempdir");
        let install = dir.path().join("install");
        let external = dir.path().join("external");
        fs::create_dir_all(&install).expect("install dir");
        fs::create_dir_all(&external).expect("external dir");
        let link = install.join("link");
        symlink(&external, &link).expect("symlink");

        let root = ValidatedInstallRoot::new(&install).expect("install root");
        let err = root
            .child_path("link/file.txt")
            .expect_err("symlink must fail");
        assert!(err.to_string().contains("symlink"));
    }

    #[test]
    fn ensure_child_path_fails_on_symlink_metadata_error() {
        let mut runtime = TestRuntimeGuard::acquire();
        runtime.set_var("WAVECRATE_UPDATER_ALLOW_SYMLINK_ERRORS", "0");
        fn fail_metadata(_: &Path) -> io::Result<fs::Metadata> {
            Err(io::Error::new(
                ErrorKind::PermissionDenied,
                "metadata denied",
            ))
        }

        let dir = tempdir().expect("tempdir");
        let root = ValidatedInstallRoot::new(dir.path()).expect("install root");
        let err = root
            .child_path_with_metadata("ok/file.txt", fail_metadata)
            .expect_err("metadata failures must fail closed");
        assert!(
            err.to_string()
                .contains("Failed to validate update path for symlinks")
        );
    }
}
