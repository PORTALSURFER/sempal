use super::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[cfg(not(test))]
use std::sync::mpsc::Sender;

pub(super) enum TrashMoveMessage {
    SetTotal(usize),
    Progress {
        completed: usize,
        detail: Option<String>,
    },
    Finished(TrashMoveFinished),
}

pub(super) struct TrashMoveFinished {
    pub(super) total: usize,
    pub(super) moved: usize,
    pub(super) cancelled: bool,
    pub(super) errors: Vec<String>,
    pub(super) collections_changed: bool,
    pub(super) collections: Vec<Collection>,
    pub(super) affected_sources: Vec<SourceId>,
}

#[cfg(not(test))]
pub(super) fn run_trash_move_task(
    sources: Vec<SampleSource>,
    collections: Vec<Collection>,
    trash_root: PathBuf,
    cancel: Arc<AtomicBool>,
    sender: Option<&Sender<TrashMoveMessage>>,
) -> TrashMoveFinished {
    run_trash_move_task_with_progress(
        sources,
        collections,
        trash_root,
        cancel,
        |message| {
            if let Some(tx) = sender {
                let _ = tx.send(message);
            }
        },
    )
}

pub(super) fn run_trash_move_task_with_progress<F>(
    sources: Vec<SampleSource>,
    mut collections: Vec<Collection>,
    trash_root: PathBuf,
    cancel: Arc<AtomicBool>,
    mut on_message: F,
) -> TrashMoveFinished
where
    F: FnMut(TrashMoveMessage),
{
    let mut errors = Vec::new();
    let mut trashed_by_source: Vec<(SampleSource, Vec<WavEntry>)> = Vec::new();
    for source in sources {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let db = match SourceDatabase::open(&source.root) {
            Ok(db) => db,
            Err(err) => {
                errors.push(format!("{}: {err}", source.root.display()));
                continue;
            }
        };
        let entries = match db.list_files() {
            Ok(entries) => entries,
            Err(err) => {
                errors.push(format!("{}: {err}", source.root.display()));
                continue;
            }
        };
        let trashed: Vec<WavEntry> = entries
            .into_iter()
            .filter(|entry| entry.tag == SampleTag::Trash)
            .collect();
        if !trashed.is_empty() {
            trashed_by_source.push((source, trashed));
        }
    }

    let total: usize = trashed_by_source
        .iter()
        .map(|(_, entries)| entries.len())
        .sum();
    on_message(TrashMoveMessage::SetTotal(total));

    if total == 0 {
        return TrashMoveFinished {
            total,
            moved: 0,
            cancelled: cancel.load(Ordering::Relaxed),
            errors,
            collections_changed: false,
            collections,
            affected_sources: Vec::new(),
        };
    }

    #[derive(Hash, PartialEq, Eq)]
    struct MovedKey {
        source_id: SourceId,
        relative_path: PathBuf,
    }

    let mut moved = 0usize;
    let mut completed = 0usize;
    let mut moved_keys: std::collections::HashSet<MovedKey> = std::collections::HashSet::new();
    let mut affected_sources: std::collections::HashSet<SourceId> =
        std::collections::HashSet::new();

    for (source, entries) in trashed_by_source {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let db = match SourceDatabase::open(&source.root) {
            Ok(db) => db,
            Err(err) => {
                errors.push(format!("{}: {err}", source.root.display()));
                continue;
            }
        };
        for entry in entries {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            let detail = format!("Moving {}", entry.relative_path.display());
            if completed % 5 == 0 {
                on_message(TrashMoveMessage::Progress {
                    completed,
                    detail: Some(detail.clone()),
                });
            }

            match move_to_trash(&source, &entry, &trash_root) {
                Ok(()) => {
                    if let Err(err) = db.remove_file(&entry.relative_path) {
                        errors.push(format!(
                            "Failed to drop database row for {}: {err}",
                            entry.relative_path.display()
                        ));
                    } else {
                        moved += 1;
                        moved_keys.insert(MovedKey {
                            source_id: source.id.clone(),
                            relative_path: entry.relative_path.clone(),
                        });
                        affected_sources.insert(source.id.clone());
                    }
                }
                Err(err) => errors.push(err),
            }

            completed += 1;
            on_message(TrashMoveMessage::Progress {
                completed,
                detail: Some(detail),
            });
        }
    }

    let mut collections_changed = false;
    if !moved_keys.is_empty() {
        for collection in &mut collections {
            let before = collection.members.len();
            collection.members.retain(|member| {
                !moved_keys.contains(&MovedKey {
                    source_id: member.source_id.clone(),
                    relative_path: member.relative_path.clone(),
                })
            });
            collections_changed |= before != collection.members.len();
        }
    }

    TrashMoveFinished {
        total,
        moved,
        cancelled: cancel.load(Ordering::Relaxed),
        errors,
        collections_changed,
        collections,
        affected_sources: affected_sources.into_iter().collect(),
    }
}

fn unique_destination(root: &Path, relative: &Path) -> Result<PathBuf, String> {
    let mut candidate = root.join(relative);
    if !candidate.exists() {
        return Ok(candidate);
    }
    let parent = candidate
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| root.to_path_buf());
    let stem = relative
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    let ext = relative.extension().and_then(|e| e.to_str()).unwrap_or("");
    for idx in 1..=1000 {
        let mut name = format!("{stem}_{idx}");
        if !ext.is_empty() {
            name.push('.');
            name.push_str(ext);
        }
        candidate = parent.join(name);
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err("Could not create unique trash destination".into())
}

fn move_to_trash(source: &SampleSource, entry: &WavEntry, trash_root: &Path) -> Result<(), String> {
    let absolute = source.root.join(&entry.relative_path);
    if !absolute.is_file() {
        return Err(format!("File not found for trash: {}", absolute.display()));
    }
    let destination = unique_destination(trash_root, &entry.relative_path)?;
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("Unable to prepare trash folder {}: {err}", parent.display()))?;
    }
    if let Err(err) = fs::rename(&absolute, &destination) {
        fs::copy(&absolute, &destination).map_err(|copy_err| {
            format!(
                "Failed to move {} to trash: rename error {err}; copy error {copy_err}",
                absolute.display()
            )
        })?;
        fs::remove_file(&absolute).map_err(|remove_err| {
            format!(
                "Failed to remove original {} after copy: {remove_err}",
                absolute.display()
            )
        })?;
    }
    Ok(())
}
