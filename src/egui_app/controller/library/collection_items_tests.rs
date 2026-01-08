use super::test_support::{dummy_controller, write_test_wav};
use super::*;
use std::path::Path;
use tempfile::tempdir;

fn setup_collection_with_sample(file_name: &str) -> (EguiController, SampleSource, CollectionId) {
    let (mut controller, source) = dummy_controller();
    controller.cache_db(&source).unwrap();
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    let path = source.root.join(file_name);
    write_test_wav(&path, &[0.2, -0.4, 0.3, -0.1]);

    let collection = Collection::new("Test");
    let collection_id = collection.id.clone();
    controller.selection_state.ctx.selected_collection = Some(collection_id.clone());
    controller.library.collections.push(collection);
    controller
        .add_sample_to_collection(&collection_id, Path::new(file_name))
        .unwrap();
    (controller, source, collection_id)
}

fn enable_export_with_existing_member(
    controller: &mut EguiController,
    collection_id: &CollectionId,
    export_root: &Path,
) {
    std::fs::create_dir_all(export_root).unwrap();
    controller.settings.collection_export_root = Some(export_root.to_path_buf());
    controller.ui.collection_export_root = Some(export_root.to_path_buf());
    if let Some(collection) = controller
        .library
        .collections
        .iter_mut()
        .find(|c| c.id == *collection_id)
    {
        if let Some(member) = collection.members.first().cloned() {
            controller
                .export_member_if_needed(collection_id, &member)
                .unwrap();
        }
    }
}

#[test]
fn collection_tagging_updates_database() {
    let (mut controller, source, _) = setup_collection_with_sample("one.wav");

    controller
        .tag_collection_sample(0, SampleTag::Keep)
        .unwrap();

    let db = controller.database_for(&source).unwrap();
    let rows = db.list_files().unwrap();
    let entry = rows
        .iter()
        .find(|row| row.relative_path == Path::new("one.wav"))
        .unwrap();
    assert_eq!(entry.tag, SampleTag::Keep);
}

#[test]
fn collection_rows_expose_tags_in_ui() {
    let (mut controller, source, _) = setup_collection_with_sample("one.wav");
    controller.ui.collections.selected_sample = Some(0);

    controller.tag_selected_collection_sample(SampleTag::Keep);
    assert_eq!(
        controller
            .ui
            .collections
            .samples
            .first()
            .map(|sample| sample.tag),
        Some(SampleTag::Keep)
    );

    controller.tag_selected_collection_left();
    assert_eq!(
        controller
            .ui
            .collections
            .samples
            .first()
            .map(|sample| sample.tag),
        Some(SampleTag::Neutral)
    );

    let db = controller.database_for(&source).unwrap();
    let rows = db.list_files().unwrap();
    let entry = rows
        .iter()
        .find(|row| row.relative_path == Path::new("one.wav"))
        .unwrap();
    assert_eq!(entry.tag, SampleTag::Neutral);
}

#[test]
fn collection_clip_tagging_works_without_registered_source() {
    let (mut controller, _source, collection_id) = setup_collection_with_sample("one.wav");
    let temp = tempdir().unwrap();
    let clip_root = temp.path().join("clips");
    std::fs::create_dir_all(&clip_root).unwrap();
    write_test_wav(&clip_root.join("clip_sel.wav"), &[0.2, -0.1]);

    controller
        .add_clip_to_collection(&collection_id, clip_root, PathBuf::from("clip_sel.wav"))
        .unwrap();
    controller.refresh_collections_ui();
    controller.select_collection_sample(1);

    controller.tag_selected_collection_sample(SampleTag::Keep);

    assert_eq!(controller.ui.collections.samples[1].tag, SampleTag::Keep);
    assert_ne!(
        controller.ui.status.text,
        "Source not available for this sample"
    );
    assert_ne!(controller.ui.status.badge_label, "Warning");
}

#[test]
fn collection_rename_moves_files_and_export() {
    let (mut controller, source, collection_id) = setup_collection_with_sample("one.wav");
    let export_root = source.root.parent().unwrap().join("export");
    enable_export_with_existing_member(&mut controller, &collection_id, &export_root);

    controller.rename_collection_sample(0, "renamed").unwrap();

    assert!(!source.root.join("one.wav").exists());
    assert!(source.root.join("renamed.wav").is_file());
    let collection = controller
        .library
        .collections
        .iter()
        .find(|c| c.id == collection_id)
        .unwrap();
    assert_eq!(
        collection.members[0].relative_path,
        Path::new("renamed.wav")
    );
    let export_dir = export_root.join("Test");
    assert!(export_dir.join("renamed.wav").is_file());
    assert!(!export_dir.join("one.wav").exists());
    let db = controller.database_for(&source).unwrap();
    let rows = db.list_files().unwrap();
    assert!(
        rows.iter()
            .any(|row| row.relative_path == Path::new("renamed.wav"))
    );
}

#[test]
fn collection_rename_preserves_extension_and_handles_dots() {
    let (mut controller, source, collection_id) = setup_collection_with_sample("loop.v1.WAV");
    controller.selection_state.ctx.selected_collection = Some(collection_id.clone());

    controller.rename_collection_sample(0, "loop.v2").unwrap();
    assert!(source.root.join("loop.v2.WAV").is_file());
    assert!(!source.root.join("loop.v1.WAV").exists());

    controller
        .rename_collection_sample(0, "loop-final.mp3")
        .unwrap();
    assert!(source.root.join("loop-final.WAV").is_file());
    assert!(!source.root.join("loop.v2.WAV").exists());
}

#[test]
fn collection_normalize_overwrites_audio() {
    let (mut controller, source) = dummy_controller();
    controller.cache_db(&source).unwrap();
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    let wav_path = source.root.join("normalize.wav");
    write_test_wav(&wav_path, &[0.1, -0.25, 0.4, -0.2]);
    let collection = Collection::new("Test");
    let collection_id = collection.id.clone();
    controller.selection_state.ctx.selected_collection = Some(collection_id.clone());
    controller.library.collections.push(collection);
    controller
        .add_sample_to_collection(&collection_id, Path::new("normalize.wav"))
        .unwrap();

    controller
        .normalize_collection_sample(0)
        .expect("normalize should succeed");

    let samples: Vec<f32> = hound::WavReader::open(&wav_path)
        .unwrap()
        .samples::<f32>()
        .map(|s| s.unwrap())
        .collect();
    let peak = samples
        .iter()
        .fold(0.0_f32, |acc, sample| acc.max(sample.abs()));
    assert!((peak - 1.0).abs() < 0.001);
}
