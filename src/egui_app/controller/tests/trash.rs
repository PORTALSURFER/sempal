use super::super::test_support::{dummy_controller, sample_entry, write_test_wav};
use super::super::*;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

#[test]
fn moving_trashed_samples_moves_and_prunes_state() -> Result<(), String> {
    let temp = tempdir().unwrap();
    let trash_root = temp.path().join("trash");
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.settings.trash_folder = Some(trash_root.clone());
    controller.ui.trash_folder = Some(trash_root.clone());

    let trash_file = source.root.join("trash.wav");
    let keep_file = source.root.join("keep.wav");
    write_test_wav(&trash_file, &[0.1, -0.1]);
    write_test_wav(&keep_file, &[0.2, -0.2]);

    let db = controller.database_for(&source).unwrap();
    db.upsert_file(Path::new("trash.wav"), 4, 1).unwrap();
    db.upsert_file(Path::new("keep.wav"), 4, 1).unwrap();
    db.set_tag(Path::new("trash.wav"), SampleTag::Trash)
        .unwrap();
    db.set_tag(Path::new("keep.wav"), SampleTag::Keep).unwrap();

    controller.set_wav_entries_for_tests( vec![
        sample_entry("trash.wav", SampleTag::Trash),
        sample_entry("keep.wav", SampleTag::Keep),
    ]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.move_all_trashed_to_folder();

    assert!(trash_root.join("trash.wav").is_file());
    assert!(!source.root.join("trash.wav").exists());
    let rows = controller
        .database_for(&source)
        .unwrap()
        .list_files()
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].relative_path, PathBuf::from("keep.wav"));
    assert_eq!(rows[0].tag, SampleTag::Keep);
    assert_eq!(controller.wav_entries_len(), 1);
    assert!(
        controller
            .wav_entries
            .entries
            .iter()
            .all(|entry| entry.relative_path != PathBuf::from("trash.wav"))
    );
    assert!(controller.ui.browser.trash.is_empty());
    Ok(())
}

#[test]
fn moving_trashed_samples_can_cancel_midway() -> Result<(), String> {
    let temp = tempdir().unwrap();
    let trash_root = temp.path().join("trash");
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.settings.trash_folder = Some(trash_root.clone());
    controller.ui.trash_folder = Some(trash_root.clone());

    {
        let db = controller.database_for(&source).unwrap();
        for name in ["one.wav", "two.wav"] {
            let path = source.root.join(name);
            write_test_wav(&path, &[0.2, -0.2]);
            db.upsert_file(Path::new(name), 4, 1).unwrap();
            db.set_tag(Path::new(name), SampleTag::Trash).unwrap();
        }
    }

    controller.set_wav_entries_for_tests( vec![
        sample_entry("one.wav", SampleTag::Trash),
        sample_entry("two.wav", SampleTag::Trash),
    ]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.runtime.progress_cancel_after = Some(1);

    controller.move_all_trashed_to_folder();

    assert!(trash_root.join("one.wav").is_file());
    assert!(!source.root.join("one.wav").exists());
    assert!(source.root.join("two.wav").exists());
    assert!(!controller.ui.progress.visible);
    assert!(controller.ui.status.text.contains("Canceled trash move"));
    Ok(())
}

#[test]
fn taking_out_trash_deletes_files() {
    let temp = tempdir().unwrap();
    let trash_root = temp.path().join("trash");
    std::fs::create_dir_all(trash_root.join("nested")).unwrap();
    std::fs::write(trash_root.join("junk.wav"), b"junk").unwrap();
    std::fs::write(trash_root.join("nested").join("more.wav"), b"more").unwrap();

    let (mut controller, _source) = dummy_controller();
    controller.settings.trash_folder = Some(trash_root.clone());
    controller.ui.trash_folder = Some(trash_root.clone());

    controller.take_out_trash();

    assert!(trash_root.is_dir());
    assert!(!trash_root.join("junk.wav").exists());
    assert!(!trash_root.join("nested").join("more.wav").exists());
    let remaining: Vec<_> = std::fs::read_dir(&trash_root).unwrap().collect();
    assert!(remaining.is_empty());
}
