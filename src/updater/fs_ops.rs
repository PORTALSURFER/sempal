use std::{
    fs,
    path::{Path, PathBuf},
};

use super::{UpdateError, ValidatedInstallRoot};

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

/// A disposable transaction path that remained after the update was committed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostCommitCleanupFailure {
    /// Validated path to the `.old` or `.new` remnant.
    pub path: PathBuf,
    /// Human-readable cleanup error.
    pub error: String,
}

/// Outcome of committing every staged update entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TransactionCommitOutcome {
    /// Every entry was committed and all transaction remnants were removed.
    Committed,
    /// Every entry was committed, but disposable transaction remnants remain.
    CommittedWithCleanupFailures(Vec<PostCommitCleanupFailure>),
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
#[derive(Debug)]
pub(super) struct UpdateTransaction {
    root: ValidatedInstallRoot,
    staged: Vec<StagedEntry>,
    committed: Vec<CommittedEntry>,
}

impl UpdateTransaction {
    /// Create a new update transaction.
    pub(super) fn new(root: ValidatedInstallRoot) -> Self {
        Self {
            root,
            staged: Vec::new(),
            committed: Vec::new(),
        }
    }

    /// Stage a file update by copying into a `.new` sibling.
    pub(super) fn stage_file(&mut self, src: &Path, dest: &str) -> Result<(), UpdateError> {
        self.stage_entry(src, dest, StagedKind::File)
    }

    /// Stage a directory update by copying into a `.new` sibling.
    pub(super) fn stage_dir(&mut self, src: &Path, dest: &str) -> Result<(), UpdateError> {
        self.stage_entry(src, dest, StagedKind::Dir)
    }

    /// Commit all staged updates, rolling back if any swap fails.
    pub(super) fn commit(self) -> Result<TransactionCommitOutcome, UpdateError> {
        self.commit_with_cleanup(remove_path_if_exists)
    }

    fn commit_with_cleanup(
        mut self,
        mut cleanup: impl FnMut(&Path, StagedKind) -> Result<(), UpdateError>,
    ) -> Result<TransactionCommitOutcome, UpdateError> {
        if let Err(err) = self.commit_staged() {
            let rollback_result = self.rollback_committed();
            let cleanup_failures = self.cleanup_staged_with(&mut cleanup);
            if let Err(rollback_err) = rollback_result {
                return Err(UpdateError::Invalid(format!(
                    "Update transaction failed: {err}; rollback failed: {rollback_err}"
                )));
            }
            if !cleanup_failures.is_empty() {
                return Err(UpdateError::Invalid(format!(
                    "Update transaction failed: {err}; cleanup failed: {}",
                    format_cleanup_failures(&cleanup_failures)
                )));
            }
            return Err(err);
        }
        let cleanup_failures = self.cleanup_staged_with(&mut cleanup);
        if cleanup_failures.is_empty() {
            Ok(TransactionCommitOutcome::Committed)
        } else {
            Ok(TransactionCommitOutcome::CommittedWithCleanupFailures(
                cleanup_failures,
            ))
        }
    }

    fn stage_entry(&mut self, src: &Path, dest: &str, kind: StagedKind) -> Result<(), UpdateError> {
        let result = self
            .root
            .child_path(dest)
            .and_then(|dest| stage_entry_copy(src, &dest, kind));
        match result {
            Ok(entry) => {
                self.staged.push(entry);
                Ok(())
            }
            Err(err) => {
                let cleanup_failures = self.cleanup_staged();
                if !cleanup_failures.is_empty() {
                    return Err(UpdateError::Invalid(format!(
                        "Failed to stage update: {err}; cleanup failed: {}",
                        format_cleanup_failures(&cleanup_failures)
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

    fn cleanup_staged(&self) -> Vec<PostCommitCleanupFailure> {
        self.cleanup_staged_with(&mut remove_path_if_exists)
    }

    fn cleanup_staged_with(
        &self,
        cleanup: &mut impl FnMut(&Path, StagedKind) -> Result<(), UpdateError>,
    ) -> Vec<PostCommitCleanupFailure> {
        let mut failures = Vec::new();
        for entry in &self.staged {
            cleanup_entry_with(entry, cleanup, &mut failures);
        }
        failures
    }

    #[cfg(test)]
    pub(super) fn commit_with_cleanup_failure(
        self,
        fail_path: PathBuf,
    ) -> Result<TransactionCommitOutcome, UpdateError> {
        self.commit_with_cleanup(|path, kind| {
            if path == fail_path {
                return Err(std::io::Error::other("injected cleanup failure").into());
            }
            remove_path_if_exists(path, kind)
        })
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
        if had_dest && let Err(restore_err) = fs::rename(&entry.old_path, &entry.dest) {
            return Err(UpdateError::Invalid(format!(
                "Failed to swap {}: {err}; restore failed: {restore_err}",
                entry.dest.display()
            )));
        }
        return Err(err.into());
    }
    Ok(CommittedEntry {
        staged: entry.clone(),
        had_dest,
    })
}

fn rollback_entry(entry: &CommittedEntry) -> Result<(), UpdateError> {
    remove_path_if_exists(&entry.staged.dest, entry.staged.kind)?;
    if entry.had_dest {
        fs::rename(&entry.staged.old_path, &entry.staged.dest)?;
    }
    Ok(())
}

fn cleanup_entry_with(
    entry: &StagedEntry,
    cleanup: &mut impl FnMut(&Path, StagedKind) -> Result<(), UpdateError>,
    failures: &mut Vec<PostCommitCleanupFailure>,
) {
    for path in [&entry.new_path, &entry.old_path] {
        if let Err(err) = cleanup(path, entry.kind) {
            failures.push(PostCommitCleanupFailure {
                path: path.clone(),
                error: err.to_string(),
            });
        }
    }
}

fn remove_path_if_exists(path: &Path, kind: StagedKind) -> Result<(), UpdateError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.into()),
    };
    if metadata.file_type().is_symlink() {
        return remove_symlink(path, &metadata.file_type());
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

fn remove_symlink(path: &Path, file_type: &fs::FileType) -> Result<(), UpdateError> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileTypeExt;

        return remove_symlink_path(path, file_type.is_symlink_dir());
    }
    #[cfg(not(windows))]
    {
        let _ = file_type;
        remove_symlink_path(path, false)
    }
}

fn remove_symlink_path(path: &Path, directory_link: bool) -> Result<(), UpdateError> {
    if directory_link {
        fs::remove_dir(path)?;
    } else {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn format_cleanup_failures(failures: &[PostCommitCleanupFailure]) -> String {
    failures
        .iter()
        .map(|failure| format!("{} ({})", failure.path.display(), failure.error))
        .collect::<Vec<_>>()
        .join(", ")
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

        let install_root = ValidatedInstallRoot::new(&install_dir).unwrap();
        let mut tx = UpdateTransaction::new(install_root);
        tx.stage_file(&src_dir.join("a.txt"), "a.txt").unwrap();
        tx.stage_file(&src_dir.join("b.txt"), "b.txt").unwrap();

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

    #[test]
    fn update_transaction_reports_cleanup_failure_after_committing_new_file() {
        let tmp = tempdir().unwrap();
        let install_dir = tmp.path().join("install");
        let src_dir = tmp.path().join("src");
        fs::create_dir_all(&install_dir).unwrap();
        fs::create_dir_all(&src_dir).unwrap();

        let dest = install_dir.join("wavecrate");
        fs::write(&dest, "old-binary").unwrap();
        fs::write(src_dir.join("wavecrate"), "new-binary").unwrap();

        let install_root = ValidatedInstallRoot::new(&install_dir).unwrap();
        let mut tx = UpdateTransaction::new(install_root);
        tx.stage_file(&src_dir.join("wavecrate"), "wavecrate")
            .unwrap();
        let old_path = install_dir.canonicalize().unwrap().join("wavecrate.old");

        let outcome = tx
            .commit_with_cleanup_failure(old_path.clone())
            .expect("committed update must not become fatal");

        assert_eq!(fs::read_to_string(&dest).unwrap(), "new-binary");
        assert_eq!(
            outcome,
            TransactionCommitOutcome::CommittedWithCleanupFailures(vec![
                PostCommitCleanupFailure {
                    path: old_path.clone(),
                    error: "I/O error: injected cleanup failure".to_string(),
                }
            ])
        );
        assert!(old_path.exists());
    }

    #[test]
    fn update_transaction_removes_prior_remnants_before_restaging() {
        let tmp = tempdir().unwrap();
        let install_dir = tmp.path().join("install");
        let src_dir = tmp.path().join("src");
        fs::create_dir_all(&install_dir).unwrap();
        fs::create_dir_all(&src_dir).unwrap();

        let dest = install_dir.join("wavecrate");
        let old_path = dest.with_file_name("wavecrate.old");
        let new_path = dest.with_file_name("wavecrate.new");
        fs::write(&dest, "current-binary").unwrap();
        fs::write(&old_path, "prior-backup").unwrap();
        fs::write(&new_path, "prior-staging").unwrap();
        fs::write(src_dir.join("wavecrate"), "next-binary").unwrap();

        let install_root = ValidatedInstallRoot::new(&install_dir).unwrap();
        let mut tx = UpdateTransaction::new(install_root);
        tx.stage_file(&src_dir.join("wavecrate"), "wavecrate")
            .expect("validated prior remnants should be retryable");

        assert!(!old_path.exists());
        assert_eq!(fs::read_to_string(new_path).unwrap(), "next-binary");
    }

    #[cfg(unix)]
    #[test]
    fn remnant_cleanup_removes_symlink_without_touching_target() {
        use std::os::unix::fs::symlink;

        let tmp = tempdir().unwrap();
        let outside = tmp.path().join("outside.txt");
        let remnant = tmp.path().join("wavecrate.old");
        fs::write(&outside, "keep").unwrap();
        symlink(&outside, &remnant).unwrap();

        remove_path_if_exists(&remnant, StagedKind::File).unwrap();

        assert!(!remnant.exists());
        assert_eq!(fs::read_to_string(outside).unwrap(), "keep");
    }

    #[cfg(unix)]
    #[test]
    fn remnant_cleanup_removes_directory_symlink_without_touching_target() {
        use std::os::unix::fs::symlink;

        let tmp = tempdir().unwrap();
        let outside = tmp.path().join("outside");
        let remnant = tmp.path().join("resources.old");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("keep.txt"), "keep").unwrap();
        symlink(&outside, &remnant).unwrap();

        remove_path_if_exists(&remnant, StagedKind::Dir).unwrap();

        assert!(!remnant.exists());
        assert_eq!(
            fs::read_to_string(outside.join("keep.txt")).unwrap(),
            "keep"
        );
    }

    #[test]
    fn directory_link_cleanup_uses_directory_unlink_operation() {
        let tmp = tempdir().unwrap();
        let remnant = tmp.path().join("resources.old");
        fs::create_dir(&remnant).unwrap();

        remove_symlink_path(&remnant, true).unwrap();

        assert!(!remnant.exists());
    }

    #[test]
    fn update_transaction_rejects_destinations_outside_install_root() {
        let tmp = tempdir().unwrap();
        let install_dir = tmp.path().join("install");
        let src_dir = tmp.path().join("src");
        fs::create_dir_all(&install_dir).unwrap();
        fs::create_dir_all(&src_dir).unwrap();
        fs::write(src_dir.join("payload.txt"), "payload").unwrap();

        let install_root = ValidatedInstallRoot::new(&install_dir).unwrap();
        let mut tx = UpdateTransaction::new(install_root);
        let err = tx
            .stage_file(&src_dir.join("payload.txt"), "../escape.txt")
            .expect_err("parent traversal must be rejected");

        assert!(err.to_string().contains("Invalid update path"));
        assert!(!tmp.path().join("escape.txt").exists());
        assert!(!install_dir.join("escape.txt.new").exists());
    }
}
