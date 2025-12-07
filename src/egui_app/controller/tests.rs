use super::test_support::{dummy_controller, sample_entry, write_test_wav};
use super::*;
use crate::egui_app::state::{DragPayload, TriageFilter};
use crate::sample_sources::collections::CollectionMember;
use crate::egui_app::controller::collection_export;
use hound::WavReader;
use std::path::{Path, PathBuf};
use std::io::Cursor;
use tempfile::tempdir;

fn max_sample_amplitude(path: &Path) -> f32 {
    WavReader::open(path)
        .unwrap()
        .samples::<f32>()
        .map(|s| s.unwrap().abs())
        .fold(0.0, f32::max)
}

#[test]
fn missing_source_is_pruned_during_load() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    std::fs::remove_dir_all(&source.root).unwrap();
    controller.queue_wav_load();
    controller.poll_wav_loader();
    assert!(controller.sources.is_empty());
    assert!(controller.selected_source.is_none());
}

#[test]
fn label_cache_builds_on_first_lookup() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.wav_entries = vec![
        sample_entry("a.wav", SampleTag::Neutral),
        sample_entry("b.wav", SampleTag::Neutral),
    ];
    controller.rebuild_wav_lookup();
    controller.rebuild_triage_lists();

    assert!(controller.label_cache.get(&source.id).is_none());
    let label = controller.wav_label(1).unwrap();
    assert_eq!(label, "b.wav");
    assert!(controller.label_cache.get(&source.id).is_some());
}

#[test]
fn triage_indices_track_tags() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source);
    controller.wav_entries = vec![
        sample_entry("trash.wav", SampleTag::Trash),
        sample_entry("neutral.wav", SampleTag::Neutral),
        sample_entry("keep.wav", SampleTag::Keep),
    ];
    controller.selected_wav = Some(PathBuf::from("neutral.wav"));
    controller.loaded_wav = Some(PathBuf::from("keep.wav"));
    controller.rebuild_wav_lookup();
    controller.rebuild_triage_lists();

    assert_eq!(controller.triage_indices(TriageColumn::Trash).len(), 1);
    assert_eq!(controller.triage_indices(TriageColumn::Neutral).len(), 1);
    assert_eq!(controller.triage_indices(TriageColumn::Keep).len(), 1);
    assert_eq!(controller.visible_triage_indices(), &[0, 1, 2]);

    let selected = controller.ui.triage.selected.unwrap();
    assert_eq!(selected.column, TriageColumn::Neutral);
    assert_eq!(controller.ui.triage.selected_visible, Some(1));
    let loaded = controller.ui.triage.loaded.unwrap();
    assert_eq!(loaded.column, TriageColumn::Keep);
    assert_eq!(controller.ui.triage.loaded_visible, Some(2));
}

#[test]
fn dropping_triage_sample_adds_to_collection_and_db() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("source");
    std::fs::create_dir_all(&root).unwrap();
    let renderer = WaveformRenderer::new(10, 10);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.selected_source = Some(source.id.clone());
    controller.sources.push(source.clone());

    let file_path = root.join("sample.wav");
    std::fs::write(&file_path, b"data").unwrap();

    let collection = Collection::new("Test");
    let collection_id = collection.id.clone();
    controller.collections.push(collection);
    controller.selected_collection = Some(collection_id.clone());

    controller.ui.drag.payload = Some(DragPayload::Sample {
        path: PathBuf::from("sample.wav"),
    });
    controller.ui.drag.hovering_collection = Some(collection_id.clone());

    controller.finish_active_drag();

    let collection = controller
        .collections
        .iter()
        .find(|c| c.id == collection_id)
        .unwrap();
    assert_eq!(collection.members.len(), 1);
    assert_eq!(
        collection.members[0].relative_path,
        PathBuf::from("sample.wav")
    );

    let db = controller.database_for(&source).unwrap();
    let rows = db.list_files().unwrap();
    assert!(
        rows.iter()
            .any(|row| row.relative_path == PathBuf::from("sample.wav"))
    );
}

#[test]
fn triage_autoscroll_disabled_when_collection_selected() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source);
    controller.wav_entries = vec![sample_entry("one.wav", SampleTag::Neutral)];
    controller.selected_wav = Some(PathBuf::from("one.wav"));
    controller.rebuild_wav_lookup();
    controller.rebuild_triage_lists();
    controller.ui.collections.selected_sample = Some(0);
    controller.rebuild_triage_lists();
    assert!(!controller.ui.triage.autoscroll);
}

#[test]
fn triage_filter_limits_visible_rows() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source);
    controller.wav_entries = vec![
        sample_entry("trash.wav", SampleTag::Trash),
        sample_entry("neutral.wav", SampleTag::Neutral),
        sample_entry("keep.wav", SampleTag::Keep),
    ];
    controller.rebuild_wav_lookup();
    controller.rebuild_triage_lists();

    controller.set_triage_filter(TriageFilter::Keep);
    assert_eq!(controller.visible_triage_indices(), &[2]);
    controller.set_triage_filter(TriageFilter::Trash);
    assert_eq!(controller.visible_triage_indices(), &[0]);
    controller.set_triage_filter(TriageFilter::Untagged);
    assert_eq!(controller.visible_triage_indices(), &[1]);
    controller.set_triage_filter(TriageFilter::All);
    assert_eq!(controller.visible_triage_indices(), &[0, 1, 2]);
}

#[test]
fn tagging_keeps_selection_on_same_sample() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source);
    controller.wav_entries = vec![
        sample_entry("one.wav", SampleTag::Neutral),
        sample_entry("two.wav", SampleTag::Neutral),
    ];
    controller.selected_wav = Some(PathBuf::from("one.wav"));
    controller.rebuild_wav_lookup();
    controller.rebuild_triage_lists();

    controller.tag_selected(SampleTag::Keep);

    assert_eq!(
        controller.selected_wav.as_deref(),
        Some(Path::new("one.wav"))
    );
    assert_eq!(controller.ui.triage.selected_visible, Some(0));
    assert_eq!(controller.wav_entries[0].tag, SampleTag::Keep);
}

#[test]
fn left_tagging_from_keep_untags_then_trashes() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source);
    controller.wav_entries = vec![
        sample_entry("one.wav", SampleTag::Keep),
        sample_entry("two.wav", SampleTag::Neutral),
    ];
    controller.selected_wav = Some(PathBuf::from("one.wav"));
    controller.rebuild_wav_lookup();
    controller.rebuild_triage_lists();

    controller.tag_selected_left();
    assert_eq!(controller.wav_entries[0].tag, SampleTag::Neutral);

    controller.tag_selected_left();
    assert_eq!(controller.wav_entries[0].tag, SampleTag::Trash);
}

#[test]
fn exporting_selection_updates_entries_and_db() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("source");
    std::fs::create_dir_all(&root).unwrap();
    let renderer = WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());

    let orig = root.join("orig.wav");
    write_test_wav(&orig, &[0.0, 0.25, 0.5, 0.75]);

    controller
        .load_waveform_for_selection(&source, Path::new("orig.wav"))
        .unwrap();

    let entry = controller
        .export_selection_clip(
            &source.id,
            Path::new("orig.wav"),
            SelectionRange::new(0.0, 0.5),
            Some(SampleTag::Keep),
        )
        .unwrap();

    assert_eq!(entry.tag, SampleTag::Keep);
    assert_eq!(entry.relative_path, PathBuf::from("orig_sel.wav"));
    assert_eq!(controller.wav_entries.len(), 1);
    assert_eq!(controller.ui.triage.visible.len(), 1);
    let exported_path = root.join(&entry.relative_path);
    assert!(exported_path.exists());
    let exported: Vec<f32> = hound::WavReader::open(&exported_path)
        .unwrap()
        .samples::<f32>()
        .map(|s| s.unwrap())
        .collect();
    assert_eq!(exported, vec![0.0, 0.25]);

    let db = controller.database_for(&source).unwrap();
    let rows = db.list_files().unwrap();
    let saved = rows
        .iter()
        .find(|row| row.relative_path == entry.relative_path)
        .unwrap();
    assert_eq!(saved.tag, SampleTag::Keep);
}

#[test]
fn waveform_image_resizes_to_view() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    let wav_path = source.root.join("resize.wav");
    write_test_wav(&wav_path, &[0.0, 0.25, -0.5, 0.75]);

    controller
        .load_waveform_for_selection(&source, Path::new("resize.wav"))
        .unwrap();
    controller.update_waveform_size(24, 8);

    let size = controller.ui.waveform.image.as_ref().unwrap().image.size;
    assert_eq!(size, [24, 8]);
}

#[test]
fn selection_drop_adds_clip_to_collection() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("source");
    std::fs::create_dir_all(&root).unwrap();
    let renderer = WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());

    let orig = root.join("clip.wav");
    write_test_wav(&orig, &[0.1, 0.2, 0.3, 0.4]);
    controller
        .load_waveform_for_selection(&source, Path::new("clip.wav"))
        .unwrap();

    let collection = Collection::new("Crops");
    let collection_id = collection.id.clone();
    controller.collections.push(collection);
    controller.selected_collection = Some(collection_id.clone());
    controller.refresh_collections_ui();

    controller.ui.drag.payload = Some(DragPayload::Selection {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("clip.wav"),
        bounds: SelectionRange::new(0.25, 0.75),
    });
    controller.ui.drag.hovering_collection = Some(collection_id.clone());
    controller.ui.drag.hovering_drop_zone = true;
    controller.finish_active_drag();

    let collection = controller
        .collections
        .iter()
        .find(|c| c.id == collection_id)
        .unwrap();
    assert_eq!(collection.members.len(), 1);
    let member_path = &collection.members[0].relative_path;
    assert!(root.join(member_path).exists());
    assert!(
        controller
            .wav_entries
            .iter()
            .any(|entry| &entry.relative_path == member_path)
    );
    assert!(
        controller
            .ui
            .collections
            .samples
            .iter()
            .any(|sample| sample.path == *member_path)
    );
}

#[test]
fn export_path_copies_and_refreshes_members() -> Result<(), String> {
    let temp = tempdir().unwrap();
    let source_root = temp.path().join("source");
    let export_root = temp.path().join("export");
    std::fs::create_dir_all(&source_root).unwrap();
    std::fs::create_dir_all(&export_root).unwrap();
    let renderer = WaveformRenderer::new(10, 10);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(source_root.clone());
    controller.cache_db(&source).unwrap();
    controller.selected_source = Some(source.id.clone());
    controller.sources.push(source.clone());

    let collection = Collection::new("Test");
    let collection_id = collection.id.clone();
    controller.collections.push(collection);
    controller.selected_collection = Some(collection_id.clone());

    let sample_path = source_root.join("one.wav");
    std::fs::write(&sample_path, b"data").unwrap();

    if let Some(collection) = controller
        .collections
        .iter_mut()
        .find(|c| c.id == collection_id)
    {
        collection.export_path = Some(export_root.clone());
    }
    controller.add_sample_to_collection(&collection_id, Path::new("one.wav"))?;
    assert!(export_root.join("Test").join("one.wav").is_file());

    std::fs::remove_file(export_root.join("Test").join("one.wav")).unwrap();
    let extra_path = source_root.join("extra.wav");
    std::fs::write(&extra_path, b"more").unwrap();
    let nested = export_root.join("Test").join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(nested.join("extra.wav"), b"more").unwrap();

    controller.refresh_collection_export(&collection_id);
    let collection = controller
        .collections
        .iter()
        .find(|c| c.id == collection_id)
        .unwrap();
    let labels: Vec<_> = collection
        .members
        .iter()
        .map(|m| m.relative_path.to_string_lossy().to_string())
        .collect();
    assert_eq!(labels, vec!["extra.wav"]);
    Ok(())
}

#[test]
fn renaming_collection_updates_export_folder() -> Result<(), String> {
    let temp = tempdir().unwrap();
    let source_root = temp.path().join("source");
    let export_root = temp.path().join("export");
    std::fs::create_dir_all(&source_root).unwrap();
    std::fs::create_dir_all(&export_root).unwrap();
    let renderer = WaveformRenderer::new(10, 10);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(source_root.clone());
    controller.cache_db(&source).unwrap();
    controller.selected_source = Some(source.id.clone());
    controller.sources.push(source.clone());

    let mut collection = Collection::new("Old");
    collection.export_path = Some(export_root.clone());
    std::fs::create_dir_all(export_root.join("Old")).unwrap();
    collection.add_member(source.id.clone(), PathBuf::from("one.wav"));
    let collection_id = collection.id.clone();
    controller.selected_collection = Some(collection_id.clone());
    controller.collections.push(collection);

    controller.rename_collection(&collection_id, "New Name".into());

    let new_folder = export_root.join("New Name");
    assert!(new_folder.is_dir());
    assert!(!export_root.join("Old").exists());
    assert_eq!(controller.collections[0].name, "New Name");
    Ok(())
}

#[test]
fn triage_rename_updates_collections_and_lookup() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("source");
    std::fs::create_dir_all(&root).unwrap();
    let renderer = WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());

    write_test_wav(&root.join("one.wav"), &[0.1, -0.2]);
    controller.wav_entries = vec![sample_entry("one.wav", SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_triage_lists();

    let mut collection = Collection::new("Crops");
    let collection_id = collection.id.clone();
    collection.add_member(source.id.clone(), PathBuf::from("one.wav"));
    controller.collections.push(collection);
    controller.selected_collection = Some(collection_id.clone());

    controller.rename_triage_sample(0, "renamed.wav").unwrap();

    assert!(!root.join("one.wav").exists());
    assert!(root.join("renamed.wav").is_file());
    assert_eq!(
        controller.wav_entries[0].relative_path,
        PathBuf::from("renamed.wav")
    );
    assert!(controller
        .wav_lookup
        .contains_key(Path::new("renamed.wav")));
    let collection = controller
        .collections
        .iter()
        .find(|c| c.id == collection_id)
        .unwrap();
    assert!(collection
        .members
        .iter()
        .any(|m| m.relative_path == PathBuf::from("renamed.wav")));
}

#[test]
fn triage_normalize_refreshes_exports() -> Result<(), String> {
    let temp = tempdir().unwrap();
    let root = temp.path().join("source");
    let export_root = temp.path().join("export");
    std::fs::create_dir_all(&root).unwrap();
    let renderer = WaveformRenderer::new(16, 16);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.selected_source = Some(source.id.clone());
    controller.sources.push(source.clone());

    write_test_wav(&root.join("one.wav"), &[0.25, -0.5]);
    controller.wav_entries = vec![sample_entry("one.wav", SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_triage_lists();
    controller
        .load_waveform_for_selection(&source, Path::new("one.wav"))
        .unwrap();

    let mut collection = Collection::new("Export");
    let collection_id = collection.id.clone();
    collection.export_path = Some(export_root.clone());
    collection.add_member(source.id.clone(), PathBuf::from("one.wav"));
    controller.collections.push(collection);

    let member = CollectionMember {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("one.wav"),
    };
    controller.export_member_if_needed(&collection_id, &member)?;
    controller.normalize_triage_sample(0)?;

    let collection = controller
        .collections
        .iter()
        .find(|c| c.id == collection_id)
        .unwrap();
    let export_dir = collection_export::export_dir_for(collection)?;
    let exported_path = export_dir.join("one.wav");
    assert!(exported_path.is_file());
    assert!((max_sample_amplitude(&root.join("one.wav")) - 1.0).abs() < 1e-6);
    assert!((max_sample_amplitude(&exported_path) - 1.0).abs() < 1e-6);
    let loaded = controller.loaded_audio.as_ref().expect("loaded audio");
    let max_loaded = WavReader::new(Cursor::new(loaded.bytes.as_slice()))
        .unwrap()
        .samples::<f32>()
        .map(|s| s.unwrap().abs())
        .fold(0.0, f32::max);
    assert!((max_loaded - 1.0).abs() < 1e-6);
    Ok(())
}

#[test]
fn triage_delete_prunes_collections_and_exports() -> Result<(), String> {
    let temp = tempdir().unwrap();
    let root = temp.path().join("source");
    let export_root = temp.path().join("export");
    std::fs::create_dir_all(&root).unwrap();
    let renderer = WaveformRenderer::new(10, 10);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.selected_source = Some(source.id.clone());
    controller.sources.push(source.clone());

    write_test_wav(&root.join("delete.wav"), &[0.1, 0.2]);
    controller.wav_entries = vec![sample_entry("delete.wav", SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_triage_lists();

    let mut collection = Collection::new("Delete");
    let collection_id = collection.id.clone();
    collection.export_path = Some(export_root.clone());
    collection.add_member(source.id.clone(), PathBuf::from("delete.wav"));
    controller.collections.push(collection);

    let member = CollectionMember {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("delete.wav"),
    };
    controller.export_member_if_needed(&collection_id, &member)?;
    controller.delete_triage_sample(0)?;

    assert!(!root.join("delete.wav").exists());
    let db = controller.database_for(&source).expect("open db");
    assert!(db.list_files().unwrap().is_empty());
    let collection = controller
        .collections
        .iter()
        .find(|c| c.id == collection_id)
        .unwrap();
    assert!(collection.members.is_empty());
    let export_dir = collection_export::export_dir_for(collection)?;
    assert!(!export_dir.join("delete.wav").exists());
    assert!(controller.wav_entries.is_empty());
    assert!(controller.ui.triage.visible.is_empty());
    Ok(())
}
