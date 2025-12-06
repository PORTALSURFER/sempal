use super::*;
use tempfile::tempdir;

fn dummy_controller() -> (EguiController, SampleSource) {
    let renderer = WaveformRenderer::new(10, 10);
    let mut controller = EguiController::new(renderer, None);
    let dir = tempdir().unwrap();
    let root = dir.path().join("source");
    std::fs::create_dir_all(&root).unwrap();
    let source = SampleSource::new(root);
    controller.selected_source = Some(source.id.clone());
    (controller, source)
}

fn sample_entry(name: &str, tag: SampleTag) -> WavEntry {
    WavEntry {
        relative_path: PathBuf::from(name),
        file_size: 0,
        modified_ns: 0,
        tag,
    }
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

    let selected = controller.ui.triage.selected.unwrap();
    assert_eq!(selected.column, TriageColumn::Neutral);
    let loaded = controller.ui.triage.loaded.unwrap();
    assert_eq!(loaded.column, TriageColumn::Keep);
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

    controller.ui.drag.active_path = Some(PathBuf::from("sample.wav"));
    controller.ui.drag.hovering_collection = Some(collection_id.clone());

    controller.finish_sample_drag();

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
