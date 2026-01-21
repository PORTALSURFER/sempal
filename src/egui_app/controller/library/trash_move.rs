use super::*;
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(not(test))]
use std::sync::mpsc::Sender;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Debug)]
pub(crate) enum TrashMoveMessage {
    SetTotal(usize),
    Progress {
        completed: usize,
        detail: Option<String>,
    },
    Finished(TrashMoveFinished),
}

#[derive(Clone)]
pub(crate) struct TrashMoveFinished {
    pub(crate) total: usize,
    pub(crate) moved: usize,
    pub(crate) cancelled: bool,
    pub(crate) errors: Vec<String>,
    pub(crate) affected_sources: Vec<SourceId>,
}

#[cfg(not(test))]
pub(crate) fn run_trash_move_task(
    sources: Vec<SampleSource>,
    trash_root: PathBuf,
    cancel: Arc<AtomicBool>,
    sender: Option<&Sender<TrashMoveMessage>>,
) -> TrashMoveFinished {
    run_trash_move_task_with_progress(
        sources,
        trash_root,
        cancel,
        |message| {
            if let Some(tx) = sender {
                let _ = tx.send(message);
            }
        },
        move_to_trash,
    )
}

pub(crate) fn run_trash_move_task_with_progress<F, M>(
    sources: Vec<SampleSource>,
    trash_root: PathBuf,
    cancel: Arc<AtomicBool>,
    mut on_message: F,
    mut mover: M,
) -> TrashMoveFinished
where
    F: FnMut(TrashMoveMessage),
    M: FnMut(&SampleSource, &WavEntry, &Path) -> Result<(), String>,
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
        let trashed = match db.list_files_by_tag(crate::sample_sources::Rating::TRASH_3) {
            Ok(entries) => entries,
            Err(err) => {
                errors.push(format!("{}: {err}", source.root.display()));
                continue;
            }
        };
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
        let finished = TrashMoveFinished {
            total,
            moved: 0,
            cancelled: cancel.load(Ordering::Relaxed),
            errors,
            affected_sources: Vec::new(),
        };
        on_message(TrashMoveMessage::Finished(finished.clone()));
        return finished;
    }

    let mut moved = 0usize;
    let mut completed = 0usize;
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

            // 1. Mark as missing in database (Write-Ahead)
            if let Err(err) = db.set_missing(&entry.relative_path, true) {
                errors.push(format!(
                    "Failed to mark {} as missing before move: {err}",
                    entry.relative_path.display()
                ));
                completed += 1;
                continue;
            }

            // 2. Perform filesystem move
            match mover(&source, &entry, &trash_root) {
                Ok(()) => {
                    // 3. Remove from database
                    if let Err(err) = db.remove_file(&entry.relative_path) {
                        errors.push(format!(
                            "Failed to drop database row for {}: {err}",
                            entry.relative_path.display()
                        ));
                        // Even if drop fails, the file is moved and marked missing, so it's safer.
                    } else {
                        moved += 1;
                        affected_sources.insert(source.id.clone());
                    }
                }
                Err(err) => {
                    // 4. Rollback: Unmark as missing if move failed
                    errors.push(err);
                    if let Err(rollback_err) = db.set_missing(&entry.relative_path, false) {
                        errors.push(format!(
                            "Failed to rollback missing status for {}: {rollback_err}",
                            entry.relative_path.display()
                        ));
                    }
                }
            }

            completed += 1;
            on_message(TrashMoveMessage::Progress {
                completed,
                detail: Some(detail),
            });
        }
    }

    let finished = TrashMoveFinished {
        total,
        moved,
        cancelled: cancel.load(Ordering::Relaxed),
        errors,
        affected_sources: affected_sources.into_iter().collect(),
    };
    on_message(TrashMoveMessage::Finished(finished.clone()));
    finished
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

pub(crate) fn move_to_trash(source: &SampleSource, entry: &WavEntry, trash_root: &Path) -> Result<(), String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample_sources::{Rating, SourceId};
    use tempfile::tempdir;

    fn make_test_db(dir: &Path, filename: &str) -> SourceDatabase {
        let db = SourceDatabase::open(dir).unwrap();
        db.upsert_file(Path::new(filename), 123, 456).unwrap();
        db.set_tag(Path::new(filename), Rating::TRASH_3).unwrap();
        db
    }

    #[test]
    fn rollback_on_failure() {
        let dir = tempdir().unwrap();
        let source_root = dir.path().to_path_buf();
        let db = make_test_db(&source_root, "fail.wav");

        let source = SampleSource {
            id: SourceId::new(),
            root: source_root.clone(),
        };

        let trash_root = dir.path().join("trash");
        let cancel = Arc::new(AtomicBool::new(false));

        let finished = run_trash_move_task_with_progress(
            vec![source],
            trash_root,
            cancel,
            |_| {},
            |_source, _entry, _root| Err("Simulated IO Error".to_string()),
        );

        assert!(!finished.errors.is_empty());

        let files = db.list_files().unwrap();
        assert_eq!(files.len(), 1);
        assert!(!files[0].missing, "Should rollback missing status on failure");
    }

    #[test]
    fn success_removes_from_db() {
        let dir = tempdir().unwrap();
        let source_root = dir.path().to_path_buf();
        let db = make_test_db(&source_root, "success.wav");

        let source = SampleSource {
            id: SourceId::new(),
            root: source_root.clone(),
        };

        let trash_root = dir.path().join("trash");
        let cancel = Arc::new(AtomicBool::new(false));

        let finished = run_trash_move_task_with_progress(
            vec![source],
            trash_root,
            cancel,
            |_| {},
            |_source, _entry, _root| Ok(()),
        );

        assert!(finished.errors.is_empty());

        let files = db.list_files().unwrap();
        assert_eq!(files.len(), 0, "Should remove file from DB on success");
    }
}
