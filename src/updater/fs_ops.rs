use std::{
    fs,
    path::{Path, PathBuf},
};

use super::UpdateError;

/// Ensure a directory exists and is empty, deleting any prior contents.
pub(super) fn ensure_empty_dir(path: &Path) -> Result<(), UpdateError> {
    if path.exists() {
        fs::remove_dir_all(path)?;
    }
    fs::create_dir_all(path)?;
    Ok(())
}

/// Return the list of direct child entries inside a directory.
pub(super) fn list_root_entries(path: &Path) -> Result<Vec<PathBuf>, UpdateError> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        entries.push(entry.path());
    }
    Ok(entries)
}

#[derive(Debug, Clone, Copy)]
enum StagedKind {
    File,
    Dir,
}

#[derive(Debug, Clone)]
struct StagedEntry {
    dest: PathBuf,
    new_path: PathBuf,
    old_path: PathBuf,
    kind: StagedKind,
}

#[derive(Debug, Clone)]
struct CommittedEntry {
    staged: StagedEntry,
    had_dest: bool,
}

/// Transactional update helper that stages updates before swapping them in.
#[derive(Debug, Default)]
pub(super) struct UpdateTransaction {
    staged: Vec<StagedEntry>,
    committed: Vec<CommittedEntry>,
}

impl UpdateTransaction {
    /// Create a new update transaction.
    pub(super) fn new() -> Self {
        Self::default()
    }

    /// Stage a file update by copying into a `.new` sibling.
    pub(super) fn stage_file(&mut self, src: &Path, dest: &Path) -> Result<(), UpdateError> {
        self.stage_entry(src, dest, StagedKind::File)
    }

    /// Stage a directory update by copying into a `.new` sibling.
    pub(super) fn stage_dir(&mut self, src: &Path, dest: &Path) -> Result<(), UpdateError> {
        self.stage_entry(src, dest, StagedKind::Dir)
    }

    /// Commit all staged updates, rolling back if any swap fails.
    pub(super) fn commit(mut self) -> Result<(), UpdateError> {
        if let Err(err) = self.commit_staged() {
            let rollback_result = self.rollback_committed();
            let cleanup_result = self.cleanup_staged();
            if let Err(rollback_err) = rollback_result {
                return Err(UpdateError::Invalid(format!(
                    "Update transaction failed: {err}; rollback failed: {rollback_err}"
                )));
            }
            if let Err(cleanup_err) = cleanup_result {
                return Err(UpdateError::Invalid(format!(
                    "Update transaction failed: {err}; cleanup failed: {cleanup_err}"
                )));
            }
            return Err(err);
        }
        if let Err(err) = self.cleanup_staged() {
            return Err(UpdateError::Invalid(format!(
                "Update applied but cleanup failed: {err}"
            )));
        }
        Ok(())
    }

    fn stage_entry(
        &mut self,
        src: &Path,
        dest: &Path,
        kind: StagedKind,
    ) -> Result<(), UpdateError> {
        let result = stage_entry_copy(src, dest, kind);
        match result {
            Ok(entry) => {
                self.staged.push(entry);
                Ok(())
            }
            Err(err) => {
                let cleanup_result = self.cleanup_staged();
                if let Err(cleanup_err) = cleanup_result {
                    return Err(UpdateError::Invalid(format!(
                        "Failed to stage update: {err}; cleanup failed: {cleanup_err}"
                    )));
                }
                Err(err)
            }
        }
    }

    fn commit_staged(&mut self) -> Result<(), UpdateError> {
        for entry in &self.staged {
            let committed = commit_entry(entry)?;
            self.committed.push(committed);
        }
        Ok(())
    }

    fn rollback_committed(&mut self) -> Result<(), UpdateError> {
        for entry in self.committed.iter().rev() {
            rollback_entry(entry)?;
        }
        Ok(())
    }

    fn cleanup_staged(&self) -> Result<(), UpdateError> {
        for entry in &self.staged {
            cleanup_entry(entry)?;
        }
        Ok(())
    }
}

fn stage_entry_copy(src: &Path, dest: &Path, kind: StagedKind) -> Result<StagedEntry, UpdateError> {
    let entry = StagedEntry::new(dest, kind);
    remove_path_if_exists(&entry.old_path, kind)?;
    remove_path_if_exists(&entry.new_path, kind)?;
    match kind {
        StagedKind::File => {
            fs::copy(src, &entry.new_path)?;
        }
        StagedKind::Dir => {
            copy_dir_all(src, &entry.new_path)?;
        }
    }
    Ok(entry)
}

fn commit_entry(entry: &StagedEntry) -> Result<CommittedEntry, UpdateError> {
    let had_dest = entry.dest.exists();
    if had_dest {
        fs::rename(&entry.dest, &entry.old_path)?;
    }
    if let Err(err) = fs::rename(&entry.new_path, &entry.dest) {
        if had_dest {
            if let Err(restore_err) = fs::rename(&entry.old_path, &entry.dest) {
                return Err(UpdateError::Invalid(format!(
                    "Failed to swap {}: {err}; restore failed: {restore_err}",
                    entry.dest.display()
                )));
            }
        }
        return Err(err.into());
    }
    Ok(CommittedEntry { staged: entry.clone(), had_dest })
}

fn rollback_entry(entry: &CommittedEntry) -> Result<(), UpdateError> {
    remove_path_if_exists(&entry.staged.dest, entry.staged.kind)?;
    if entry.had_dest {
        fs::rename(&entry.staged.old_path, &entry.staged.dest)?;
    }
    Ok(())
}

fn cleanup_entry(entry: &StagedEntry) -> Result<(), UpdateError> {
    remove_path_if_exists(&entry.new_path, entry.kind)?;
    remove_path_if_exists(&entry.old_path, entry.kind)?;
    Ok(())
}

fn remove_path_if_exists(path: &Path, kind: StagedKind) -> Result<(), UpdateError> {
    if !path.exists() {
        return Ok(());
    }
    match kind {
        StagedKind::File => {
            fs::remove_file(path)?;
        }
        StagedKind::Dir => {
            fs::remove_dir_all(path)?;
        }
    }
    Ok(())
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("file")
        .to_string();
    name.push('.');
    name.push_str(suffix);
    path.with_file_name(name)
}

fn copy_dir_all(src: &Path, dest: &Path) -> Result<(), UpdateError> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&src_path, &dest_path)?;
        } else if ty.is_file() {
            fs::copy(&src_path, &dest_path)?;
        }
    }
    Ok(())
}

impl StagedEntry {
    fn new(dest: &Path, kind: StagedKind) -> Self {
        Self {
            dest: dest.to_path_buf(),
            new_path: with_suffix(dest, "new"),
            old_path: with_suffix(dest, "old"),
            kind,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn update_transaction_rolls_back_on_commit_failure() {
        let tmp = tempdir().unwrap();
        let install_dir = tmp.path().join("install");
        let src_dir = tmp.path().join("src");
        fs::create_dir_all(&install_dir).unwrap();
        fs::create_dir_all(&src_dir).unwrap();

        let file_a = install_dir.join("a.txt");
        let file_b = install_dir.join("b.txt");
        fs::write(&file_a, "old-a").unwrap();
        fs::write(&file_b, "old-b").unwrap();
        fs::write(src_dir.join("a.txt"), "new-a").unwrap();
        fs::write(src_dir.join("b.txt"), "new-b").unwrap();

        let mut tx = UpdateTransaction::new();
        tx.stage_file(&src_dir.join("a.txt"), &file_a).unwrap();
        tx.stage_file(&src_dir.join("b.txt"), &file_b).unwrap();

        let missing_new = tx.staged[1].new_path.clone();
        fs::remove_file(&missing_new).unwrap();

        assert!(tx.commit().is_err());

        assert_eq!(fs::read_to_string(&file_a).unwrap(), "old-a");
        assert_eq!(fs::read_to_string(&file_b).unwrap(), "old-b");
        assert!(!file_a.with_file_name("a.txt.new").exists());
        assert!(!file_a.with_file_name("a.txt.old").exists());
        assert!(!file_b.with_file_name("b.txt.new").exists());
        assert!(!file_b.with_file_name("b.txt.old").exists());
    }
}
