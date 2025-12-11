use super::super::selection_edits::SelectionEditRequest;
use super::super::test_support::{dummy_controller, sample_entry, write_test_wav};
use super::super::*;
use super::common::*;
use crate::egui_app::controller::collection_export;
use crate::egui_app::controller::hotkeys;
use crate::egui_app::state::{
    DestructiveSelectionEdit, DragPayload, DragSource, DragTarget, FocusContext,
    SampleBrowserActionPrompt, TriageFlagColumn, TriageFlagFilter, WaveformView,
};
use crate::sample_sources::Collection;
use crate::sample_sources::collections::CollectionMember;
use crate::waveform::DecodedWaveform;
use egui;
use hound::WavReader;
use rand::SeedableRng;
use rand::rngs::StdRng;
use rand::seq::IteratorRandom;
use std::io::Cursor;
use std::mem;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::tempdir;

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
    controller.collection_export_root = Some(export_root.clone());
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
    controller.collection_export_root = Some(export_root.clone());
    controller.ui.collection_export_root = Some(export_root.clone());
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
fn browser_rename_updates_collections_and_lookup() {
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
    controller.rebuild_browser_lists();

    let mut collection = Collection::new("Crops");
    let collection_id = collection.id.clone();
    collection.add_member(source.id.clone(), PathBuf::from("one.wav"));
    controller.collections.push(collection);
    controller.selected_collection = Some(collection_id.clone());

    controller.rename_browser_sample(0, "renamed").unwrap();

    assert!(!root.join("one.wav").exists());
    assert!(root.join("renamed.wav").is_file());
    assert_eq!(
        controller.wav_entries[0].relative_path,
        PathBuf::from("renamed.wav")
    );
    assert!(controller.wav_lookup.contains_key(Path::new("renamed.wav")));
    let collection = controller
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
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    controller.wav_entries = vec![sample_entry("one.wav", SampleTag::Neutral)];
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
fn browser_rename_preserves_extension_and_stem_with_dots() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("source");
    std::fs::create_dir_all(&root).unwrap();
    let renderer = WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());

    let original = root.join("take.001.WAV");
    write_test_wav(&original, &[0.1, -0.2]);
    controller.wav_entries = vec![sample_entry("take.001.WAV", SampleTag::Neutral)];
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
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    controller.wav_entries = vec![sample_entry("one.wav", SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.focus_browser_list();
    controller.start_browser_rename();

    controller.cancel_browser_rename();

    assert!(controller.ui.browser.pending_action.is_none());
    assert!(!controller.ui.browser.rename_focus_requested);
}

