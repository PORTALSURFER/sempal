use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

#[cfg(test)]
use std::{cell::RefCell, collections::BTreeSet};

use cap_fs_ext::{DirExt, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use tracing::warn;
#[cfg(any(test, not(windows)))]
use wavecrate_library::filesystem_identity::filesystem_change_marker;
use wavecrate_library::filesystem_identity::{
    stable_filesystem_identity, stable_filesystem_identity_from_open_file,
};
use wavecrate_library::sample_sources::{
    SourceEntryClassification, SourceEntryFileType, SourceFileClassification,
    SourceIndexDiagnostic, SourceIndexEntry, SourceTraversalPolicy,
    classify_source_entry_with_policy, is_rejected_source_file_path,
};
use wavecrate_library::timestamps::system_time_to_unix_nanos;

use crate::sample_sources::SourceDatabase;

use super::scan::ScanError;
use super::scan::{DirectoryRepeatKind, SourceTreeDiagnostic, SourceTreeFile, SourceTreeSnapshot};
use super::scan_capability::SourceRootCapability;
use super::scan_index::{
    inaccessible_index_entry, index_entry_from_file_facts, non_unicode_index_entry,
};

const MAX_LAYOUT_DIAGNOSTICS: usize = 16;

#[cfg(test)]
thread_local! {
    static FORCED_DIRECTORY_READ_FAILURES: RefCell<BTreeSet<PathBuf>> = const { RefCell::new(BTreeSet::new()) };
    static FORCED_DIRECTORY_ENTRY_FAILURES: RefCell<BTreeSet<PathBuf>> = const { RefCell::new(BTreeSet::new()) };
    static FORCED_FILE_TYPE_FAILURES: RefCell<BTreeSet<PathBuf>> = const { RefCell::new(BTreeSet::new()) };
    static FORCED_FILE_METADATA_FAILURES: RefCell<BTreeSet<PathBuf>> = const { RefCell::new(BTreeSet::new()) };
    static FORCED_DIRECTORY_IDENTITIES: RefCell<BTreeMap<PathBuf, Option<String>>> = const { RefCell::new(BTreeMap::new()) };
}

/// A per-traversal directory identity set shared by full and targeted scans.
#[derive(Default)]
pub(super) struct VisitedDirectories {
    identities: BTreeMap<String, PathBuf>,
    diagnostics: Vec<SourceTreeDiagnostic>,
}

pub(super) enum DirectoryVisit {
    New,
    AlreadyVisited,
    Repeated,
    IdentityUnavailable,
}

impl VisitedDirectories {
    pub(super) fn observe(
        &mut self,
        dir: &Dir,
        absolute_path: &Path,
        relative_path: &Path,
    ) -> DirectoryVisit {
        let identity = match directory_identity(dir, absolute_path) {
            Ok(Some(identity)) => identity,
            Ok(None) => {
                self.record(SourceTreeDiagnostic::DirectoryIdentityUnavailable {
                    path: relative_path.to_path_buf(),
                    error: None,
                });
                return DirectoryVisit::IdentityUnavailable;
            }
            Err(error) => {
                self.record(SourceTreeDiagnostic::DirectoryIdentityUnavailable {
                    path: relative_path.to_path_buf(),
                    error: Some(error.to_string()),
                });
                return DirectoryVisit::IdentityUnavailable;
            }
        };
        if let Some(first_path) = self.identities.get(&identity) {
            if first_path == relative_path {
                return DirectoryVisit::AlreadyVisited;
            }
            let kind = if first_path.as_os_str().is_empty() || relative_path.starts_with(first_path)
            {
                DirectoryRepeatKind::Cycle
            } else {
                DirectoryRepeatKind::RepeatedTarget
            };
            self.record(SourceTreeDiagnostic::RepeatedDirectory {
                path: relative_path.to_path_buf(),
                first_path: first_path.clone(),
                kind,
            });
            return DirectoryVisit::Repeated;
        }
        self.identities
            .insert(identity, relative_path.to_path_buf());
        DirectoryVisit::New
    }

    pub(super) fn record(&mut self, diagnostic: SourceTreeDiagnostic) {
        if self.diagnostics.len() < MAX_LAYOUT_DIAGNOSTICS {
            self.diagnostics.push(diagnostic);
        }
    }

    pub(super) fn diagnostics(&self) -> &[SourceTreeDiagnostic] {
        &self.diagnostics
    }
}

/// Deterministically emulate an unreadable directory for scanner regression
/// tests without depending on platform-specific permission behavior.
#[cfg(test)]
pub(crate) fn force_directory_read_failure(path: &Path) -> ForcedTraversalFailure {
    ForcedTraversalFailure::new(path, ForcedFailureKind::DirectoryRead)
}

/// Deterministically emulate a directory iterator failure for scanner
/// regression tests without depending on filesystem races.
#[cfg(test)]
pub(crate) fn force_directory_entry_failure(path: &Path) -> ForcedTraversalFailure {
    ForcedTraversalFailure::new(path, ForcedFailureKind::DirectoryEntry)
}

/// Deterministically emulate an entry-type failure for scanner regression
/// tests without depending on filesystem races.
#[cfg(test)]
pub(crate) fn force_file_type_failure(path: &Path) -> ForcedTraversalFailure {
    ForcedTraversalFailure::new(path, ForcedFailureKind::FileType)
}

/// Deterministically emulate an unreadable file-metadata observation.
#[cfg(test)]
pub(crate) fn force_file_metadata_failure(path: &Path) -> ForcedTraversalFailure {
    ForcedTraversalFailure::new(path, ForcedFailureKind::FileMetadata)
}

/// Deterministically inject a directory identity for traversal regression tests.
#[cfg(test)]
pub(crate) fn force_directory_identity(
    path: &Path,
    identity: Option<&str>,
) -> ForcedTraversalFailure {
    let path = path.to_path_buf();
    FORCED_DIRECTORY_IDENTITIES.with(|identities| {
        identities
            .borrow_mut()
            .insert(path.clone(), identity.map(str::to_owned));
    });
    ForcedTraversalFailure::new_directory_identity(path)
}

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum ForcedFailureKind {
    DirectoryRead,
    DirectoryEntry,
    FileType,
    FileMetadata,
    DirectoryIdentity,
}

#[cfg(test)]
pub(crate) struct ForcedTraversalFailure {
    path: PathBuf,
    kind: ForcedFailureKind,
}

#[cfg(test)]
impl ForcedTraversalFailure {
    fn new(path: &Path, kind: ForcedFailureKind) -> Self {
        let path = path.to_path_buf();
        forced_failures(kind).with(|failures| {
            failures.borrow_mut().insert(path.clone());
        });
        Self { path, kind }
    }

    #[cfg(test)]
    fn new_directory_identity(path: PathBuf) -> Self {
        Self {
            path,
            kind: ForcedFailureKind::DirectoryIdentity,
        }
    }
}

#[cfg(test)]
impl Drop for ForcedTraversalFailure {
    fn drop(&mut self) {
        if self.kind == ForcedFailureKind::DirectoryIdentity {
            FORCED_DIRECTORY_IDENTITIES.with(|identities| {
                identities.borrow_mut().remove(&self.path);
            });
            return;
        }
        forced_failures(self.kind).with(|failures| {
            failures.borrow_mut().remove(&self.path);
        });
    }
}

#[cfg(test)]
fn forced_failures(
    kind: ForcedFailureKind,
) -> &'static std::thread::LocalKey<RefCell<BTreeSet<PathBuf>>> {
    match kind {
        ForcedFailureKind::DirectoryRead => &FORCED_DIRECTORY_READ_FAILURES,
        ForcedFailureKind::DirectoryEntry => &FORCED_DIRECTORY_ENTRY_FAILURES,
        ForcedFailureKind::FileType => &FORCED_FILE_TYPE_FAILURES,
        ForcedFailureKind::FileMetadata => &FORCED_FILE_METADATA_FAILURES,
        ForcedFailureKind::DirectoryIdentity => &FORCED_DIRECTORY_READ_FAILURES,
    }
}

#[cfg(test)]
pub(super) fn forced_directory_read_error(path: &Path) -> Option<std::io::Error> {
    FORCED_DIRECTORY_READ_FAILURES.with(|failures| {
        failures.borrow().contains(path).then(|| {
            std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "forced directory read failure",
            )
        })
    })
}

#[cfg(test)]
pub(super) fn forced_directory_entry_error(path: &Path) -> Option<std::io::Error> {
    FORCED_DIRECTORY_ENTRY_FAILURES.with(|failures| {
        failures.borrow().contains(path).then(|| {
            std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "forced directory entry failure",
            )
        })
    })
}

#[cfg(test)]
pub(super) fn forced_file_type_error(path: &Path) -> Option<std::io::Error> {
    FORCED_FILE_TYPE_FAILURES.with(|failures| {
        failures.borrow().contains(path).then(|| {
            std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "forced file type failure",
            )
        })
    })
}

#[cfg(test)]
fn forced_file_metadata_error(path: &Path) -> Option<std::io::Error> {
    FORCED_FILE_METADATA_FAILURES.with(|failures| {
        failures.borrow().contains(path).then(|| {
            std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "forced file metadata failure",
            )
        })
    })
}

#[derive(Clone, Debug)]
pub(super) struct FileFacts {
    pub(super) relative: PathBuf,
    pub(super) size: u64,
    pub(super) modified_ns: i64,
    pub(super) file_identity: Option<String>,
    change_marker: Option<String>,
}

impl FileFacts {
    pub(super) fn same_file_facts(&self, other: &Self) -> bool {
        self.size == other.size
            && self.modified_ns == other.modified_ns
            && self.file_identity == other.file_identity
    }

    pub(super) fn same_content_snapshot(&self, other: &Self) -> bool {
        self.same_file_facts(other)
            && self
                .change_marker
                .as_ref()
                .zip(other.change_marker.as_ref())
                .is_some_and(|(before, after)| before == after)
    }
}

pub(super) fn ensure_root_dir(db: &SourceDatabase) -> Result<PathBuf, ScanError> {
    let root = db.root().to_path_buf();
    if root.is_dir() {
        Ok(root)
    } else {
        Err(ScanError::InvalidRoot(root))
    }
}

pub(super) fn visit_dir_with_cancel_check(
    source_root: &SourceRootCapability,
    root: &Path,
    policy: SourceTraversalPolicy,
    should_cancel: &mut impl FnMut() -> bool,
    visitor: &mut impl FnMut(&Path) -> Result<(), ScanError>,
) -> Result<SourceTreeSnapshot, ScanError> {
    let mut snapshot = SourceTreeSnapshot {
        directories: vec![PathBuf::new()],
        ..SourceTreeSnapshot::default()
    };
    let root_dir = source_root.clone_root_dir()?;
    let mut visited = VisitedDirectories::default();
    if !matches!(
        visited.observe(&root_dir, root, Path::new("")),
        DirectoryVisit::New
    ) {
        record_uncertain_prefix(&mut snapshot, root, root);
        snapshot
            .diagnostics
            .extend_from_slice(visited.diagnostics());
        return Ok(snapshot);
    }
    let mut stack = vec![(root_dir, PathBuf::new())];
    while let Some((dir, relative_dir)) = stack.pop() {
        if should_cancel() {
            return Err(ScanError::Canceled);
        }
        let absolute_dir = root.join(&relative_dir);
        let entries = match read_dir(&dir, &absolute_dir) {
            Ok(entries) => entries,
            Err(source) if !relative_dir.as_os_str().is_empty() => {
                warn!(
                    dir = %absolute_dir.display(),
                    error = %source,
                    "Failed to read directory during scan"
                );
                record_layout_diagnostic(
                    &mut snapshot,
                    SourceTreeDiagnostic::DirectoryUnavailable {
                        path: relative_dir.clone(),
                        error: source.to_string(),
                    },
                );
                record_uncertain_prefix(&mut snapshot, root, &absolute_dir);
                continue;
            }
            Err(source) => {
                return Err(ScanError::Io {
                    path: absolute_dir,
                    source,
                });
            }
        };
        for entry_result in entries {
            let entry = match read_dir_entry(entry_result, &absolute_dir) {
                Ok(entry) => entry,
                Err(err) => {
                    warn!(
                        dir = %absolute_dir.display(),
                        error = %err,
                        "Failed to read directory entry during scan"
                    );
                    record_layout_diagnostic(
                        &mut snapshot,
                        SourceTreeDiagnostic::DirectoryEntryUnavailable {
                            path: relative_dir.clone(),
                            error: err.to_string(),
                        },
                    );
                    record_uncertain_prefix(&mut snapshot, root, &absolute_dir);
                    continue;
                }
            };

            let name = entry.file_name();
            let relative = relative_dir.join(&name);
            let path = root.join(&relative);
            let file_type = match read_file_type(&entry, &path) {
                Ok(file_type) => file_type,
                Err(err) => {
                    if is_rejected_source_file_path(&relative) {
                        record_uncertain_prefix(&mut snapshot, root, &path);
                        continue;
                    }
                    warn!(
                        path = %path.display(),
                        error = %err,
                        "Failed to read file type during scan"
                    );
                    record_layout_diagnostic(
                        &mut snapshot,
                        SourceTreeDiagnostic::EntryTypeUnavailable {
                            path: relative.clone(),
                            error: err.to_string(),
                        },
                    );
                    record_uncertain_prefix(&mut snapshot, root, &path);
                    snapshot.index_entries.push(inaccessible_index_entry(
                        relative,
                        SourceIndexDiagnostic::EntryTypeUnavailable,
                    ));
                    continue;
                }
            };
            match classify_source_entry_with_policy(
                &relative,
                cap_source_entry_file_type(&file_type),
                policy,
            ) {
                SourceEntryClassification::Directory { .. } => {
                    match dir.open_dir_nofollow(Path::new(&name)) {
                        Ok(child) => match visited.observe(&child, &path, &relative) {
                            DirectoryVisit::New => {
                                snapshot.directories.push(relative.clone());
                                stack.push((child, relative));
                            }
                            DirectoryVisit::AlreadyVisited => {}
                            DirectoryVisit::Repeated | DirectoryVisit::IdentityUnavailable => {
                                record_uncertain_prefix(&mut snapshot, root, &path);
                            }
                        },
                        Err(source) => {
                            warn!(
                                path = %path.display(),
                                error = %source,
                                "Failed to open directory during scan"
                            );
                            record_layout_diagnostic(
                                &mut snapshot,
                                SourceTreeDiagnostic::DirectoryUnavailable {
                                    path: relative.clone(),
                                    error: source.to_string(),
                                },
                            );
                            if !dir
                                .symlink_metadata(Path::new(&name))
                                .is_ok_and(|metadata| metadata.is_symlink())
                            {
                                record_uncertain_prefix(&mut snapshot, root, &path);
                            }
                        }
                    }
                }
                classification @ SourceEntryClassification::File { .. } => {
                    if classification.indexes_audio() && relative.to_str().is_some() {
                        visitor(&path)?;
                    } else if let Some(file_classification) = classification.file_classification()
                        && (file_classification != SourceFileClassification::SupportedAudio
                            || relative.to_str().is_none())
                    {
                        match source_index_entry(
                            &dir,
                            Path::new(&name),
                            root,
                            &relative,
                            file_classification,
                        ) {
                            Ok(index_entry) => {
                                if let Some(file_size) = index_entry.file_size {
                                    snapshot.other_files.push(SourceTreeFile {
                                        relative_path: relative,
                                        file_size,
                                    });
                                }
                                snapshot.index_entries.push(index_entry);
                            }
                            Err(err) => {
                                record_layout_diagnostic(
                                    &mut snapshot,
                                    SourceTreeDiagnostic::FileMetadataUnavailable {
                                        path: relative.clone(),
                                        error: err.to_string(),
                                    },
                                );
                                record_uncertain_prefix(&mut snapshot, root, &path);
                                snapshot.index_entries.push(inaccessible_index_entry(
                                    relative,
                                    SourceIndexDiagnostic::MetadataUnavailable,
                                ));
                            }
                        }
                    }
                }
                SourceEntryClassification::Rejected(_) => {}
            }
        }
    }
    snapshot.directories.sort();
    snapshot
        .other_files
        .sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    snapshot
        .index_entries
        .sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    snapshot
        .diagnostics
        .extend_from_slice(visited.diagnostics());
    snapshot.diagnostics.truncate(MAX_LAYOUT_DIAGNOSTICS);
    snapshot.uncertain_prefixes.sort();
    Ok(snapshot)
}

fn source_index_entry(
    parent: &Dir,
    name: &Path,
    root: &Path,
    relative_path: &Path,
    classification: SourceFileClassification,
) -> Result<SourceIndexEntry, std::io::Error> {
    let path = root.join(relative_path);
    #[cfg(test)]
    if let Some(error) = forced_file_metadata_error(&path) {
        return Err(error);
    }
    let mut options = OpenOptions::new();
    options.read(true).follow(cap_fs_ext::FollowSymlinks::No);
    let metadata = parent.open_with(name, &options)?.into_std().metadata()?;
    if !metadata.file_type().is_file() {
        return Err(std::io::Error::other(
            "entry changed type during metadata inspection",
        ));
    }
    let modified = metadata.modified()?;
    let modified_ns = system_time_to_unix_nanos(modified);
    if let Some(entry) = index_entry_from_file_facts(
        relative_path.to_path_buf(),
        classification,
        metadata.len(),
        modified_ns,
        stable_filesystem_identity(&path, &metadata),
    ) {
        return Ok(entry);
    }
    if classification == SourceFileClassification::SupportedAudio
        && relative_path.to_str().is_none()
    {
        return Ok(non_unicode_index_entry(
            relative_path.to_path_buf(),
            Some(metadata.len()),
            Some(modified_ns),
            stable_filesystem_identity(&path, &metadata),
        ));
    }
    Err(std::io::Error::other(
        "supported audio is not an index-only entry",
    ))
}

fn record_uncertain_prefix(snapshot: &mut SourceTreeSnapshot, root: &Path, path: &Path) {
    let Ok(relative) = path.strip_prefix(root) else {
        return;
    };
    let relative = relative.to_path_buf();
    if !snapshot.uncertain_prefixes.contains(&relative) {
        snapshot.uncertain_prefixes.push(relative);
    }
}

fn read_dir(dir: &Dir, path: &Path) -> Result<cap_std::fs::ReadDir, std::io::Error> {
    #[cfg(test)]
    if let Some(error) = forced_directory_read_error(path) {
        return Err(error);
    }
    #[cfg(not(test))]
    let _ = path;
    dir.entries()
}

fn directory_identity(dir: &Dir, _path: &Path) -> Result<Option<String>, std::io::Error> {
    #[cfg(test)]
    if let Some(identity) =
        FORCED_DIRECTORY_IDENTITIES.with(|identities| identities.borrow().get(_path).cloned())
    {
        return Ok(identity);
    }
    let file = dir.try_clone()?.into_std_file();
    Ok(stable_filesystem_identity_from_open_file(&file))
}

fn read_file_type(
    entry: &cap_std::fs::DirEntry,
    _path: &Path,
) -> Result<cap_std::fs::FileType, std::io::Error> {
    #[cfg(test)]
    if let Some(error) = forced_file_type_error(_path) {
        return Err(error);
    }
    entry.file_type()
}

fn read_dir_entry(
    entry: Result<cap_std::fs::DirEntry, std::io::Error>,
    _directory: &Path,
) -> Result<cap_std::fs::DirEntry, std::io::Error> {
    #[cfg(test)]
    if let Some(error) = forced_directory_entry_error(_directory) {
        return Err(error);
    }
    entry
}

fn record_layout_diagnostic(snapshot: &mut SourceTreeSnapshot, diagnostic: SourceTreeDiagnostic) {
    if snapshot.diagnostics.len() < MAX_LAYOUT_DIAGNOSTICS {
        snapshot.diagnostics.push(diagnostic);
    }
}

#[cfg(test)]
pub(super) fn read_facts(root: &Path, path: &Path) -> Result<FileFacts, ScanError> {
    let relative = strip_relative(root, path)?;
    #[cfg(test)]
    if let Some(source) = forced_file_metadata_error(path) {
        return Err(ScanError::Io {
            path: path.to_path_buf(),
            source,
        });
    }
    let meta = path.metadata().map_err(|source| ScanError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let modified_ns =
        system_time_to_unix_nanos(meta.modified().map_err(|source| ScanError::Io {
            path: path.to_path_buf(),
            source,
        })?);
    Ok(FileFacts {
        relative,
        size: meta.len(),
        modified_ns,
        file_identity: stable_filesystem_identity(path, &meta),
        change_marker: filesystem_change_marker(path, &meta),
    })
}

/// Read facts from an already-open regular file.
///
/// Targeted watcher reconciliation uses this after opening through a
/// capability-scoped, no-follow path. Keeping the descriptor through hashing
/// ensures the object that was classified is the one whose contents are read.
pub(super) fn read_facts_from_open_file(
    root: &Path,
    path: &Path,
    file: &fs::File,
) -> Result<FileFacts, ScanError> {
    let relative = strip_relative(root, path)?;
    #[cfg(test)]
    if let Some(source) = forced_file_metadata_error(path) {
        return Err(ScanError::Io {
            path: path.to_path_buf(),
            source,
        });
    }
    let meta = file.metadata().map_err(|source| ScanError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let modified_ns =
        system_time_to_unix_nanos(meta.modified().map_err(|source| ScanError::Io {
            path: path.to_path_buf(),
            source,
        })?);
    Ok(FileFacts {
        relative,
        size: meta.len(),
        modified_ns,
        file_identity: {
            #[cfg(windows)]
            {
                stable_filesystem_identity_from_open_file(file)
            }
            #[cfg(not(windows))]
            {
                stable_filesystem_identity(path, &meta)
            }
        },
        change_marker: {
            #[cfg(windows)]
            {
                filesystem_change_marker_from_open_file(file)
            }
            #[cfg(not(windows))]
            {
                filesystem_change_marker(path, &meta)
            }
        },
    })
}

#[cfg(windows)]
fn filesystem_change_marker_from_open_file(file: &fs::File) -> Option<String> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{FILE_BASIC_INFO, FileBasicInfo, GetFileInformationByHandleEx},
    };

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

pub(super) fn is_supported_scannable_audio_file(
    root: &Path,
    relative_path: &Path,
    policy: SourceTraversalPolicy,
) -> bool {
    let absolute_path = root.join(relative_path);
    fs::symlink_metadata(&absolute_path).is_ok_and(|metadata| {
        let file_type = metadata.file_type();
        classify_source_entry_with_policy(relative_path, source_entry_file_type(&file_type), policy)
            .indexes_audio()
    })
}

fn source_entry_file_type(file_type: &fs::FileType) -> SourceEntryFileType {
    SourceEntryFileType::from_no_followed_type(
        file_type.is_dir(),
        file_type.is_file(),
        file_type.is_symlink(),
    )
}

fn cap_source_entry_file_type(file_type: &cap_std::fs::FileType) -> SourceEntryFileType {
    SourceEntryFileType::from_no_followed_type(
        file_type.is_dir(),
        file_type.is_file(),
        file_type.is_symlink(),
    )
}

pub(super) fn compute_content_hash_with_reader(
    path: &Path,
    reader: &mut impl Read,
    cancel: Option<&AtomicBool>,
) -> Result<String, ScanError> {
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        if let Some(cancel) = cancel
            && cancel.load(Ordering::Relaxed)
        {
            return Err(ScanError::Canceled);
        }
        let read = reader.read(&mut buffer).map_err(|source| ScanError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn strip_relative(root: &Path, path: &Path) -> Result<PathBuf, ScanError> {
    if let Ok(relative) = path.strip_prefix(root) {
        return Ok(PathBuf::from(relative));
    }
    if let (Ok(canon_root), Ok(canon_path)) = (root.canonicalize(), path.canonicalize())
        && let Ok(relative) = canon_path.strip_prefix(&canon_root)
    {
        return Ok(PathBuf::from(relative));
    }
    Err(ScanError::InvalidRoot(path.to_path_buf()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::sync::atomic::AtomicBool;

    struct CancelingReader {
        remaining: usize,
        chunk: usize,
        cancel: std::sync::Arc<AtomicBool>,
        canceled: bool,
    }

    impl CancelingReader {
        fn new(total_bytes: usize, chunk: usize, cancel: std::sync::Arc<AtomicBool>) -> Self {
            Self {
                remaining: total_bytes,
                chunk,
                cancel,
                canceled: false,
            }
        }
    }

    impl io::Read for CancelingReader {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.remaining == 0 {
                return Ok(0);
            }
            let to_write = self.chunk.min(self.remaining).min(buf.len());
            buf[..to_write].fill(0);
            self.remaining -= to_write;
            if !self.canceled {
                self.cancel.store(true, Ordering::Relaxed);
                self.canceled = true;
            }
            Ok(to_write)
        }
    }

    #[test]
    fn compute_content_hash_cancels_during_read() {
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let mut reader = CancelingReader::new(256 * 1024, 64 * 1024, cancel.clone());
        let result = compute_content_hash_with_reader(
            Path::new("fake.wav"),
            &mut reader,
            Some(cancel.as_ref()),
        );
        assert!(matches!(result, Err(ScanError::Canceled)));
    }

    #[test]
    fn missing_change_markers_never_prove_a_stable_snapshot() {
        let facts = FileFacts {
            relative: PathBuf::from("sample.wav"),
            size: 32,
            modified_ns: 1,
            file_identity: Some(String::from("identity")),
            change_marker: None,
        };

        assert!(!facts.same_content_snapshot(&facts));
    }

    #[test]
    fn injected_directory_identities_distinguish_cycles_repeats_and_fallback() {
        use cap_fs_ext::ambient_authority;

        let temp = tempfile::tempdir().unwrap();
        let child_path = temp.path().join("child");
        std::fs::create_dir(&child_path).unwrap();
        let root_dir =
            cap_std::fs::Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
        let child_dir = root_dir.open_dir("child").unwrap();
        let _root_identity = force_directory_identity(temp.path(), Some("root"));
        let _child_identity = force_directory_identity(&child_path, Some("root"));

        let mut visited = VisitedDirectories::default();
        assert!(matches!(
            visited.observe(&root_dir, temp.path(), Path::new("")),
            DirectoryVisit::New
        ));
        assert!(matches!(
            visited.observe(&child_dir, &child_path, Path::new("child")),
            DirectoryVisit::Repeated
        ));
        assert!(matches!(
            visited.diagnostics(),
            [SourceTreeDiagnostic::RepeatedDirectory {
                kind: DirectoryRepeatKind::Cycle,
                ..
            }]
        ));

        drop(_child_identity);
        let _unsupported = force_directory_identity(&child_path, None);
        assert!(matches!(
            visited.observe(&child_dir, &child_path, Path::new("child")),
            DirectoryVisit::IdentityUnavailable
        ));
        assert!(matches!(
            visited.diagnostics().last(),
            Some(SourceTreeDiagnostic::DirectoryIdentityUnavailable { error: None, .. })
        ));
    }
}
