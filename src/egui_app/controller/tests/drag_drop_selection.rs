use super::super::test_support::{sample_entry, write_test_wav};
use super::super::*;
use crate::egui_app::controller::collection_export;
use crate::egui_app::state::{DragPayload, DragSource, DragTarget, FocusContext};
use crate::sample_sources::Collection;
use crate::app_dirs::ConfigBaseGuard;
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

    // Selecting the new collection clip should queue audio successfully.
    let expected_path = member_path.clone();
    controller.select_collection_sample(0);
    assert_eq!(controller.ui.waveform.loading.as_ref(), Some(&expected_path));
}

#[test]
fn selection_drop_uses_collection_export_dir_when_configured() {
    let temp = tempdir().unwrap();
    let _guard = ConfigBaseGuard::set(temp.path().to_path_buf());
    let root = temp.path().join("source");
    let export_root = temp.path().join("export");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&export_root).unwrap();
    let renderer = WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    controller.collection_export_root = Some(export_root.clone());
    controller.ui.collection_export_root = Some(export_root.clone());

    let orig = root.join("clip.wav");
    write_test_wav(&orig, &[0.1, 0.2, 0.3, 0.4]);
    controller
        .load_waveform_for_selection(&source, Path::new("clip.wav"))
        .unwrap();

    let collection = Collection::new("Crops");
    let collection_id = collection.id.clone();
    let export_dir = export_root.join(collection_export::collection_folder_name(&collection));
    controller.collections.push(collection);
    controller.selected_collection = Some(collection_id.clone());
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
    controller.finish_active_drag();

    let collection = controller
        .collections
        .iter()
        .find(|c| c.id == collection_id)
        .unwrap();
    let member = &collection.members[0];
    let clip_root = member.clip_root.as_ref().expect("clip root set");
    assert_eq!(clip_root, &export_dir);
    assert!(clip_root.join(&member.relative_path).exists());
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
    controller.ui.focus.context = FocusContext::Waveform;
    controller.wav_entries = vec![sample_entry("clip.wav", SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    let collection = Collection::new("Active");
    let collection_id = collection.id.clone();
    controller.collections.push(collection);
    controller.selected_collection = Some(collection_id.clone());
    controller.refresh_collections_ui();

    controller.ui.drag.payload = Some(DragPayload::Selection {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("clip.wav"),
        bounds: SelectionRange::new(0.0, 0.5),
        keep_source_focused: false,
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
    assert!(
        controller
            .wav_entries
            .iter()
            .any(|entry| entry.relative_path == PathBuf::from("clip_sel.wav"))
    );
    assert_eq!(controller.selected_wav.as_deref(), Some(Path::new("clip_sel.wav")));
    assert_eq!(controller.ui.focus.context, FocusContext::SampleBrowser);
    assert_eq!(
        controller.ui.browser.selected_visible,
        controller.visible_row_for_path(Path::new("clip_sel.wav"))
    );
}

#[test]
fn selection_drop_to_browser_can_keep_source_focused() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("source");
    std::fs::create_dir_all(&root).unwrap();
    let renderer = WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    controller.wav_entries = vec![sample_entry("clip.wav", SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    let orig = root.join("clip.wav");
    write_test_wav(&orig, &[0.1, 0.2, 0.3, 0.4]);
    controller
        .load_waveform_for_selection(&source, Path::new("clip.wav"))
        .unwrap();
    assert_eq!(controller.selected_wav.as_deref(), Some(Path::new("clip.wav")));

    controller.ui.drag.payload = Some(DragPayload::Selection {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("clip.wav"),
        bounds: SelectionRange::new(0.0, 0.5),
        keep_source_focused: true,
    });
    controller.ui.drag.set_target(
        DragSource::Browser,
        DragTarget::BrowserTriage(TriageFlagColumn::Keep),
    );
    controller.finish_active_drag();

    assert_eq!(controller.selected_wav.as_deref(), Some(Path::new("clip.wav")));
    assert!(root.join("clip_sel.wav").is_file());
}

#[test]
fn selection_drop_to_browser_creates_clip_in_focused_folder() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("source");
    let target = root.join("sub");
    std::fs::create_dir_all(&target).unwrap();
    let renderer = WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    controller.cache_db(&source).unwrap();

    let orig = root.join("clip.wav");
    write_test_wav(&orig, &[0.1, 0.2, 0.3, 0.4]);
    controller
        .load_waveform_for_selection(&source, Path::new("clip.wav"))
        .unwrap();
    controller.ui.focus.context = FocusContext::Waveform;
    controller.wav_entries = vec![sample_entry("clip.wav", SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.ui.sources.folders.rows = vec![
        crate::egui_app::state::FolderRowView {
            path: PathBuf::new(),
            name: "Source".into(),
            depth: 0,
            has_children: true,
            expanded: true,
            selected: false,
            is_root: true,
        },
        crate::egui_app::state::FolderRowView {
            path: PathBuf::from("sub"),
            name: "sub".into(),
            depth: 1,
            has_children: false,
            expanded: false,
            selected: false,
            is_root: false,
        },
    ];
    controller.ui.sources.folders.focused = Some(1);

    controller.ui.drag.payload = Some(DragPayload::Selection {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("clip.wav"),
        bounds: SelectionRange::new(0.0, 0.5),
        keep_source_focused: false,
    });
    controller.ui.drag.set_target(
        DragSource::Browser,
        DragTarget::BrowserTriage(TriageFlagColumn::Keep),
    );
    controller.finish_active_drag();

    assert!(root.join("sub").join("clip_sel.wav").is_file());
    assert!(
        controller
            .wav_entries
            .iter()
            .any(|entry| entry.relative_path == PathBuf::from("sub/clip_sel.wav"))
    );
}

#[test]
fn selection_drop_to_browser_respects_shift_pressed_mid_drag() {
    let temp = tempdir().unwrap();
    let _guard = ConfigBaseGuard::set(temp.path().to_path_buf());
    let root = temp.path().join("source");
    std::fs::create_dir_all(&root).unwrap();
    let renderer = WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    controller.cache_db(&source).unwrap();

    let orig = root.join("clip.wav");
    write_test_wav(&orig, &[0.1, 0.2, 0.3, 0.4]);
    controller
        .load_waveform_for_selection(&source, Path::new("clip.wav"))
        .unwrap();
    controller.ui.focus.context = FocusContext::Waveform;
    controller.wav_entries = vec![sample_entry("clip.wav", SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.ui.drag.payload = Some(DragPayload::Selection {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("clip.wav"),
        bounds: SelectionRange::new(0.0, 0.5),
        keep_source_focused: false,
    });

    // Simulate the user pressing shift mid-drag, before dropping onto the browser.
    controller.update_active_drag(
        eframe::egui::pos2(10.0, 10.0),
        DragSource::Browser,
        DragTarget::BrowserTriage(crate::egui_app::state::TriageFlagColumn::Keep),
        true,
    );
    controller.finish_active_drag();

    assert!(
        controller
            .wav_entries
            .iter()
            .any(|entry| entry.relative_path == PathBuf::from("clip_sel.wav"))
    );
    assert_eq!(controller.selected_wav.as_deref(), Some(Path::new("clip.wav")));
    assert_eq!(controller.ui.focus.context, FocusContext::Waveform);
}

#[test]
fn selection_drop_to_folder_panel_creates_clip_in_folder() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("source");
    std::fs::create_dir_all(root.join("sub")).unwrap();
    let renderer = WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    controller.cache_db(&source).unwrap();

    let orig = root.join("clip.wav");
    write_test_wav(&orig, &[0.1, 0.2, 0.3, 0.4]);
    controller
        .load_waveform_for_selection(&source, Path::new("clip.wav"))
        .unwrap();
    controller.wav_entries = vec![sample_entry("clip.wav", SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.ui.drag.payload = Some(DragPayload::Selection {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("clip.wav"),
        bounds: SelectionRange::new(0.0, 0.5),
        keep_source_focused: false,
    });
    controller.ui.drag.set_target(
        DragSource::Folders,
        DragTarget::FolderPanel {
            folder: Some(PathBuf::from("sub")),
        },
    );
    controller.finish_active_drag();

    assert!(root.join("sub").join("clip_sel.wav").is_file());
    assert!(
        controller
            .wav_entries
            .iter()
            .any(|entry| entry.relative_path == PathBuf::from("sub/clip_sel.wav"))
    );
}

#[test]
fn selection_drop_without_hover_falls_back_to_active_collection() {
    let temp = tempdir().unwrap();
    let _guard = ConfigBaseGuard::set(temp.path().to_path_buf());
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
        keep_source_focused: false,
    });
    // No hover flags set; should cancel instead of creating collection clips implicitly.
    controller.finish_active_drag();

    let collection = controller
        .collections
        .iter()
        .find(|c| c.id == collection_id)
        .unwrap();
    assert!(collection.members.is_empty());
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
