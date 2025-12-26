use super::super::test_support::dummy_controller;
use super::super::*;
use crate::sample_sources::Collection;
use crate::sample_sources::SourceDatabase;
use crate::sample_sources::collections::CollectionMember;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

#[test]
fn selecting_missing_sample_sets_waveform_notice() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    controller.set_wav_entries_for_tests( vec![WavEntry {
        relative_path: PathBuf::from("one.wav"),
        file_size: 1,
        modified_ns: 1,
        content_hash: None,
        tag: SampleTag::Neutral,
        missing: true,
    }]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.select_wav_by_path(Path::new("one.wav"));

    assert!(
        controller
            .ui
            .waveform
            .notice
            .as_ref()
            .is_some_and(|msg| msg.contains("one.wav"))
    );
    assert!(controller.sample_view.wav.loaded_audio.is_none());
}

#[test]
fn collection_views_flag_missing_members() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    controller.set_wav_entries_for_tests( vec![WavEntry {
        relative_path: PathBuf::from("one.wav"),
        file_size: 1,
        modified_ns: 1,
        content_hash: None,
        tag: SampleTag::Neutral,
        missing: true,
    }]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.rebuild_missing_lookup_for_source(&source.id);

    let mut collection = Collection::new("Test");
    collection.members.push(CollectionMember {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("one.wav"),
        clip_root: None,
    });
    controller.selection_state.ctx.selected_collection = Some(collection.id.clone());
    controller.library.collections.push(collection);
    controller.refresh_collections_ui();

    assert!(
        controller
            .ui
            .collections
            .rows
            .first()
            .is_some_and(|row| row.missing)
    );
    assert!(
        controller
            .ui
            .collections
            .samples
            .iter()
            .any(|sample| sample.missing)
    );
}

#[test]
fn read_failure_marks_sample_missing() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    let rel = PathBuf::from("gone.wav");
    controller.set_wav_entries_for_tests( vec![WavEntry {
        relative_path: rel.clone(),
        file_size: 1,
        modified_ns: 1,
        content_hash: None,
        tag: SampleTag::Neutral,
        missing: false,
    }]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    let err = controller
        .load_waveform_for_selection(&source, &rel)
        .unwrap_err();
    assert!(err.contains("Failed to read"));
    assert!(controller.sample_missing(&source.id, &rel));
    assert!(controller.wav_entry(0).unwrap().missing);
    assert!(
        controller
            .library
            .missing
            .wavs
            .get(&source.id)
            .is_some_and(|set| set.contains(&rel))
    );
}

#[test]
fn apply_wav_entries_updates_missing_lookup() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    controller
        .ui_cache
        .browser
        .analysis_failures
        .insert(source.id.clone(), HashMap::new());
    let entries = vec![
        WavEntry {
            relative_path: PathBuf::from("alive.wav"),
            file_size: 1,
            modified_ns: 1,
            content_hash: None,
            tag: SampleTag::Neutral,
            missing: false,
        },
        WavEntry {
            relative_path: PathBuf::from("gone.wav"),
            file_size: 1,
            modified_ns: 1,
            content_hash: None,
            tag: SampleTag::Neutral,
            missing: true,
        },
    ];

    controller.apply_wav_entries(
        entries,
        2,
        controller.wav_entries.page_size,
        0,
        true,
        Some(source.id.clone()),
        None,
    );

    assert!(
        controller
            .library
            .missing
            .wavs
            .get(&source.id)
            .is_some_and(|set| set.contains(&PathBuf::from("gone.wav")))
    );
    assert!(
        !controller
            .library
            .missing
            .wavs
            .get(&source.id)
            .is_some_and(|set| set.contains(&PathBuf::from("alive.wav")))
    );
}

#[test]
fn remove_dead_links_rebuilds_missing_state() -> Result<(), String> {
    let temp = tempdir().unwrap();
    let root = temp.path().join("source");
    std::fs::create_dir_all(&root).unwrap();
    let renderer = WaveformRenderer::new(10, 10);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    controller.cache_db(&source).unwrap();
    controller
        .ui_cache
        .browser
        .analysis_failures
        .insert(source.id.clone(), HashMap::new());

    let db = SourceDatabase::open(&root).unwrap();
    db.upsert_file(Path::new("alive.wav"), 1, 1).unwrap();
    db.upsert_file(Path::new("gone.wav"), 1, 1).unwrap();
    db.set_missing(Path::new("gone.wav"), true).unwrap();

    let entries = db.list_files().unwrap();
    controller.apply_wav_entries(
        entries,
        2,
        controller.wav_entries.page_size,
        0,
        true,
        Some(source.id.clone()),
        None,
    );

    let removed = controller.remove_dead_links_for_source_entries(&source)?;
    assert_eq!(removed, 1);

    let remaining = db
        .list_files()
        .unwrap()
        .iter()
        .map(|entry| entry.relative_path.clone())
        .collect::<Vec<_>>();
    assert_eq!(remaining, vec![PathBuf::from("alive.wav")]);
    assert!(
        !controller
            .library
            .missing
            .wavs
            .get(&source.id)
            .is_some_and(|set| set.contains(&PathBuf::from("gone.wav")))
    );
    let entries = controller.wav_entries.pages.get(&0).expect("entries");
    assert!(entries
        .iter()
        .all(|entry| entry.relative_path != PathBuf::from("gone.wav")));
    Ok(())
}

#[test]
fn mark_missing_updates_cache_db_and_missing_set_when_inactive() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("source");
    let other_root = temp.path().join("other");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&other_root).unwrap();
    let renderer = WaveformRenderer::new(10, 10);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    let other = SampleSource::new(other_root);
    controller.library.sources.push(source.clone());
    controller.library.sources.push(other.clone());
    controller.selection_state.ctx.selected_source = Some(other.id.clone());
    controller.cache_db(&source).unwrap();

    let db = SourceDatabase::open(&root).unwrap();
    db.upsert_file(Path::new("one.wav"), 1, 1).unwrap();
    let mut cache = WavEntriesState::new(1, controller.wav_entries.page_size);
    cache.insert_page(
        0,
        vec![WavEntry {
            relative_path: PathBuf::from("one.wav"),
            file_size: 1,
            modified_ns: 1,
            content_hash: None,
            tag: SampleTag::Neutral,
            missing: false,
        }],
    );
    controller.cache.wav.entries.insert(source.id.clone(), cache);

    controller.mark_sample_missing(&source, Path::new("one.wav"));

    let db_entries = db.list_files().unwrap();
    assert!(
        db_entries
            .iter()
            .any(|entry| entry.relative_path == PathBuf::from("one.wav") && entry.missing)
    );
    assert!(
        controller
            .cache
            .wav
            .entries
            .get(&source.id)
            .is_some_and(|entries| entries.entry(0).is_some_and(|entry| entry.missing))
    );
    assert!(
        controller
            .library
            .missing
            .wavs
            .get(&source.id)
            .is_some_and(|set| set.contains(&PathBuf::from("one.wav")))
    );
}

#[test]
fn mark_missing_updates_db_and_missing_set_without_cache() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("source");
    let other_root = temp.path().join("other");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&other_root).unwrap();
    let renderer = WaveformRenderer::new(10, 10);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    let other = SampleSource::new(other_root);
    controller.library.sources.push(source.clone());
    controller.library.sources.push(other.clone());
    controller.selection_state.ctx.selected_source = Some(other.id.clone());
    controller.cache_db(&source).unwrap();

    let db = SourceDatabase::open(&root).unwrap();
    db.upsert_file(Path::new("ghost.wav"), 1, 1).unwrap();

    controller.mark_sample_missing(&source, Path::new("ghost.wav"));

    let db_entries = db.list_files().unwrap();
    assert!(
        db_entries
            .iter()
            .any(|entry| entry.relative_path == PathBuf::from("ghost.wav") && entry.missing)
    );
    assert!(!controller.cache.wav.entries.contains_key(&source.id));
    assert!(
        controller
            .library
            .missing
            .wavs
            .get(&source.id)
            .is_some_and(|set| set.contains(&PathBuf::from("ghost.wav")))
    );
}
