use super::super::test_support::{sample_entry, write_test_wav};
use super::super::*;
use crate::app_dirs::ConfigBaseGuard;
use crate::egui_app::state::{DragPayload, DragSource, DragTarget};
use crate::sample_sources::Collection;
use std::path::PathBuf;
use tempfile::tempdir;

#[test]
fn selection_drop_without_hover_falls_back_to_active_collection() {
    let temp = tempdir().unwrap();
    let _guard = ConfigBaseGuard::set(temp.path().to_path_buf());
    let root = temp.path().join("source");
    std::fs::create_dir_all(&root).unwrap();
    let renderer = WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());

    let orig = root.join("clip.wav");
    write_test_wav(&orig, &[0.1, 0.2, 0.3, 0.4]);
    controller
        .load_waveform_for_selection(&source, std::path::Path::new("clip.wav"))
        .unwrap();

    let collection = Collection::new("Active");
    let collection_id = collection.id.clone();
    controller.library.collections.push(collection);
    controller.selection_state.ctx.selected_collection = Some(collection_id.clone());
    controller.refresh_collections_ui();

    controller.ui.drag.payload = Some(DragPayload::Selection {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("clip.wav"),
        bounds: SelectionRange::new(0.0, 0.5),
        keep_source_focused: false,
    });
    // No hover flags set; should cancel instead of creating collection clips implicitly.
    controller.finish_active_drag();

    let collection = controller
        .library
        .collections
        .iter()
        .find(|c| c.id == collection_id)
        .unwrap();
    assert!(collection.members.is_empty());
}

#[test]
fn sample_drop_without_active_collection_warns() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("source");
    std::fs::create_dir_all(&root).unwrap();
    let renderer = WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    controller.cache_db(&source).unwrap();
    write_test_wav(&root.join("one.wav"), &[0.1, 0.2]);
    controller.wav_entries.entries = vec![sample_entry("one.wav", SampleTag::Neutral)];
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
    assert!(controller.library.collections.is_empty());
}

#[test]
fn sample_drop_without_selection_warns_even_with_collections() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("source");
    std::fs::create_dir_all(&root).unwrap();
    let renderer = WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    controller.cache_db(&source).unwrap();
    write_test_wav(&root.join("one.wav"), &[0.1, 0.2]);
    controller.wav_entries.entries = vec![sample_entry("one.wav", SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    let mut collection = Collection::new("Existing");
    let collection_id = collection.id.clone();
    collection.add_member(source.id.clone(), PathBuf::from("existing.wav"));
    controller.library.collections.push(collection);
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
        .library
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
