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
    controller.ui.drag.set_target(
        DragSource::Collections,
        DragTarget::CollectionsDropZone { collection_id: None },
    );
    assert!(matches!(
        controller.ui.drag.active_target,
        DragTarget::CollectionsDropZone { collection_id: None }
    ));
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
            .all(|entry| &entry.relative_path != member_path)
    );
    assert!(controller.ui.browser.visible.is_empty());
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
fn selection_drop_to_browser_ignores_active_collection() {
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

    let collection = Collection::new("Active");
    let collection_id = collection.id.clone();
    controller.collections.push(collection);
    controller.selected_collection = Some(collection_id.clone());
    controller.refresh_collections_ui();

    controller.ui.drag.payload = Some(DragPayload::Selection {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("clip.wav"),
        bounds: SelectionRange::new(0.0, 0.5),
    });
    controller.ui.drag.set_target(
        DragSource::Browser,
        DragTarget::BrowserTriage(TriageFlagColumn::Keep),
    );
    controller.finish_active_drag();

    let collection = controller
        .collections
        .iter()
        .find(|c| c.id == collection_id)
        .unwrap();
    assert!(collection.members.is_empty());
    assert_eq!(controller.ui.browser.visible.len(), 1);
    assert_eq!(controller.wav_entries.len(), 1);
    assert_eq!(
        controller.wav_entries[0].relative_path,
        PathBuf::from("clip_sel.wav")
    );
}

#[test]
fn selection_drop_without_hover_falls_back_to_active_collection() {
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

    let collection = Collection::new("Active");
    let collection_id = collection.id.clone();
    controller.collections.push(collection);
    controller.selected_collection = Some(collection_id.clone());
    controller.refresh_collections_ui();

    controller.ui.drag.payload = Some(DragPayload::Selection {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("clip.wav"),
        bounds: SelectionRange::new(0.0, 0.5),
    });
    // No hover flags set; should fall back to active collection because there's no triage target.
    controller.finish_active_drag();

    let collection = controller
        .collections
        .iter()
        .find(|c| c.id == collection_id)
        .unwrap();
    assert_eq!(collection.members.len(), 1);
    assert!(root.join(&collection.members[0].relative_path).exists());
    assert!(
        controller
            .wav_entries
            .iter()
            .all(|entry| entry.relative_path != collection.members[0].relative_path)
    );
}

#[test]
fn sample_drop_falls_back_to_active_collection() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("source");
    std::fs::create_dir_all(&root).unwrap();
    let renderer = WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    controller.cache_db(&source).unwrap();
    write_test_wav(&root.join("one.wav"), &[0.1, 0.2]);
    controller.wav_entries = vec![sample_entry("one.wav", SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    let collection = Collection::new("Active");
    let collection_id = collection.id.clone();
    controller.collections.push(collection);
    controller.selected_collection = Some(collection_id.clone());
    controller.refresh_collections_ui();

    controller.ui.drag.payload = Some(DragPayload::Sample {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("one.wav"),
    });
    controller.ui.drag.set_target(
        DragSource::Collections,
        DragTarget::CollectionsRow(collection_id.clone()),
    );
    controller.finish_active_drag();

    let collection = controller
        .collections
        .iter()
        .find(|c| c.id == collection_id)
        .unwrap();
    assert_eq!(collection.members.len(), 1);
    assert_eq!(
        collection.members[0].relative_path,
        PathBuf::from("one.wav")
    );
}

#[test]
fn sample_drop_without_active_collection_warns() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("source");
    std::fs::create_dir_all(&root).unwrap();
    let renderer = WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    controller.cache_db(&source).unwrap();
    write_test_wav(&root.join("one.wav"), &[0.1, 0.2]);
    controller.wav_entries = vec![sample_entry("one.wav", SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.ui.drag.payload = Some(DragPayload::Sample {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("one.wav"),
    });
    controller.ui.drag.set_target(
        DragSource::Collections,
        DragTarget::CollectionsDropZone {
            collection_id: None,
        },
    );
    controller.finish_active_drag();

    assert_eq!(
        controller.ui.status.text,
        "Create or select a collection before dropping samples"
    );
    assert_eq!(controller.ui.status.badge_label, "Warning");
    assert!(controller.collections.is_empty());
}

#[test]
fn sample_drop_without_selection_warns_even_with_collections() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("source");
    std::fs::create_dir_all(&root).unwrap();
    let renderer = WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    controller.cache_db(&source).unwrap();
    write_test_wav(&root.join("one.wav"), &[0.1, 0.2]);
    controller.wav_entries = vec![sample_entry("one.wav", SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    let mut collection = Collection::new("Existing");
    let collection_id = collection.id.clone();
    collection.add_member(source.id.clone(), PathBuf::from("existing.wav"));
    controller.collections.push(collection);
    controller.refresh_collections_ui();

    controller.ui.drag.payload = Some(DragPayload::Sample {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("one.wav"),
    });
    controller.ui.drag.set_target(
        DragSource::Collections,
        DragTarget::CollectionsDropZone {
            collection_id: Some(collection_id.clone()),
        },
    );
    controller.finish_active_drag();

    let stored = controller
        .collections
        .iter()
        .find(|c| c.id == collection_id)
        .unwrap();
    assert!(
        stored
            .members
            .iter()
            .all(|member| member.relative_path != PathBuf::from("one.wav"))
    );
    assert_eq!(
        controller.ui.status.text,
        "Create or select a collection before dropping samples"
    );
    assert_eq!(controller.ui.status.badge_label, "Warning");
}


