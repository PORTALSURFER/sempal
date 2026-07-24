use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use super::{
    super::{
        FileEntry, FolderEntry,
        collections::MissingCollectionSnapshot,
        path_helpers::{folder_label, path_id},
        scan_types::{
            FolderScanDiscovery, FolderScanItem, FolderScanLifecycle, FolderScanProgress,
            FolderScanRequest, FolderScanResult, MetadataHydrationStatus,
        },
    },
    entry::{BrowserEntryKind, classify_path_without_following},
    file_entry_metadata::{file_entry_with_metadata, file_entry_with_snapshot_metadata},
    metadata::SourceMetadataMap,
    traversal::placeholder_folder,
};
use wavecrate::sample_sources::{Rating, SourceDatabase};
use wavecrate_library::sample_sources::BrowserFileMetadata;
#[cfg(test)]
use wavecrate_scan::sample_sources::scanner::UncoordinatedScanWriter;
use wavecrate_scan::{
    ScanStats, SourceTreeSnapshot,
    sample_sources::scanner::{
        self, CommittedScanBatch, CommittedSourceDelta, ScanWritePhase, ScanWriter,
    },
};

struct CommittedSourceTreeSnapshot {
    delta: CommittedSourceDelta,
    layout: SourceTreeSnapshot,
}

#[derive(Default)]
struct ProvisionalDiscoveryState {
    emitted_root: bool,
}

impl ProvisionalDiscoveryState {
    fn clear(&mut self) {
        self.emitted_root = false;
    }
}

/// Publish at most one source-index progress update per bounded file batch.
pub(in crate::native_app) const INDEX_PROGRESS_REPORT_INTERVAL: usize = 128;

#[cfg(test)]
pub(in crate::native_app) fn scan_source_with_progress(
    request: FolderScanRequest,
    progress: impl FnMut(FolderScanProgress),
    discovered: impl FnMut(FolderScanDiscovery),
) -> FolderScanResult {
    scan_source_with_progress_cancellable(
        request,
        progress,
        discovered,
        &AtomicBool::new(false),
        &UncoordinatedScanWriter,
    )
}

pub(in crate::native_app) fn scan_source_with_progress_cancellable(
    request: FolderScanRequest,
    mut progress: impl FnMut(FolderScanProgress),
    mut discovered: impl FnMut(FolderScanDiscovery),
    cancel: &AtomicBool,
    writer: &impl ScanWriter,
) -> FolderScanResult {
    let source_root_available =
        classify_path_without_following(&request.root) == Some(BrowserEntryKind::Directory);
    let mut provisional = ProvisionalDiscoveryState::default();
    let (source_db_error, source_tree_snapshot) = if source_root_available {
        sync_source_database(
            &request,
            &mut progress,
            &mut discovered,
            &mut provisional,
            cancel,
            writer,
        )
    } else {
        (None, None)
    };
    let projection = if source_root_available && !cancel.load(Ordering::Acquire) {
        build_committed_projection(
            &request,
            source_tree_snapshot,
            &mut discovered,
            &mut provisional,
            cancel,
        )
    } else {
        Err(String::from("source projection was not attempted"))
    };
    let (folder, ratings, metadata_hydration, committed_delta) = match projection {
        Ok((folder, ratings, committed_delta)) => (
            folder,
            ratings,
            MetadataHydrationStatus::Complete {
                revision: committed_delta.revision,
            },
            Some(committed_delta),
        ),
        Err(error) if source_root_available && !cancel.load(Ordering::Acquire) => {
            if provisional.emitted_root {
                discovered(reset_discovery(&request, None));
                provisional.clear();
            }
            (
                placeholder_folder(&request.root),
                SourceMetadataMap::new(),
                MetadataHydrationStatus::Failed { error },
                None,
            )
        }
        Err(_) => (
            placeholder_folder(&request.root),
            SourceMetadataMap::new(),
            MetadataHydrationStatus::NotAttempted,
            None,
        ),
    };
    let publish_discoveries = false;
    let mut scan = ScanProgressContext {
        request: &request,
        ratings,
        counter: ScanProgressCounter {
            completed: 0,
            files: 0,
            folders: 0,
        },
        progress: &mut progress,
        discovered: &mut discovered,
        cancel,
        publish_discoveries,
        committed_revision: committed_delta.as_ref().map(|delta| delta.revision),
    };
    scan.report_initial();
    publish_projection(&folder, &mut scan);
    let missing_collection_snapshot =
        MissingCollectionSnapshot::from_source_metadata(&request.root, &folder, &scan.ratings);
    let file_count = scan.counter.files;
    let folder_count = scan.counter.folders;
    drop(scan);
    FolderScanResult {
        task_id: request.task_id,
        source_id: request.source_id,
        label: request.label,
        folder,
        missing_collection_snapshot,
        file_count,
        folder_count,
        source_db_error,
        metadata_hydration,
        committed_delta,
        source_root_available,
        cancelled: cancel.load(Ordering::Acquire),
    }
}

fn sync_source_database(
    request: &FolderScanRequest,
    progress: &mut impl FnMut(FolderScanProgress),
    discovered: &mut impl FnMut(FolderScanDiscovery),
    provisional: &mut ProvisionalDiscoveryState,
    cancel: &AtomicBool,
    writer: &impl ScanWriter,
) -> (Option<String>, Option<CommittedSourceTreeSnapshot>) {
    let _writer = writer.lock(ScanWritePhase::Open);
    if cancel.load(Ordering::Acquire) {
        return (
            Some(String::from("source scan canceled before database open")),
            None,
        );
    }
    let db = match SourceDatabase::open_for_background_job_with_database_root(
        &request.root,
        &request.database_root,
    ) {
        Ok(db) => db,
        Err(err) => return (Some(format!("open source index: {err}")), None),
    };
    drop(_writer);
    let mut publish_committed_batch = |batch: CommittedScanBatch| {
        if cancel.load(Ordering::Acquire) {
            return;
        }
        if !provisional.emitted_root {
            discovered(reset_discovery(request, Some(batch.revision)));
            provisional.emitted_root = true;
        }
        let mut folders = BTreeSet::new();
        for path in &batch.paths {
            let mut parent = path.parent().map(Path::to_path_buf);
            while let Some(folder) = parent.filter(|path| !path.as_os_str().is_empty()) {
                let next_parent = folder.parent().map(Path::to_path_buf);
                folders.insert(folder);
                parent = next_parent;
            }
        }
        for folder in folders {
            let absolute = request.root.join(&folder);
            let parent = folder.parent().unwrap_or_else(|| Path::new(""));
            let parent_absolute = if parent.as_os_str().is_empty() {
                request.root.clone()
            } else {
                request.root.join(parent)
            };
            discovered(FolderScanDiscovery {
                task_id: request.task_id,
                source_id: request.source_id.clone(),
                committed_revision: Some(batch.revision),
                parent_id: path_id(&parent_absolute),
                item: FolderScanItem::Folder(placeholder_folder(&absolute)),
            });
        }
        for path in batch.paths {
            let absolute = request.root.join(&path);
            let parent = absolute.parent().unwrap_or(&request.root);
            discovered(FolderScanDiscovery {
                task_id: request.task_id,
                source_id: request.source_id.clone(),
                committed_revision: Some(batch.revision),
                parent_id: path_id(parent),
                item: FolderScanItem::File(file_entry_with_metadata(
                    &absolute,
                    Rating::NEUTRAL,
                    false,
                    Vec::new(),
                    None,
                    None,
                )),
            });
        }
    };
    let mut sync_progress = |completed: usize, path: &Path| {
        if completed != 1 && !completed.is_multiple_of(INDEX_PROGRESS_REPORT_INTERVAL) {
            return;
        }
        progress(FolderScanProgress::new(
            request.task_id,
            request.source_id.clone(),
            request.label.clone(),
            FolderScanLifecycle::Scanning,
            completed,
            0,
            format!("Indexing | {}", path.display()),
        ));
    };
    let stats = match scanner::scan_with_progress_and_writer_and_committed_batch(
        &db,
        scanner::ScanMode::Quick,
        Some(cancel),
        &mut sync_progress,
        &mut publish_committed_batch,
        writer,
    ) {
        Ok(stats) => stats,
        Err(err) => {
            if !cancel.load(Ordering::Acquire) && provisional.emitted_root {
                discovered(reset_discovery(request, None));
                provisional.clear();
            }
            return (Some(format!("sync source index: {err}")), None);
        }
    };
    let fallback_snapshot =
        stats
            .source_tree_snapshot
            .clone()
            .map(|layout| CommittedSourceTreeSnapshot {
                delta: stats.committed_delta.clone(),
                layout,
            });
    match scanner::complete_deferred_rename_candidates_with_cancel_and_writer(
        &db,
        stats,
        Some(cancel),
        writer,
    ) {
        Ok(ScanStats {
            committed_delta,
            source_tree_snapshot,
            ..
        }) => (
            None,
            source_tree_snapshot.map(|layout| CommittedSourceTreeSnapshot {
                delta: committed_delta,
                layout,
            }),
        ),
        Err(err) => (
            Some(format!("finish deferred rename hashing: {err}")),
            fallback_snapshot,
        ),
    }
}

fn reset_discovery(request: &FolderScanRequest, revision: Option<u64>) -> FolderScanDiscovery {
    FolderScanDiscovery {
        task_id: request.task_id,
        source_id: request.source_id.clone(),
        committed_revision: revision,
        parent_id: path_id(&request.root),
        item: FolderScanItem::ResetFolder,
    }
}

#[derive(Default)]
struct ProjectionFolder {
    children: Vec<PathBuf>,
    files: Vec<FileEntry>,
}

fn build_committed_projection(
    request: &FolderScanRequest,
    source_tree_snapshot: Option<CommittedSourceTreeSnapshot>,
    discovered: &mut impl FnMut(FolderScanDiscovery),
    provisional: &mut ProvisionalDiscoveryState,
    cancel: &AtomicBool,
) -> Result<(FolderEntry, SourceMetadataMap, CommittedSourceDelta), String> {
    let started_at = Instant::now();
    let committed = source_tree_snapshot
        .ok_or_else(|| String::from("authoritative source traversal did not produce a layout"))?;
    let layout = committed.layout;
    if !layout.is_complete() {
        return Err(format!(
            "authoritative source traversal was incomplete: {}",
            layout
                .diagnostics
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    let db =
        SourceDatabase::open_for_ui_read_with_database_root(&request.root, &request.database_root)
            .map_err(|error| error.to_string())?;
    let revision = committed.delta.revision;
    let mut files = Vec::new();
    let mut cursor = None;
    loop {
        if cancel.load(Ordering::Acquire) {
            return Err(String::from("source projection canceled"));
        }
        let page = db
            .browser_metadata_page(revision, cursor.as_deref(), 64)
            .map_err(|error| error.to_string())?;
        files.extend(page.files);
        cursor = page.next_path;
        if cursor.is_none() {
            break;
        }
    }
    emit_authoritative_discoveries(request, &layout, &files, revision, discovered, provisional);
    let ratings = files
        .iter()
        .map(|entry| {
            (
                entry.relative_path.clone(),
                (
                    entry.rating,
                    entry.locked,
                    entry.collections.clone(),
                    entry.last_played_at,
                    entry.last_curated_at,
                ),
            )
        })
        .collect::<SourceMetadataMap>();

    let mut folders = layout
        .directories
        .iter()
        .cloned()
        .map(|path| (path, ProjectionFolder::default()))
        .collect::<BTreeMap<_, _>>();
    folders.entry(PathBuf::new()).or_default();
    for directory in layout
        .directories
        .iter()
        .filter(|path| !path.as_os_str().is_empty())
    {
        let parent = directory.parent().unwrap_or_else(|| Path::new(""));
        folders
            .entry(parent.to_path_buf())
            .or_default()
            .children
            .push(directory.clone());
    }
    for entry in files.iter().filter(|entry| !entry.missing) {
        let absolute = request.root.join(&entry.relative_path);
        let file = file_entry_with_snapshot_metadata(
            &absolute,
            entry.file_size,
            entry.rating,
            entry.locked,
            entry.collections.clone(),
            entry.last_played_at,
            entry.last_curated_at,
        );
        folders
            .entry(
                entry
                    .relative_path
                    .parent()
                    .unwrap_or_else(|| Path::new(""))
                    .to_path_buf(),
            )
            .or_default()
            .files
            .push(file);
    }
    let other_file_count = layout.other_files.len();
    for entry in layout.other_files {
        let absolute = request.root.join(&entry.relative_path);
        let file = file_entry_with_snapshot_metadata(
            &absolute,
            entry.file_size,
            Rating::NEUTRAL,
            false,
            Vec::new(),
            None,
            None,
        );
        let parent = entry
            .relative_path
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .to_path_buf();
        let destination = folders.entry(parent).or_default();
        if !destination
            .files
            .iter()
            .any(|existing| existing.id == file.id)
        {
            destination.files.push(file);
        }
    }
    for folder in folders.values_mut() {
        folder.children.sort_by(|left, right| {
            folder_label(&request.root.join(left))
                .to_ascii_lowercase()
                .cmp(&folder_label(&request.root.join(right)).to_ascii_lowercase())
        });
        folder.files.sort_by_key(FileEntry::name_sort_key);
    }
    let folder_count = folders.len();
    let file_count = files.iter().filter(|entry| !entry.missing).count() + other_file_count;
    let folder = materialize_projection_folder(&request.root, Path::new(""), &folders);
    tracing::info!(
        source_id = request.source_id,
        revision,
        filesystem_traversals = 1,
        metadata_pages = (files.len().saturating_add(63)) / 64,
        folder_count,
        file_count,
        elapsed_ms = started_at.elapsed().as_millis(),
        "Built browser projection from committed source snapshot"
    );
    Ok((folder, ratings, committed.delta))
}

fn emit_authoritative_discoveries(
    request: &FolderScanRequest,
    layout: &SourceTreeSnapshot,
    files: &[BrowserFileMetadata],
    revision: u64,
    discovered: &mut impl FnMut(FolderScanDiscovery),
    provisional: &mut ProvisionalDiscoveryState,
) {
    if provisional.emitted_root {
        return;
    }
    discovered(reset_discovery(request, Some(revision)));
    provisional.emitted_root = true;
    for folder in layout
        .directories
        .iter()
        .filter(|path| !path.as_os_str().is_empty())
    {
        let absolute = request.root.join(folder);
        let parent = folder.parent().unwrap_or_else(|| Path::new(""));
        let parent_absolute = if parent.as_os_str().is_empty() {
            request.root.clone()
        } else {
            request.root.join(parent)
        };
        discovered(FolderScanDiscovery {
            task_id: request.task_id,
            source_id: request.source_id.clone(),
            committed_revision: Some(revision),
            parent_id: path_id(&parent_absolute),
            item: FolderScanItem::Folder(placeholder_folder(&absolute)),
        });
        discovered(FolderScanDiscovery {
            task_id: request.task_id,
            source_id: request.source_id.clone(),
            committed_revision: Some(revision),
            parent_id: path_id(&absolute),
            item: FolderScanItem::ResetFolder,
        });
    }
    for entry in files.iter().filter(|entry| !entry.missing) {
        let absolute = request.root.join(&entry.relative_path);
        let parent = entry
            .relative_path
            .parent()
            .unwrap_or_else(|| Path::new(""));
        let parent_absolute = if parent.as_os_str().is_empty() {
            request.root.clone()
        } else {
            request.root.join(parent)
        };
        discovered(FolderScanDiscovery {
            task_id: request.task_id,
            source_id: request.source_id.clone(),
            committed_revision: Some(revision),
            parent_id: path_id(&parent_absolute),
            item: FolderScanItem::File(file_entry_with_snapshot_metadata(
                &absolute,
                entry.file_size,
                entry.rating,
                entry.locked,
                entry.collections.clone(),
                entry.last_played_at,
                entry.last_curated_at,
            )),
        });
    }
    for entry in &layout.other_files {
        let absolute = request.root.join(&entry.relative_path);
        let parent = entry
            .relative_path
            .parent()
            .unwrap_or_else(|| Path::new(""));
        let parent_absolute = if parent.as_os_str().is_empty() {
            request.root.clone()
        } else {
            request.root.join(parent)
        };
        discovered(FolderScanDiscovery {
            task_id: request.task_id,
            source_id: request.source_id.clone(),
            committed_revision: Some(revision),
            parent_id: path_id(&parent_absolute),
            item: FolderScanItem::File(file_entry_with_snapshot_metadata(
                &absolute,
                entry.file_size,
                Rating::NEUTRAL,
                false,
                Vec::new(),
                None,
                None,
            )),
        });
    }
}

fn materialize_projection_folder(
    root: &Path,
    relative: &Path,
    folders: &BTreeMap<PathBuf, ProjectionFolder>,
) -> FolderEntry {
    let absolute = if relative.as_os_str().is_empty() {
        root.to_path_buf()
    } else {
        root.join(relative)
    };
    let projection = folders.get(relative);
    FolderEntry {
        id: path_id(&absolute),
        name: folder_label(&absolute),
        children: projection
            .map(|folder| {
                folder
                    .children
                    .iter()
                    .map(|child| materialize_projection_folder(root, child, folders))
                    .collect()
            })
            .unwrap_or_default(),
        files: projection
            .map(|folder| folder.files.clone())
            .unwrap_or_default(),
    }
}

struct ScanProgressCounter {
    completed: usize,
    files: usize,
    folders: usize,
}

struct ScanProgressContext<'a, P, D>
where
    P: FnMut(FolderScanProgress),
    D: FnMut(FolderScanDiscovery),
{
    request: &'a FolderScanRequest,
    ratings: SourceMetadataMap,
    counter: ScanProgressCounter,
    progress: &'a mut P,
    discovered: &'a mut D,
    cancel: &'a AtomicBool,
    publish_discoveries: bool,
    committed_revision: Option<u64>,
}

impl<P, D> ScanProgressContext<'_, P, D>
where
    P: FnMut(FolderScanProgress),
    D: FnMut(FolderScanDiscovery),
{
    fn report_initial(&mut self) {
        (self.progress)(FolderScanProgress::new(
            self.request.task_id,
            self.request.source_id.clone(),
            self.request.label.clone(),
            FolderScanLifecycle::Scanning,
            0,
            0,
            self.request.root.display().to_string(),
        ));
    }

    fn record_folder(&mut self, path: &Path, parent_id: &str) {
        self.counter.completed += 1;
        self.counter.folders += 1;
        self.maybe_report_progress(path);
        if self.publish_discoveries {
            (self.discovered)(FolderScanDiscovery {
                task_id: self.request.task_id,
                source_id: self.request.source_id.clone(),
                committed_revision: self.committed_revision,
                parent_id: parent_id.to_string(),
                item: FolderScanItem::Folder(placeholder_folder(path)),
            });
        }
    }

    fn record_folder_snapshot_start(&mut self, folder_id: &str) {
        if self.publish_discoveries {
            (self.discovered)(FolderScanDiscovery {
                task_id: self.request.task_id,
                source_id: self.request.source_id.clone(),
                committed_revision: self.committed_revision,
                parent_id: folder_id.to_string(),
                item: FolderScanItem::ResetFolder,
            });
        }
    }

    fn record_file(&mut self, path: &Path, parent_id: &str, file: FileEntry) {
        self.counter.completed += 1;
        self.counter.files += 1;
        self.maybe_report_progress(path);
        if self.publish_discoveries {
            (self.discovered)(FolderScanDiscovery {
                task_id: self.request.task_id,
                source_id: self.request.source_id.clone(),
                committed_revision: self.committed_revision,
                parent_id: parent_id.to_string(),
                item: FolderScanItem::File(file),
            });
        }
    }

    fn maybe_report_progress(&mut self, path: &Path) {
        if self.counter.completed == 1 || self.counter.completed.is_multiple_of(64) {
            (self.progress)(FolderScanProgress::new(
                self.request.task_id,
                self.request.source_id.clone(),
                self.request.label.clone(),
                FolderScanLifecycle::Scanning,
                self.counter.completed,
                0,
                path.display().to_string(),
            ));
        }
    }
}

fn publish_projection<P, D>(folder: &FolderEntry, scan: &mut ScanProgressContext<'_, P, D>)
where
    P: FnMut(FolderScanProgress),
    D: FnMut(FolderScanDiscovery),
{
    if scan.cancel.load(Ordering::Acquire) {
        return;
    }
    let path = PathBuf::from(&folder.id);
    let parent_id = folder.id.clone();
    scan.record_folder_snapshot_start(&parent_id);
    for child in &folder.children {
        if scan.cancel.load(Ordering::Acquire) {
            return;
        }
        let child_path = PathBuf::from(&child.id);
        scan.record_folder(&child_path, &parent_id);
        publish_projection(child, scan);
    }
    for file in &folder.files {
        if scan.cancel.load(Ordering::Acquire) {
            return;
        }
        scan.record_file(&path.join(&file.name), &parent_id, file.clone());
    }
}
