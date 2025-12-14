use super::super::test_support::{sample_entry, write_test_wav};
use super::super::*;
use crate::app_dirs::ConfigBaseGuard;
use crate::egui_app::state::{DragPayload, DragSource, DragTarget};
use crate::sample_sources::Collection;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

#[test]
fn selection_drop_adds_clip_to_collection() {
    let temp = tempdir().unwrap();
    let _guard = ConfigBaseGuard::set(temp.path().to_path_buf());
    let root = temp.path().join("source");
    std::fs::create_dir_all(&root).unwrap();
    let renderer = WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.sources.push(source.clone());
    controller.selection_ctx.selected_source = Some(source.id.clone());

    let orig = root.join("clip.wav");
    write_test_wav(&orig, &[0.1, 0.2, 0.3, 0.4]);
    controller
        .load_waveform_for_selection(&source, Path::new("clip.wav"))
        .unwrap();

    let collection = Collection::new("Crops");
    let collection_id = collection.id.clone();
    controller.collections.push(collection);
    controller.selection_ctx.selected_collection = Some(collection_id.clone());
    controller.refresh_collections_ui();

    controller.ui.drag.payload = Some(DragPayload::Selection {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("clip.wav"),
        bounds: SelectionRange::new(0.25, 0.75),
        keep_source_focused: false,
    });
    controller.ui.drag.set_target(
        DragSource::Collections,
        DragTarget::CollectionsDropZone {
            collection_id: None,
        },
    );
    assert!(matches!(
        controller.ui.drag.active_target,
        DragTarget::CollectionsDropZone {
            collection_id: None
        }
    ));
    controller.finish_active_drag();

    let collection = controller
        .collections
        .iter()
        .find(|c| c.id == collection_id)
        .unwrap();
    assert_eq!(collection.members.len(), 1);
    let member = &collection.members[0];
    let member_path = &member.relative_path;
    let clip_root = member.clip_root.as_ref().expect("clip root set");
    assert!(clip_root.join(member_path).exists());
    assert!(!root.join(member_path).exists());
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
fn sample_drop_to_folder_moves_and_updates_state() {
    let temp = tempdir().unwrap();
    let _guard = ConfigBaseGuard::set(temp.path().to_path_buf());
    let root = temp.path().join("source");
    std::fs::create_dir_all(root.join("dest")).unwrap();
    let renderer = WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.sources.push(source.clone());
    controller.selection_ctx.selected_source = Some(source.id.clone());
    controller.cache_db(&source).unwrap();

    write_test_wav(&root.join("one.wav"), &[0.1, 0.2]);
    controller.wav_entries = vec![sample_entry("one.wav", SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    let mut collection = Collection::new("Dest");
    let collection_id = collection.id.clone();
    collection.add_member(source.id.clone(), PathBuf::from("one.wav"));
    controller.collections.push(collection);
    controller.selection_ctx.selected_collection = Some(collection_id.clone());
    controller.refresh_collections_ui();

    controller.ui.drag.payload = Some(DragPayload::Sample {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("one.wav"),
    });
    controller.ui.drag.set_target(
        DragSource::Browser,
        DragTarget::BrowserTriage(TriageFlagColumn::Keep),
    );
    controller.ui.drag.set_target(
        DragSource::Folders,
        DragTarget::FolderPanel {
            folder: Some(PathBuf::from("dest")),
        },
    );
    controller.finish_active_drag();

    assert!(!root.join("one.wav").exists());
    assert!(root.join("dest").join("one.wav").is_file());
    assert!(
        controller
            .wav_entries
            .iter()
            .any(|entry| entry.relative_path == PathBuf::from("dest").join("one.wav"))
    );
    let collection = controller
        .collections
        .iter()
        .find(|c| c.id == collection_id)
        .unwrap();
    assert!(
        collection
            .members
            .iter()
            .any(|m| m.relative_path == PathBuf::from("dest").join("one.wav"))
    );
}

#[test]
fn sample_drop_to_folder_rejects_conflicts() {
    let temp = tempdir().unwrap();
    let _guard = ConfigBaseGuard::set(temp.path().to_path_buf());
    let root = temp.path().join("source");
    let dest = root.join("dest");
    std::fs::create_dir_all(&dest).unwrap();
    write_test_wav(&root.join("one.wav"), &[0.1, 0.2]);
    write_test_wav(&dest.join("one.wav"), &[0.3, 0.4]);
    let renderer = WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.sources.push(source.clone());
    controller.selection_ctx.selected_source = Some(source.id.clone());
    controller.cache_db(&source).unwrap();

    controller.wav_entries = vec![sample_entry("one.wav", SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.ui.drag.payload = Some(DragPayload::Sample {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("one.wav"),
    });
    controller.ui.drag.set_target(
        DragSource::Folders,
        DragTarget::FolderPanel {
            folder: Some(PathBuf::from("dest")),
        },
    );
    controller.finish_active_drag();

    assert!(root.join("one.wav").is_file());
    assert!(dest.join("one.wav").is_file());
    assert!(
        controller
            .wav_entries
            .iter()
            .any(|entry| entry.relative_path == PathBuf::from("one.wav"))
    );
}
