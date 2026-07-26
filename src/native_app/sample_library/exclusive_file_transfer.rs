use std::{
    fs::{self, File},
    io::{self, ErrorKind, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::Arc,
};

use platform::{rename_no_replace, same_file_handles};

#[derive(Clone, Debug)]
pub(super) struct CommittedFile {
    path: PathBuf,
    owned_file: Arc<File>,
}

impl CommittedFile {
    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn remove_if_owned(&self) -> io::Result<bool> {
        self.remove_if_owned_with(|| {})
    }

    fn remove_if_owned_with(&self, before_quarantine: impl FnOnce()) -> io::Result<bool> {
        if !self.still_owned()? {
            return Ok(false);
        }
        before_quarantine();
        let parent = self.path.parent().unwrap_or_else(|| Path::new(""));
        let quarantine = tempfile::Builder::new()
            .prefix(".wavecrate-cleanup-")
            .tempdir_in(parent)?;
        let quarantined = quarantine.path().join("owned-file");
        rename_no_replace(&self.path, &quarantined)?;
        // From this point onward, keep the private directory on every error path. Its
        // destructor must never recursively remove an object that was moved from the
        // user-visible destination before its identity was verified.
        let quarantine = quarantine.keep();
        let moved = fs::symlink_metadata(&quarantined)
            .and_then(|metadata| {
                metadata
                    .is_file()
                    .then(|| File::open(&quarantined))
                    .transpose()
            })
            .map_err(|error| preserved_cleanup_error(error, &quarantined))?;
        let owned = match moved.as_ref() {
            Some(moved) => same_file_handles(&self.owned_file, moved)
                .map_err(|error| preserved_cleanup_error(error, &quarantined))?,
            None => false,
        };
        if !owned {
            return restore_quarantined_destination(&quarantined, &self.path, &quarantine);
        }
        drop(moved);
        if let Err(remove_error) = fs::remove_file(&quarantined) {
            return Err(io::Error::new(
                remove_error.kind(),
                format!(
                    "owned destination cleanup failed and was preserved at {}: {remove_error}",
                    quarantined.display()
                ),
            ));
        }
        fs::remove_dir(quarantine)?;
        Ok(true)
    }

    pub(super) fn move_back_if_owned(&self, source: &Path) -> io::Result<bool> {
        let mut input = self.owned_file.try_clone()?;
        input.seek(SeekFrom::Start(0))?;
        copy_open_file_no_replace(
            &mut input,
            self.owned_file.metadata()?.permissions(),
            source,
        )?;
        let _ = self.remove_if_owned();
        Ok(true)
    }

    fn still_owned(&self) -> io::Result<bool> {
        let actual = match File::open(&self.path) {
            Ok(actual) => actual,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        };
        same_file_handles(&self.owned_file, &actual)
    }
}

fn preserved_cleanup_error(error: io::Error, quarantined: &Path) -> io::Error {
    io::Error::new(
        error.kind(),
        format!(
            "destination cleanup could not verify the quarantined object at {}; it was preserved: {error}",
            quarantined.display()
        ),
    )
}

fn restore_quarantined_destination(
    quarantined: &Path,
    destination: &Path,
    quarantine: &Path,
) -> io::Result<bool> {
    match rename_no_replace(quarantined, destination) {
        Ok(()) => {
            fs::remove_dir(quarantine)?;
            Ok(false)
        }
        Err(restore_error) => Err(io::Error::new(
            restore_error.kind(),
            format!(
                "destination changed during cleanup and was preserved at {} after restore failed: {restore_error}",
                quarantined.display()
            ),
        )),
    }
}

pub(super) fn copy_file_no_replace(source: &Path, destination: &Path) -> io::Result<CommittedFile> {
    let staged = stage_file_copy(source, destination)?;
    publish_staged_file(staged, destination)
}

pub(super) fn copy_file_to_unique_destination(
    source: &Path,
    first_candidate: &Path,
) -> io::Result<CommittedFile> {
    copy_file_to_unique_destination_with(source, first_candidate, |_, _| {})
}

pub(super) fn move_file_no_replace(source: &Path, destination: &Path) -> io::Result<CommittedFile> {
    let source_entry = fs::symlink_metadata(source)?;
    if !source_entry.file_type().is_file() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "native file moves require a regular source file: {}",
                source.display()
            ),
        ));
    }
    let source_file = File::open(source)?;
    match rename_no_replace(source, destination) {
        Ok(()) => Ok(committed_file(destination, source_file)),
        Err(error) => move_file_after_rename_error(source, destination, error),
    }
}

fn move_file_after_rename_error(
    source: &Path,
    destination: &Path,
    rename_error: io::Error,
) -> io::Result<CommittedFile> {
    if rename_requires_copy_fallback(&rename_error) {
        let committed = copy_file_no_replace(source, destination)?;
        if let Err(remove_error) = fs::remove_file(source) {
            return Err(io::Error::new(
                remove_error.kind(),
                format!(
                    "copied to {} without replacing another file, but failed to remove the source: {remove_error}; the completed copy was preserved",
                    destination.display()
                ),
            ));
        }
        Ok(committed)
    } else {
        Err(rename_error)
    }
}

pub(super) fn move_file_to_unique_destination(
    source: &Path,
    first_candidate: &Path,
) -> io::Result<CommittedFile> {
    for index in 0..10_000 {
        let candidate = unique_copy_candidate(first_candidate, index);
        match move_file_no_replace(source, &candidate) {
            Ok(committed) => return Ok(committed),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        ErrorKind::AlreadyExists,
        "could not find an available destination name",
    ))
}

pub(super) fn unique_copy_candidate(first_candidate: &Path, index: usize) -> PathBuf {
    if index == 0 {
        return first_candidate.to_path_buf();
    }
    let parent = first_candidate.parent().unwrap_or_else(|| Path::new(""));
    let stem = first_candidate
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| String::from("sample"));
    let extension = first_candidate
        .extension()
        .map(|extension| extension.to_string_lossy().to_string());
    let file_name = match extension {
        Some(extension) => format!("{stem}_copy{index:03}.{extension}"),
        None => format!("{stem}_copy{index:03}"),
    };
    parent.join(file_name)
}

fn stage_file_copy(source: &Path, destination: &Path) -> io::Result<tempfile::NamedTempFile> {
    let parent = destination.parent().unwrap_or_else(|| Path::new(""));
    let mut input = File::open(source)?;
    let permissions = input.metadata()?.permissions();
    stage_open_file_copy(&mut input, permissions, parent)
}

fn stage_open_file_copy(
    input: &mut File,
    permissions: fs::Permissions,
    parent: &Path,
) -> io::Result<tempfile::NamedTempFile> {
    let mut staged = tempfile::Builder::new()
        .prefix(".wavecrate-transfer-")
        .suffix(".tmp")
        .tempfile_in(parent)?;
    io::copy(input, staged.as_file_mut())?;
    staged.as_file().set_permissions(permissions)?;
    staged.as_file().sync_all()?;
    Ok(staged)
}

fn copy_open_file_no_replace(
    input: &mut File,
    permissions: fs::Permissions,
    destination: &Path,
) -> io::Result<CommittedFile> {
    let parent = destination.parent().unwrap_or_else(|| Path::new(""));
    publish_staged_file(
        stage_open_file_copy(input, permissions, parent)?,
        destination,
    )
}

fn publish_staged_file(
    staged: tempfile::NamedTempFile,
    destination: &Path,
) -> io::Result<CommittedFile> {
    let file = staged
        .persist_noclobber(destination)
        .map_err(|error| error.error)?;
    Ok(committed_file(destination, file))
}

pub(super) fn copy_file_to_unique_destination_with(
    source: &Path,
    first_candidate: &Path,
    mut before_publish: impl FnMut(usize, &Path),
) -> io::Result<CommittedFile> {
    let mut staged = stage_file_copy(source, first_candidate)?;
    for index in 0..10_000 {
        let candidate = unique_copy_candidate(first_candidate, index);
        before_publish(index, &candidate);
        match staged.persist_noclobber(&candidate) {
            Ok(file) => return Ok(committed_file(&candidate, file)),
            Err(error) if error.error.kind() == ErrorKind::AlreadyExists => {
                staged = error.file;
            }
            Err(error) => return Err(error.error),
        }
    }
    Err(io::Error::new(
        ErrorKind::AlreadyExists,
        "could not find an available destination name",
    ))
}

fn committed_file(path: &Path, owned_file: File) -> CommittedFile {
    CommittedFile {
        path: path.to_path_buf(),
        owned_file: Arc::new(owned_file),
    }
}

fn rename_requires_copy_fallback(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        ErrorKind::CrossesDevices | ErrorKind::Unsupported
    ) || platform_rename_requires_copy_fallback(error)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn platform_rename_requires_copy_fallback(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(libc::ENOSYS | libc::EINVAL | libc::EOPNOTSUPP)
    )
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn platform_rename_requires_copy_fallback(_error: &io::Error) -> bool {
    false
}

mod platform;
#[cfg(test)]
mod tests;
