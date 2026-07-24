use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    io,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use wavecrate_library::sample_sources::{
    SourceEntryClassification, SourceEntryFileType, SourceFileClassification,
    SourceIndexDiagnostic, SourceIndexEntry, SourceTraversalPolicy,
    classify_source_entry_with_policy, is_rejected_source_file_path,
};

use crate::sample_sources::{SourceDatabase, WavEntry};

use super::{
    scan::{ScanContext, ScanError, ScanMode, ScanStats},
    scan_capability::SourceRootCapability,
    scan_db_sync::{complete_scan_generation, db_sync_phase},
    scan_diff_phase::prepare_diff_from_facts,
    scan_fs::{DirectoryVisit, VisitedDirectories, ensure_root_dir},
    scan_index::{inaccessible_index_entry, index_entry_from_file_facts, non_unicode_index_entry},
    scan_walk::apply_prepared_chunk,
    scan_writer::{ScanWriter, UncoordinatedScanWriter},
};

const TARGET_PREPARE_BATCH_SIZE: usize = 64;

/// Reconcile a bounded set of changed paths against a source database.
///
/// This is the fast path for debounced watcher events. It only indexes rows at
/// or below the supplied relative paths, then applies the same diff and
/// pending-rename rules used by a normal quick scan.
pub fn sync_paths(db: &SourceDatabase, paths: &[PathBuf]) -> Result<ScanStats, ScanError> {
    sync_paths_with_progress(db, paths, None, &mut |_, _| {})
}

/// Reconcile changed paths with a progress callback and optional cancellation.
pub fn sync_paths_with_progress(
    db: &SourceDatabase,
    paths: &[PathBuf],
    cancel: Option<&AtomicBool>,
    on_progress: &mut impl FnMut(usize, &Path),
) -> Result<ScanStats, ScanError> {
    sync_paths_with_progress_and_writer(db, paths, cancel, on_progress, &UncoordinatedScanWriter)
}

/// Reconcile changed paths while coordinating only bounded database mutations.
pub fn sync_paths_with_progress_and_writer(
    db: &SourceDatabase,
    paths: &[PathBuf],
    cancel: Option<&AtomicBool>,
    on_progress: &mut impl FnMut(usize, &Path),
    writer: &impl ScanWriter,
) -> Result<ScanStats, ScanError> {
    let (manifest_revision, manifest_before) = super::manifest::capture_manifest_with_revision(db)?;
    let root = ensure_root_dir(db)?;
    let source_root = SourceRootCapability::open(&root)?;
    source_root.ensure_current_generation()?;
    let policy = db.source_traversal_policy()?;
    let targets = collect_targets(db, &root, &source_root, paths, cancel, policy)?;
    let TargetedScanTargets {
        current_files,
        existing,
        current_index_entries,
        existing_index_entries,
        uncertain_prefixes,
        diagnostics,
    } = targets;
    let mut context = ScanContext::from_existing(
        existing,
        ScanMode::Targeted,
        manifest_revision,
        manifest_before.clone(),
    );
    context.set_traversal_policy(policy);
    context.set_targeted_index_entries(
        existing_index_entries.into_values(),
        current_index_entries.into_values(),
    );
    context.mark_uncertain_prefixes(uncertain_prefixes);
    context.stats.traversal_diagnostics = diagnostics;
    let mut prepared = Vec::with_capacity(TARGET_PREPARE_BATCH_SIZE);
    let mut committed = false;
    let result = (|| {
        for targeted_file in current_files {
            if let Some(cancel) = cancel
                && cancel.load(Ordering::Relaxed)
            {
                return Err(ScanError::Canceled);
            }
            let absolute = root.join(&targeted_file.relative);
            let mut prepared_file = prepare_diff_from_facts(targeted_file.facts, &context);
            prepared_file.source_file = Some(targeted_file.file);
            prepared_file.source_handle_verified = true;
            prepared.push(prepared_file);
            context.stats.total_files += 1;
            on_progress(context.stats.total_files, &absolute);
            if prepared.len() == TARGET_PREPARE_BATCH_SIZE {
                let chunk =
                    std::mem::replace(&mut prepared, Vec::with_capacity(TARGET_PREPARE_BATCH_SIZE));
                committed |= apply_prepared_chunk(
                    db,
                    &root,
                    &source_root,
                    cancel,
                    &mut context,
                    chunk,
                    committed,
                    writer,
                )?;
            }
        }
        if !prepared.is_empty() {
            let _ = apply_prepared_chunk(
                db,
                &root,
                &source_root,
                cancel,
                &mut context,
                prepared,
                committed,
                writer,
            )?;
        }
        let committed_snapshot = db_sync_phase(db, &source_root, &mut context, cancel, writer)?;
        super::scan::reconcile_scan_renames(
            db,
            &source_root,
            &mut context,
            &manifest_before,
            committed_snapshot,
            cancel,
            writer,
        )?;
        complete_scan_generation(db, &source_root, &mut context, cancel, writer)
    })();
    super::scan::finish_scan_result(manifest_before, context, result)
}

struct TargetedScanTargets {
    current_files: Vec<TargetedFile>,
    existing: HashMap<PathBuf, WavEntry>,
    current_index_entries: BTreeMap<PathBuf, SourceIndexEntry>,
    existing_index_entries: BTreeMap<PathBuf, SourceIndexEntry>,
    uncertain_prefixes: BTreeSet<PathBuf>,
    diagnostics: Vec<super::scan::SourceTreeDiagnostic>,
}

struct TargetedFile {
    relative: PathBuf,
    facts: super::scan_fs::FileFacts,
    file: std::fs::File,
}

fn collect_targets(
    db: &SourceDatabase,
    root: &Path,
    source_root: &SourceRootCapability,
    paths: &[PathBuf],
    cancel: Option<&AtomicBool>,
    policy: SourceTraversalPolicy,
) -> Result<TargetedScanTargets, ScanError> {
    let source_root_dir = source_root.clone_root_dir()?;
    let mut current_files = BTreeMap::new();
    let mut existing = HashMap::new();
    let mut current_index_entries = BTreeMap::new();
    let mut existing_index_entries = BTreeMap::new();
    let mut uncertain_prefixes = BTreeSet::new();
    let mut visited = VisitedDirectories::default();
    if !matches!(
        visited.observe(&source_root_dir, root, Path::new("")),
        DirectoryVisit::New
    ) {
        uncertain_prefixes.insert(PathBuf::new());
        return Ok(TargetedScanTargets {
            current_files: Vec::new(),
            existing,
            current_index_entries,
            existing_index_entries,
            uncertain_prefixes,
            diagnostics: visited.diagnostics().to_vec(),
        });
    }
    for relative_path in normalized_targets(paths) {
        if let Some(cancel) = cancel
            && cancel.load(Ordering::Relaxed)
        {
            return Err(ScanError::Canceled);
        }
        // A non-Unicode target cannot have a supported manifest row: normal
        // sample paths remain deliberately UTF-8-only. Its prior state, if
        // any, is carried by the lossless index-only lookup below.
        if relative_path.to_str().is_some() {
            collect_existing_rows(db, &relative_path, &mut existing)?;
        }
        collect_existing_index_entries(db, &relative_path, &mut existing_index_entries)?;
        collect_current_files(
            &source_root_dir,
            root,
            &relative_path,
            cancel,
            policy,
            &mut current_files,
            &mut current_index_entries,
            &mut uncertain_prefixes,
            &mut visited,
        )?;
    }
    Ok(TargetedScanTargets {
        current_files: current_files.into_values().collect(),
        existing,
        current_index_entries,
        existing_index_entries,
        uncertain_prefixes,
        diagnostics: visited.diagnostics().to_vec(),
    })
}

fn normalized_targets(paths: &[PathBuf]) -> BTreeSet<PathBuf> {
    paths
        .iter()
        .filter_map(|path| {
            (path.is_relative()
                && path
                    .components()
                    .all(|component| matches!(component, Component::CurDir | Component::Normal(_))))
            .then(|| {
                path.components()
                    .filter_map(|component| match component {
                        Component::Normal(part) => Some(part),
                        Component::CurDir => None,
                        _ => None,
                    })
                    .collect::<PathBuf>()
            })
        })
        .filter(|path| !path.as_os_str().is_empty())
        .collect()
}

fn collect_existing_rows(
    db: &SourceDatabase,
    relative_path: &Path,
    existing: &mut HashMap<PathBuf, WavEntry>,
) -> Result<(), ScanError> {
    for entry in db.list_files_under_path(relative_path)? {
        existing.entry(entry.relative_path.clone()).or_insert(entry);
    }
    Ok(())
}

fn collect_existing_index_entries(
    db: &SourceDatabase,
    relative_path: &Path,
    existing: &mut BTreeMap<PathBuf, SourceIndexEntry>,
) -> Result<(), ScanError> {
    for entry in db.list_source_index_entries_under_path(relative_path)? {
        existing.entry(entry.relative_path.clone()).or_insert(entry);
    }
    Ok(())
}

fn collect_current_files(
    source_root: &Dir,
    root: &Path,
    relative_path: &Path,
    cancel: Option<&AtomicBool>,
    policy: SourceTraversalPolicy,
    current_files: &mut BTreeMap<PathBuf, TargetedFile>,
    current_index_entries: &mut BTreeMap<PathBuf, SourceIndexEntry>,
    uncertain_prefixes: &mut BTreeSet<PathBuf>,
    visited: &mut VisitedDirectories,
) -> Result<(), ScanError> {
    let Some((parent, name)) = open_target_parent(
        source_root,
        root,
        relative_path,
        uncertain_prefixes,
        visited,
    )?
    else {
        return Ok(());
    };
    let absolute_path = root.join(relative_path);
    let name_path = Path::new(&name);
    let metadata = match read_targeted_metadata(&parent, name_path, &absolute_path) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            if is_rejected_source_file_path(relative_path) {
                uncertain_prefixes.insert(relative_path.to_path_buf());
                return Ok(());
            }
            tracing::warn!(
                path = %absolute_path.display(),
                error = %source,
                "Failed to read targeted sync entry metadata"
            );
            uncertain_prefixes.insert(relative_path.to_path_buf());
            current_index_entries.insert(
                relative_path.to_path_buf(),
                inaccessible_index_entry(
                    relative_path.to_path_buf(),
                    SourceIndexDiagnostic::EntryTypeUnavailable,
                ),
            );
            return Ok(());
        }
    };
    let file_type = metadata.file_type();
    match classify_source_entry_with_policy(
        relative_path,
        targeted_source_entry_file_type(&file_type),
        policy,
    ) {
        SourceEntryClassification::Directory { .. } => {
            let dir = match parent.open_dir_nofollow(name_path) {
                Ok(dir) => dir,
                Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(()),
                Err(source) => {
                    tracing::warn!(
                        path = %root.join(relative_path).display(),
                        error = %source,
                        "Failed to open targeted sync directory without following links"
                    );
                    uncertain_prefixes.insert(relative_path.to_path_buf());
                    return Ok(());
                }
            };
            match visited.observe(&dir, &absolute_path, relative_path) {
                DirectoryVisit::New => collect_current_files_in_dir(
                    dir,
                    root,
                    relative_path,
                    cancel,
                    policy,
                    current_files,
                    current_index_entries,
                    uncertain_prefixes,
                    visited,
                )?,
                DirectoryVisit::AlreadyVisited => {}
                DirectoryVisit::Repeated | DirectoryVisit::IdentityUnavailable => {
                    uncertain_prefixes.insert(relative_path.to_path_buf());
                }
            }
        }
        classification @ SourceEntryClassification::File { .. }
            if classification.indexes_audio() =>
        {
            collect_current_file(
                &parent,
                Path::new(&name),
                root,
                relative_path,
                policy,
                current_files,
                current_index_entries,
                uncertain_prefixes,
            )?;
        }
        classification @ SourceEntryClassification::File { .. } => {
            collect_current_index_file(
                &parent,
                Path::new(&name),
                root,
                relative_path,
                classification,
                current_index_entries,
                uncertain_prefixes,
            )?;
        }
        SourceEntryClassification::Rejected(_) => {}
    }
    Ok(())
}

fn open_target_parent(
    source_root: &Dir,
    root: &Path,
    relative_path: &Path,
    uncertain_prefixes: &mut BTreeSet<PathBuf>,
    visited: &mut VisitedDirectories,
) -> Result<Option<(Dir, std::ffi::OsString)>, ScanError> {
    let Some(name) = relative_path.file_name() else {
        return Ok(None);
    };
    let mut dir = source_root.try_clone().map_err(|source| ScanError::Io {
        path: root.to_path_buf(),
        source,
    })?;
    let mut traversed = PathBuf::new();
    for component in relative_path
        .parent()
        .into_iter()
        .flat_map(Path::components)
    {
        let Component::Normal(part) = component else {
            continue;
        };
        traversed.push(part);
        dir = match dir.open_dir_nofollow(part) {
            Ok(next_dir) => match visited.observe(&next_dir, &root.join(&traversed), &traversed) {
                DirectoryVisit::New | DirectoryVisit::AlreadyVisited => next_dir,
                DirectoryVisit::Repeated | DirectoryVisit::IdentityUnavailable => {
                    uncertain_prefixes.insert(relative_path.to_path_buf());
                    return Ok(None);
                }
            },
            Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                let path = root.join(&traversed);
                if dir
                    .symlink_metadata(part)
                    .is_ok_and(|metadata| metadata.is_symlink())
                {
                    return Ok(None);
                }
                tracing::warn!(
                    path = %path.display(),
                    error = %source,
                    "Failed to open targeted sync parent directory without following links"
                );
                uncertain_prefixes.insert(relative_path.to_path_buf());
                return Ok(None);
            }
        };
    }
    Ok(Some((dir, name.to_os_string())))
}

fn collect_current_files_in_dir(
    start_dir: Dir,
    root: &Path,
    start_relative: &Path,
    cancel: Option<&AtomicBool>,
    policy: SourceTraversalPolicy,
    current_files: &mut BTreeMap<PathBuf, TargetedFile>,
    current_index_entries: &mut BTreeMap<PathBuf, SourceIndexEntry>,
    uncertain_prefixes: &mut BTreeSet<PathBuf>,
    visited: &mut VisitedDirectories,
) -> Result<(), ScanError> {
    let mut stack = vec![(start_dir, start_relative.to_path_buf())];
    while let Some((dir, relative_dir)) = stack.pop() {
        if let Some(cancel) = cancel
            && cancel.load(Ordering::Relaxed)
        {
            return Err(ScanError::Canceled);
        }
        let entries = match read_targeted_dir_entries(&dir, &root.join(&relative_dir)) {
            Ok(entries) => entries,
            Err(source) => {
                tracing::warn!(
                    dir = %root.join(&relative_dir).display(),
                    error = %source,
                    "Failed to read targeted sync directory"
                );
                uncertain_prefixes.insert(relative_dir);
                continue;
            }
        };
        for entry in entries {
            let entry = match read_targeted_dir_entry(entry, &root.join(&relative_dir)) {
                Ok(entry) => entry,
                Err(err) => {
                    tracing::warn!(
                        dir = %root.join(&relative_dir).display(),
                        error = %err,
                        "Failed to read targeted sync directory entry"
                    );
                    uncertain_prefixes.insert(relative_dir.clone());
                    continue;
                }
            };
            let name = entry.file_name();
            let name_path = Path::new(&name);
            let path = root.join(&relative_dir).join(name_path);
            let file_type = match read_targeted_file_type(&entry, &path) {
                Ok(file_type) => file_type,
                Err(err) => {
                    let relative_path = relative_dir.join(name_path);
                    if is_rejected_source_file_path(&relative_path) {
                        uncertain_prefixes.insert(relative_path);
                        continue;
                    }
                    tracing::warn!(
                        path = %path.display(),
                        error = %err,
                        "Failed to read targeted sync file type"
                    );
                    uncertain_prefixes.insert(relative_path.clone());
                    current_index_entries.insert(
                        relative_path.clone(),
                        inaccessible_index_entry(
                            relative_path,
                            SourceIndexDiagnostic::EntryTypeUnavailable,
                        ),
                    );
                    continue;
                }
            };
            let relative_path = relative_dir.join(name_path);
            match classify_source_entry_with_policy(
                &relative_path,
                targeted_source_entry_file_type(&file_type),
                policy,
            ) {
                SourceEntryClassification::Directory { .. } => {
                    let child_dir = match dir.open_dir_nofollow(name_path) {
                        Ok(child_dir) => child_dir,
                        Err(source) if source.kind() == io::ErrorKind::NotFound => continue,
                        Err(source) => {
                            tracing::warn!(
                                path = %path.display(),
                                error = %source,
                                "Failed to open targeted sync directory without following links"
                            );
                            if !dir
                                .symlink_metadata(name_path)
                                .is_ok_and(|metadata| metadata.is_symlink())
                            {
                                uncertain_prefixes.insert(relative_dir.join(name_path));
                            }
                            continue;
                        }
                    };
                    match visited.observe(&child_dir, &path, &relative_path) {
                        DirectoryVisit::New => stack.push((child_dir, relative_path)),
                        DirectoryVisit::AlreadyVisited => {}
                        DirectoryVisit::Repeated | DirectoryVisit::IdentityUnavailable => {
                            uncertain_prefixes.insert(relative_path);
                        }
                    }
                }
                classification @ SourceEntryClassification::File { .. }
                    if classification.indexes_audio() =>
                {
                    collect_current_file(
                        &dir,
                        name_path,
                        root,
                        &relative_path,
                        policy,
                        current_files,
                        current_index_entries,
                        uncertain_prefixes,
                    )?;
                }
                classification @ SourceEntryClassification::File { .. } => {
                    collect_current_index_file(
                        &dir,
                        name_path,
                        root,
                        &relative_path,
                        classification,
                        current_index_entries,
                        uncertain_prefixes,
                    )?;
                }
                SourceEntryClassification::Rejected(_) => {}
            }
        }
    }
    Ok(())
}

fn collect_current_file(
    parent: &Dir,
    name: &Path,
    root: &Path,
    relative_path: &Path,
    policy: SourceTraversalPolicy,
    current_files: &mut BTreeMap<PathBuf, TargetedFile>,
    current_index_entries: &mut BTreeMap<PathBuf, SourceIndexEntry>,
    uncertain_prefixes: &mut BTreeSet<PathBuf>,
) -> Result<(), ScanError> {
    let absolute_path = root.join(relative_path);
    if !classify_source_entry_with_policy(relative_path, SourceEntryFileType::File, policy)
        .indexes_audio()
    {
        return Ok(());
    }
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let file = match parent.open_with(name, &options) {
        Ok(file) => file.into_std(),
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            // The no-follow open is the source-boundary decision. A file that
            // became a link after directory enumeration is rejected here.
            tracing::warn!(
                path = %absolute_path.display(),
                error = %source,
                "Skipping targeted sync file that could not be opened without following links"
            );
            if !parent
                .symlink_metadata(name)
                .is_ok_and(|metadata| metadata.is_symlink())
            {
                uncertain_prefixes.insert(relative_path.to_path_buf());
                current_index_entries.insert(
                    relative_path.to_path_buf(),
                    inaccessible_index_entry(
                        relative_path.to_path_buf(),
                        SourceIndexDiagnostic::OpenUnavailable,
                    ),
                );
            }
            return Ok(());
        }
    };
    let facts = match super::scan_fs::read_facts_from_open_file(root, &absolute_path, &file) {
        Ok(facts) => facts,
        Err(error) => {
            tracing::warn!(
                path = %absolute_path.display(),
                %error,
                "Skipping targeted sync file with unavailable metadata"
            );
            uncertain_prefixes.insert(relative_path.to_path_buf());
            current_index_entries.insert(
                relative_path.to_path_buf(),
                inaccessible_index_entry(
                    relative_path.to_path_buf(),
                    SourceIndexDiagnostic::MetadataUnavailable,
                ),
            );
            return Ok(());
        }
    };
    if relative_path.to_str().is_none() {
        current_index_entries.insert(
            relative_path.to_path_buf(),
            non_unicode_index_entry(
                relative_path.to_path_buf(),
                Some(facts.size),
                Some(facts.modified_ns),
                facts.file_identity,
            ),
        );
        return Ok(());
    }
    current_files
        .entry(relative_path.to_path_buf())
        .or_insert(TargetedFile {
            relative: relative_path.to_path_buf(),
            facts,
            file,
        });
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn collect_current_index_file(
    parent: &Dir,
    name: &Path,
    root: &Path,
    relative_path: &Path,
    classification: SourceEntryClassification,
    current_index_entries: &mut BTreeMap<PathBuf, SourceIndexEntry>,
    uncertain_prefixes: &mut BTreeSet<PathBuf>,
) -> Result<(), ScanError> {
    let Some(file_classification) = classification.file_classification() else {
        return Ok(());
    };
    if file_classification == SourceFileClassification::SupportedAudio {
        return Ok(());
    }
    let absolute_path = root.join(relative_path);
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let file = match parent.open_with(name, &options) {
        Ok(file) => file.into_std(),
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            if !parent
                .symlink_metadata(name)
                .is_ok_and(|metadata| metadata.is_symlink())
            {
                tracing::warn!(
                    path = %absolute_path.display(),
                    error = %source,
                    "Targeted index-only file could not be opened without following links"
                );
                uncertain_prefixes.insert(relative_path.to_path_buf());
                current_index_entries.insert(
                    relative_path.to_path_buf(),
                    inaccessible_index_entry(
                        relative_path.to_path_buf(),
                        SourceIndexDiagnostic::OpenUnavailable,
                    ),
                );
            }
            return Ok(());
        }
    };
    let facts = match super::scan_fs::read_facts_from_open_file(root, &absolute_path, &file) {
        Ok(facts) => facts,
        Err(error) => {
            tracing::warn!(
                path = %absolute_path.display(),
                %error,
                "Targeted index-only file metadata is unavailable"
            );
            uncertain_prefixes.insert(relative_path.to_path_buf());
            current_index_entries.insert(
                relative_path.to_path_buf(),
                inaccessible_index_entry(
                    relative_path.to_path_buf(),
                    SourceIndexDiagnostic::MetadataUnavailable,
                ),
            );
            return Ok(());
        }
    };
    if let Some(entry) = index_entry_from_file_facts(
        relative_path.to_path_buf(),
        file_classification,
        facts.size,
        facts.modified_ns,
        facts.file_identity,
    ) {
        current_index_entries.insert(relative_path.to_path_buf(), entry);
    }
    Ok(())
}

fn read_targeted_dir_entries(
    dir: &Dir,
    _absolute_path: &Path,
) -> Result<cap_std::fs::ReadDir, io::Error> {
    #[cfg(test)]
    if let Some(error) = super::scan_fs::forced_directory_read_error(_absolute_path) {
        return Err(error);
    }
    dir.entries()
}

fn read_targeted_metadata(
    parent: &Dir,
    name: &Path,
    _absolute_path: &Path,
) -> Result<cap_std::fs::Metadata, io::Error> {
    #[cfg(test)]
    if let Some(error) = super::scan_fs::forced_file_type_error(_absolute_path) {
        return Err(error);
    }
    parent.symlink_metadata(name)
}

fn read_targeted_file_type(
    entry: &cap_std::fs::DirEntry,
    _absolute_path: &Path,
) -> Result<cap_std::fs::FileType, io::Error> {
    #[cfg(test)]
    if let Some(error) = super::scan_fs::forced_file_type_error(_absolute_path) {
        return Err(error);
    }
    entry.file_type()
}

fn read_targeted_dir_entry(
    entry: Result<cap_std::fs::DirEntry, io::Error>,
    _absolute_directory: &Path,
) -> Result<cap_std::fs::DirEntry, io::Error> {
    #[cfg(test)]
    if let Some(error) = super::scan_fs::forced_directory_entry_error(_absolute_directory) {
        return Err(error);
    }
    entry
}

fn targeted_source_entry_file_type(file_type: &cap_std::fs::FileType) -> SourceEntryFileType {
    SourceEntryFileType::from_no_followed_type(
        file_type.is_dir(),
        file_type.is_file(),
        file_type.is_symlink(),
    )
}
