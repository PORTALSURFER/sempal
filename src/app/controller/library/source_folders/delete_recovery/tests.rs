use super::super::DELETE_STAGING_DIR;
use super::{
    DeleteJournalStage, fail_next_save_before_replace_for_tests, load_journal,
    mark_delete_retained, stage_folder_for_delete,
};
use std::fs;
use std::path::Path;
use tempfile::tempdir;

#[test]
fn failed_journal_replace_preserves_last_committed_delete_state() -> Result<(), String> {
    let dir = tempdir().unwrap();
    let source_root = dir.path().join("source");
    let original = source_root.join("gone");
    fs::create_dir_all(&original).unwrap();

    let staging_root = source_root.join(DELETE_STAGING_DIR);
    let staged = stage_folder_for_delete(&original, &staging_root, Path::new("gone"), &[])?;

    let _failure = fail_next_save_before_replace_for_tests(staging_root.clone());
    let err = mark_delete_retained(&staging_root, &staged.id).unwrap_err();

    assert!(err.contains("Injected delete journal save failure"));
    assert!(staging_root.join("delete_journal.json").is_file());

    let journal = load_journal(&staging_root)?;
    let entry = journal
        .entries
        .iter()
        .find(|entry| entry.id == staged.id)
        .ok_or_else(|| "Missing staged delete journal entry".to_string())?;
    assert_eq!(entry.stage, DeleteJournalStage::Staged);
    Ok(())
}

#[test]
fn stage_folder_for_delete_skips_stale_journal_reserved_path() -> Result<(), String> {
    let dir = tempdir().unwrap();
    let source_root = dir.path().join("source");
    let original = source_root.join("gone");
    fs::create_dir_all(&original).unwrap();

    let staging_root = source_root.join(DELETE_STAGING_DIR);
    let first = stage_folder_for_delete(&original, &staging_root, Path::new("gone"), &[])?;
    mark_delete_retained(&staging_root, &first.id)?;
    fs::remove_dir_all(&first.staged_absolute).unwrap();

    fs::create_dir_all(&original).unwrap();
    let second = stage_folder_for_delete(&original, &staging_root, Path::new("gone"), &[])?;

    assert_ne!(second.staged_relative, first.staged_relative);
    assert_eq!(second.staged_relative, Path::new("gone.staged-1"));

    let journal = load_journal(&staging_root)?;
    assert_eq!(journal.entries.len(), 2);
    assert!(
        journal
            .entries
            .iter()
            .any(|entry| entry.staged_relative == "gone")
    );
    assert!(
        journal
            .entries
            .iter()
            .any(|entry| entry.staged_relative == "gone.staged-1")
    );
    Ok(())
}
