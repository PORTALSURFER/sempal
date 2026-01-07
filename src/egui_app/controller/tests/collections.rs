use super::super::test_support::{dummy_controller, sample_entry, write_test_wav};
use super::super::*;
use crate::app_dirs::ConfigBaseGuard;
use crate::egui_app::state::{FocusContext, SampleBrowserActionPrompt};
use crate::sample_sources::Collection;
use crate::sample_sources::collections::CollectionMember;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

#[test]
fn export_path_copies_and_refreshes_members() -> Result<(), String> {
    let temp = tempdir().unwrap();
    let _guard = ConfigBaseGuard::set(temp.path().to_path_buf());
    let source_root = temp.path().join("source");
    let export_root = temp.path().join("export");
    std::fs::create_dir_all(&source_root).unwrap();
    std::fs::create_dir_all(&export_root).unwrap();
    let renderer = WaveformRenderer::new(10, 10);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(source_root.clone());
    controller.cache_db(&source).unwrap();
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    controller.library.sources.push(source.clone());

    let collection = Collection::new("Test");
    let collection_id = collection.id.clone();
    controller.library.collections.push(collection);
    controller.selection_state.ctx.selected_collection = Some(collection_id.clone());
    controller.settings.collection_export_root = Some(export_root.clone());
    controller.ui.collection_export_root = Some(export_root.clone());

    let sample_path = source_root.join("one.wav");
    std::fs::write(&sample_path, b"data").unwrap();
    controller.add_sample_to_collection(&collection_id, Path::new("one.wav"))?;
    assert!(export_root.join("Test").join("one.wav").is_file());

    std::fs::remove_file(export_root.join("Test").join("one.wav")).unwrap();
    let extra_path = source_root.join("extra.wav");
    std::fs::write(&extra_path, b"more").unwrap();
    let nested = export_root.join("Test").join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(nested.join("extra.wav"), b"more").unwrap();

    controller.sync_collection_export(&collection_id);
    let collection = controller
        .library
        .collections
        .iter()
        .find(|c| c.id == collection_id)
        .unwrap();
    let labels: Vec<_> = collection
        .members
        .iter()
        .map(|m| m.relative_path.to_string_lossy().to_string())
        .collect();
    let expected = PathBuf::from("nested")
        .join("extra.wav")
        .to_string_lossy()
        .to_string();
    assert_eq!(labels, vec![expected]);
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
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    controller.library.sources.push(source.clone());

    let mut collection = Collection::new("Old");
    controller.settings.collection_export_root = Some(export_root.clone());
    controller.ui.collection_export_root = Some(export_root.clone());
    std::fs::create_dir_all(export_root.join("Old")).unwrap();
    collection.add_member(source.id.clone(), PathBuf::from("one.wav"));
    let collection_id = collection.id.clone();
    controller.selection_state.ctx.selected_collection = Some(collection_id.clone());
    controller.library.collections.push(collection);

    controller.rename_collection(&collection_id, "New Name".into());

    let new_folder = export_root.join("New Name");
    assert!(new_folder.is_dir());
    assert!(!export_root.join("Old").exists());
    assert_eq!(controller.library.collections[0].name, "New Name");
    Ok(())
}

#[test]
fn start_collection_rename_sets_pending_action() {
    let renderer = WaveformRenderer::new(10, 10);
    let mut controller = EguiController::new(renderer, None);
    let collection = Collection::new("Rename Me");
    let collection_id = collection.id.clone();
    controller.library.collections.push(collection);
    controller.selection_state.ctx.selected_collection = Some(collection_id.clone());

    controller.start_collection_rename();

    assert!(matches!(
        controller.ui.collections.pending_action,
        Some(crate::egui_app::state::CollectionActionPrompt::Rename { ref target, .. })
            if target == &collection_id
    ));
    assert!(controller.ui.collections.rename_focus_requested);
}

#[test]
fn cancelling_collection_rename_clears_prompt() {
    let renderer = WaveformRenderer::new(10, 10);
    let mut controller = EguiController::new(renderer, None);
    let collection = Collection::new("Rename Me");
    let collection_id = collection.id.clone();
    controller.library.collections.push(collection);
    controller.selection_state.ctx.selected_collection = Some(collection_id);
    controller.start_collection_rename();

    controller.cancel_collection_rename();

    assert!(controller.ui.collections.pending_action.is_none());
    assert!(!controller.ui.collections.rename_focus_requested);
}

#[test]
fn binding_collection_hotkey_clears_previous_binding() {
    let renderer = WaveformRenderer::new(10, 10);
    let mut controller = EguiController::new(renderer, None);
    let first = Collection::new("First");
    let second = Collection::new("Second");
    let first_id = first.id.clone();
    let second_id = second.id.clone();
    controller.library.collections.push(first);
    controller.library.collections.push(second);

    controller.bind_collection_hotkey(&first_id, Some(1));
    controller.bind_collection_hotkey(&second_id, Some(1));

    let first = controller
        .library
        .collections
        .iter()
        .find(|collection| collection.id == first_id)
        .unwrap();
    let second = controller
        .library
        .collections
        .iter()
        .find(|collection| collection.id == second_id)
        .unwrap();
    assert_eq!(first.hotkey, None);
    assert_eq!(second.hotkey, Some(1));
}

#[test]
fn applying_collection_rename_submits_and_clears_prompt() {
    let renderer = WaveformRenderer::new(10, 10);
    let mut controller = EguiController::new(renderer, None);
    let collection = Collection::new("Old Name");
    let collection_id = collection.id.clone();
    controller.library.collections.push(collection);
    controller.selection_state.ctx.selected_collection = Some(collection_id.clone());
    controller.start_collection_rename();
    if let Some(crate::egui_app::state::CollectionActionPrompt::Rename { name, .. }) =
        controller.ui.collections.pending_action.as_mut()
    {
        *name = "New Name".to_string();
    } else {
        panic!("expected collection rename prompt");
    }

    controller.apply_pending_collection_rename();

    assert!(controller.ui.collections.pending_action.is_none());
    assert!(!controller.ui.collections.rename_focus_requested);
    assert_eq!(controller.library.collections[0].name, "New Name");
}

#[test]
fn applying_collection_rename_rejects_empty_name() {
    let renderer = WaveformRenderer::new(10, 10);
    let mut controller = EguiController::new(renderer, None);
    let collection = Collection::new("Keep Me");
    let collection_id = collection.id.clone();
    controller.library.collections.push(collection);
    controller.selection_state.ctx.selected_collection = Some(collection_id);
    controller.start_collection_rename();
    if let Some(crate::egui_app::state::CollectionActionPrompt::Rename { name, .. }) =
        controller.ui.collections.pending_action.as_mut()
    {
        *name = "   ".to_string();
    } else {
        panic!("expected collection rename prompt");
    }

    controller.apply_pending_collection_rename();

    assert!(matches!(
        controller.ui.collections.pending_action,
        Some(crate::egui_app::state::CollectionActionPrompt::Rename { .. })
    ));
    assert!(controller.ui.collections.rename_focus_requested);
    assert_eq!(controller.library.collections[0].name, "Keep Me");
    assert_eq!(controller.ui.status.text, "Collection name cannot be empty");
}

#[test]
fn browser_rename_updates_collections_and_lookup() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("source");
    std::fs::create_dir_all(&root).unwrap();
    let renderer = WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());

    write_test_wav(&root.join("one.wav"), &[0.1, -0.2]);
    controller.set_wav_entries_for_tests(vec![sample_entry("one.wav", SampleTag::Neutral)]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    let mut collection = Collection::new("Crops");
    let collection_id = collection.id.clone();
    collection.add_member(source.id.clone(), PathBuf::from("one.wav"));
    controller.library.collections.push(collection);
    controller.selection_state.ctx.selected_collection = Some(collection_id.clone());

    controller.rename_browser_sample(0, "renamed").unwrap();

    assert!(!root.join("one.wav").exists());
    assert!(root.join("renamed.wav").is_file());
    assert_eq!(
        controller.wav_entry(0).unwrap().relative_path,
        PathBuf::from("renamed.wav")
    );
    assert!(
        controller
            .wav_entries
            .lookup
            .contains_key(Path::new("renamed.wav"))
    );
    let collection = controller
        .library
        .collections
        .iter()
        .find(|c| c.id == collection_id)
        .unwrap();
    assert!(
        collection
            .members
            .iter()
            .any(|m| m.relative_path == PathBuf::from("renamed.wav"))
    );
}

#[test]
fn starting_browser_rename_queues_prompt_for_focused_row() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    controller.set_wav_entries_for_tests(vec![sample_entry("one.wav", SampleTag::Neutral)]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.focus_browser_list();

    controller.start_browser_rename();

    let prompt = controller.ui.browser.pending_action.clone();
    match prompt {
        Some(SampleBrowserActionPrompt::Rename { target, name }) => {
            assert_eq!(target, PathBuf::from("one.wav"));
            assert_eq!(name, "one");
        }
        _ => panic!("expected sample rename prompt"),
    }
    assert!(controller.ui.browser.rename_focus_requested);
    assert_eq!(controller.ui.focus.context, FocusContext::SampleBrowser);
}

#[test]
fn selecting_browser_sample_clears_collection_selection() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.cache_db(&source).unwrap();

    write_test_wav(&source.root.join("a.wav"), &[0.0]);
    write_test_wav(&source.root.join("b.wav"), &[0.0]);
    controller.set_wav_entries_for_tests(vec![
        sample_entry("a.wav", SampleTag::Neutral),
        sample_entry("b.wav", SampleTag::Neutral),
    ]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    let mut collection = Collection::new("Test");
    collection.members.push(CollectionMember {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("a.wav"),
        clip_root: None,
    });
    let collection_id = collection.id.clone();
    controller.library.collections.push(collection);
    controller.selection_state.ctx.selected_collection = Some(collection_id);

    controller.select_collection_sample(0);
    assert_eq!(controller.ui.collections.selected_sample, Some(0));

    controller.select_wav_by_path(Path::new("b.wav"));

    assert!(controller.ui.collections.selected_sample.is_none());
    assert_eq!(
        controller.sample_view.wav.selected_wav.as_deref(),
        Some(Path::new("b.wav"))
    );
}

#[test]
fn selecting_collection_sample_does_not_switch_selected_source() {
    let temp = tempdir().unwrap();
    let root_a = temp.path().join("source_a");
    let root_b = temp.path().join("source_b");
    std::fs::create_dir_all(&root_a).unwrap();
    std::fs::create_dir_all(&root_b).unwrap();
    let renderer = WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source_a = SampleSource::new(root_a);
    let source_b = SampleSource::new(root_b);
    controller.library.sources.push(source_a.clone());
    controller.library.sources.push(source_b.clone());
    controller.selection_state.ctx.selected_source = Some(source_a.id.clone());

    let mut collection = Collection::new("Test");
    collection.members.push(CollectionMember {
        source_id: source_b.id.clone(),
        relative_path: PathBuf::from("b.wav"),
        clip_root: None,
    });
    controller.selection_state.ctx.selected_collection = Some(collection.id.clone());
    controller.library.collections.push(collection);
    controller.refresh_collections_ui();

    controller.select_collection_sample(0);

    assert_eq!(
        controller.selection_state.ctx.selected_source.as_ref(),
        Some(&source_a.id)
    );
}

#[test]
fn sample_tag_for_builds_wav_cache_lookup() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("source");
    std::fs::create_dir_all(&root).unwrap();
    let renderer = WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root);
    controller.library.sources.push(source.clone());

    let mut cache = WavEntriesState::new(2, controller.wav_entries.page_size);
    cache.insert_page(
        0,
        vec![
            sample_entry("a.wav", SampleTag::Keep),
            sample_entry("b.wav", SampleTag::Neutral),
        ],
    );
    controller
        .cache
        .wav
        .entries
        .insert(source.id.clone(), cache);

    let tag = controller
        .sample_tag_for(&source, Path::new("b.wav"))
        .unwrap();
    assert_eq!(tag, SampleTag::Neutral);
    assert!(controller.cache.wav.entries.contains_key(&source.id));
    assert!(
        controller
            .cache
            .wav
            .entries
            .get(&source.id)
            .unwrap()
            .lookup
            .contains_key(Path::new("b.wav"))
    );
}

#[test]
fn pruning_cached_sample_updates_wav_cache_lookup() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("source");
    std::fs::create_dir_all(&root).unwrap();
    let renderer = WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root);
    controller.library.sources.push(source.clone());

    let mut cache = WavEntriesState::new(2, controller.wav_entries.page_size);
    cache.insert_page(
        0,
        vec![
            sample_entry("a.wav", SampleTag::Neutral),
            sample_entry("b.wav", SampleTag::Neutral),
        ],
    );
    controller
        .cache
        .wav
        .entries
        .insert(source.id.clone(), cache);
    assert!(
        controller
            .cache
            .wav
            .entries
            .get(&source.id)
            .unwrap()
            .lookup
            .contains_key(Path::new("a.wav"))
    );

    controller.prune_cached_sample(&source, Path::new("a.wav"));

    let cache = controller.cache.wav.entries.get(&source.id).unwrap();
    assert_eq!(cache.total, 0);
    assert!(cache.lookup.is_empty());
}

#[test]
fn browser_selection_restores_last_browsable_source_after_clip_preview() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.cache_db(&source).unwrap();

    write_test_wav(&source.root.join("a.wav"), &[0.0]);
    controller.set_wav_entries_for_tests(vec![sample_entry("a.wav", SampleTag::Neutral)]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller
        .selection_state
        .ctx
        .last_selected_browsable_source = Some(source.id.clone());
    controller.selection_state.ctx.selected_source = Some(SourceId::from_string("collection-test"));

    controller.select_wav_by_path(Path::new("a.wav"));

    assert_eq!(
        controller.selection_state.ctx.selected_source.as_ref(),
        Some(&source.id)
    );
    assert_eq!(controller.ui.waveform.loading, Some(PathBuf::from("a.wav")));
}

#[test]
fn browser_rename_preserves_extension_and_stem_with_dots() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("source");
    std::fs::create_dir_all(&root).unwrap();
    let renderer = WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());

    let original = root.join("take.001.WAV");
    write_test_wav(&original, &[0.1, -0.2]);
    controller.set_wav_entries_for_tests(vec![sample_entry("take.001.WAV", SampleTag::Neutral)]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.rename_browser_sample(0, "take.002").unwrap();
    assert!(root.join("take.002.WAV").is_file());
    assert!(!root.join("take.001.WAV").exists());

    controller.rename_browser_sample(0, "final.mp3").unwrap();
    assert!(root.join("final.WAV").is_file());
    assert!(!root.join("take.002.WAV").exists());
}

#[test]
fn cancelling_browser_rename_clears_prompt() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    controller.set_wav_entries_for_tests(vec![sample_entry("one.wav", SampleTag::Neutral)]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.focus_browser_list();
    controller.start_browser_rename();

    controller.cancel_browser_rename();

    assert!(controller.ui.browser.pending_action.is_none());
    assert!(!controller.ui.browser.rename_focus_requested);
}
