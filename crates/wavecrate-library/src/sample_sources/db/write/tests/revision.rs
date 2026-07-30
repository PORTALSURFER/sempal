use std::path::Path;

use tempfile::tempdir;

use super::super::super::{ExistingFileMetadataUpdate, Rating, SampleSoundType, SourceDatabase};
use super::helpers::{revision_value, wav_paths_revision_value};

#[test]
fn metadata_only_mutations_do_not_bump_wav_paths_revision() {
    let dir = tempdir().unwrap();
    let db = SourceDatabase::open_for_source_write(dir.path()).unwrap();

    db.upsert_file(Path::new("one.wav"), 10, 5).unwrap();
    assert_eq!(wav_paths_revision_value(&db), 1);

    db.set_tag(Path::new("one.wav"), Rating::KEEP_1).unwrap();
    db.set_locked(Path::new("one.wav"), true).unwrap();
    db.set_missing(Path::new("one.wav"), true).unwrap();
    db.set_last_played_at(Path::new("one.wav"), 42).unwrap();
    db.set_sound_type(Path::new("one.wav"), Some(SampleSoundType::Kick))
        .unwrap();

    assert_eq!(wav_paths_revision_value(&db), 1);

    db.remove_file(Path::new("one.wav")).unwrap();
    assert_eq!(wav_paths_revision_value(&db), 2);
}

#[test]
fn curation_timestamp_tracks_user_metadata_decisions_not_playback() {
    let dir = tempdir().unwrap();
    let db = SourceDatabase::open_for_source_write(dir.path()).unwrap();
    let path = Path::new("one.wav");
    db.upsert_file(path, 10, 5).unwrap();

    db.set_last_played_at(path, 42).unwrap();
    assert_eq!(db.last_curated_at_for_path(path).unwrap(), None);

    db.set_tag(path, Rating::KEEP_1).unwrap();
    assert!(db.last_curated_at_for_path(path).unwrap().is_some());

    db.clear_last_curated_at(path).unwrap();
    assert_eq!(db.last_curated_at_for_path(path).unwrap(), None);

    db.assign_tag_to_path(path, "Kick").unwrap();
    assert!(db.last_curated_at_for_path(path).unwrap().is_some());
}

#[test]
fn single_write_wrappers_match_batch_results_and_revision_behavior() {
    let single_dir = tempdir().unwrap();
    let single = SourceDatabase::open_for_source_write(single_dir.path()).unwrap();
    single.upsert_file(Path::new("one.wav"), 10, 5).unwrap();
    assert_eq!(revision_value(&single), 1);
    single
        .set_tag(Path::new("one.wav"), Rating::KEEP_1)
        .unwrap();
    assert_eq!(revision_value(&single), 2);
    single.set_looped(Path::new("one.wav"), true).unwrap();
    assert_eq!(revision_value(&single), 3);
    single.set_locked(Path::new("one.wav"), true).unwrap();
    assert_eq!(revision_value(&single), 4);
    single.set_missing(Path::new("one.wav"), true).unwrap();
    assert_eq!(revision_value(&single), 5);
    single.set_last_played_at(Path::new("one.wav"), 42).unwrap();
    assert_eq!(revision_value(&single), 6);
    single
        .set_sound_type(Path::new("one.wav"), Some(SampleSoundType::Kick))
        .unwrap();
    assert_eq!(revision_value(&single), 7);

    let batch_dir = tempdir().unwrap();
    let batch_db = SourceDatabase::open_for_source_write(batch_dir.path()).unwrap();
    batch_db.upsert_file(Path::new("one.wav"), 10, 5).unwrap();
    assert_eq!(revision_value(&batch_db), 1);
    let mut batch = batch_db.write_batch().unwrap();
    batch.set_tag(Path::new("one.wav"), Rating::KEEP_1).unwrap();
    batch.set_looped(Path::new("one.wav"), true).unwrap();
    batch.set_locked(Path::new("one.wav"), true).unwrap();
    batch.set_missing(Path::new("one.wav"), true).unwrap();
    batch.set_last_played_at(Path::new("one.wav"), 42).unwrap();
    batch
        .set_sound_type(Path::new("one.wav"), Some(SampleSoundType::Kick))
        .unwrap();
    batch.commit().unwrap();
    assert_eq!(revision_value(&batch_db), 2);

    let single_rows = single.list_files().unwrap();
    let batch_rows = batch_db.list_files().unwrap();
    assert_eq!(single_rows.len(), 1);
    assert_eq!(single_rows[0].relative_path, batch_rows[0].relative_path);
    assert_eq!(single_rows[0].tag, batch_rows[0].tag);
    assert_eq!(single_rows[0].looped, batch_rows[0].looped);
    assert_eq!(single_rows[0].locked, batch_rows[0].locked);
    assert_eq!(single_rows[0].missing, batch_rows[0].missing);
    assert_eq!(single_rows[0].last_played_at, batch_rows[0].last_played_at);
    assert_eq!(
        single.sound_type_for_path(Path::new("one.wav")).unwrap(),
        batch_db.sound_type_for_path(Path::new("one.wav")).unwrap()
    );

    single.remove_file(Path::new("one.wav")).unwrap();
    assert_eq!(revision_value(&single), 8);
    let mut batch = batch_db.write_batch().unwrap();
    batch.remove_file(Path::new("one.wav")).unwrap();
    batch.commit().unwrap();
    assert_eq!(revision_value(&batch_db), 3);

    assert!(single.list_files().unwrap().is_empty());
    assert!(batch_db.list_files().unwrap().is_empty());
}

#[test]
fn empty_tag_batch_is_a_noop() {
    let dir = tempdir().unwrap();
    let db = SourceDatabase::open_for_source_write(dir.path()).unwrap();

    db.set_tags_batch(&[]).unwrap();

    assert_eq!(db.get_revision().unwrap(), 0);
    assert!(db.list_files().unwrap().is_empty());
}

#[test]
fn metadata_wrapper_uses_batch_revision_policy_without_wav_path_churn() {
    let dir = tempdir().unwrap();
    let db = SourceDatabase::open_for_source_write(dir.path()).unwrap();

    db.set_metadata("custom_key", "custom_value").unwrap();

    assert_eq!(
        db.get_metadata("custom_key").unwrap().as_deref(),
        Some("custom_value")
    );
    assert_eq!(revision_value(&db), 1);
    assert_eq!(wav_paths_revision_value(&db), 0);
}

#[test]
fn existing_file_metadata_update_is_revision_neutral_and_defers_missing_rows() {
    let dir = tempdir().unwrap();
    let db = SourceDatabase::open_for_source_write(dir.path()).unwrap();
    let path = Path::new("one.wav");
    db.upsert_file(path, 10, 5).unwrap();
    let revision = revision_value(&db);

    let mut batch = db.write_batch().unwrap();
    assert_eq!(
        batch.update_existing_file_metadata(path, 20, 7).unwrap(),
        ExistingFileMetadataUpdate::Updated
    );
    batch.commit_auxiliary_state().unwrap();

    assert_eq!(revision_value(&db), revision);
    let entry = db.entry_for_path(path).unwrap().unwrap();
    assert_eq!(entry.file_size, 20);
    assert_eq!(entry.modified_ns, 7);

    let missing = Path::new("missing.wav");
    let mut batch = db.write_batch().unwrap();
    assert_eq!(
        batch.update_existing_file_metadata(missing, 30, 9).unwrap(),
        ExistingFileMetadataUpdate::Missing
    );
    batch.commit_auxiliary_state().unwrap();

    assert_eq!(revision_value(&db), revision);
    assert!(db.entry_for_path(missing).unwrap().is_none());
}
