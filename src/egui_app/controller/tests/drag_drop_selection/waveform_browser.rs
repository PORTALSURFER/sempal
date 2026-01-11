use super::super::super::test_support::{sample_entry, write_test_wav};
use super::super::super::*;
use crate::egui_app::state::{DragPayload, DragSource, DragTarget, FocusContext, TriageFlagColumn};
use crate::sample_sources::Collection;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

#[test]
fn selection_drop_to_browser_ignores_active_collection() {
    let temp = tempdir().unwrap();
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
        .load_waveform_for_selection(&source, Path::new("clip.wav"))
        .unwrap();
    controller.ui.focus.context = FocusContext::Waveform;
    controller.set_wav_entries_for_tests(vec![sample_entry("clip.wav", crate::sample_sources::Rating::NEUTRAL)]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

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
    controller.ui.drag.set_target(
        DragSource::Browser,
        DragTarget::BrowserTriage(TriageFlagColumn::Keep),
    );
    controller.finish_active_drag();

    let collection = controller
        .library
        .collections
        .iter()
        .find(|c| c.id == collection_id)
        .unwrap();
    assert!(collection.members.is_empty());
    assert!(
        controller
            .wav_index_for_path(Path::new("clip_sel.wav"))
            .is_some()
    );
    assert_eq!(
        controller.sample_view.wav.selected_wav.as_deref(),
        Some(Path::new("clip_sel.wav"))
    );
    assert_eq!(controller.ui.focus.context, FocusContext::SampleBrowser);
    let visible_row = controller.visible_row_for_path(Path::new("clip_sel.wav"));
    let selected_visible = controller.ui.browser.selected_visible;
    assert_eq!(selected_visible, visible_row);
}

#[test]
fn selection_drop_to_browser_can_keep_source_focused() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("source");
    std::fs::create_dir_all(&root).unwrap();
    let renderer = WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    controller.set_wav_entries_for_tests(vec![sample_entry("clip.wav", crate::sample_sources::Rating::NEUTRAL)]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    let orig = root.join("clip.wav");
    write_test_wav(&orig, &[0.1, 0.2, 0.3, 0.4]);
    controller
        .load_waveform_for_selection(&source, Path::new("clip.wav"))
        .unwrap();
    assert_eq!(
        controller.sample_view.wav.selected_wav.as_deref(),
        Some(Path::new("clip.wav"))
    );

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

    assert_eq!(
        controller.sample_view.wav.selected_wav.as_deref(),
        Some(Path::new("clip.wav"))
    );
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
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    controller.cache_db(&source).unwrap();

    let orig = root.join("clip.wav");
    write_test_wav(&orig, &[0.1, 0.2, 0.3, 0.4]);
    controller
        .load_waveform_for_selection(&source, Path::new("clip.wav"))
        .unwrap();
    controller.ui.focus.context = FocusContext::Waveform;
    controller.set_wav_entries_for_tests(vec![sample_entry("clip.wav", crate::sample_sources::Rating::NEUTRAL)]);
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
            negated: false,
            hotkey: None,
            is_root: true,
        },
        crate::egui_app::state::FolderRowView {
            path: PathBuf::from("sub"),
            name: "sub".into(),
            depth: 1,
            has_children: false,
            expanded: false,
            selected: false,
            negated: false,
            hotkey: None,
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
            .wav_index_for_path(Path::new("sub/clip_sel.wav"))
            .is_some()
    );
}

#[test]
fn selection_drop_to_browser_respects_shift_pressed_mid_drag() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("source");
    std::fs::create_dir_all(&root).unwrap();
    let renderer = WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    controller.cache_db(&source).unwrap();

    let orig = root.join("clip.wav");
    write_test_wav(&orig, &[0.1, 0.2, 0.3, 0.4]);
    controller
        .load_waveform_for_selection(&source, Path::new("clip.wav"))
        .unwrap();
    controller.ui.focus.context = FocusContext::Waveform;
    controller.set_wav_entries_for_tests(vec![sample_entry("clip.wav", crate::sample_sources::Rating::NEUTRAL)]);
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
        false,
    );
    controller.finish_active_drag();

    assert!(
        controller
            .wav_index_for_path(Path::new("clip_sel.wav"))
            .is_some()
    );
    assert_eq!(
        controller.sample_view.wav.selected_wav.as_deref(),
        Some(Path::new("clip.wav"))
    );
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
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    controller.cache_db(&source).unwrap();

    let orig = root.join("clip.wav");
    write_test_wav(&orig, &[0.1, 0.2, 0.3, 0.4]);
    controller
        .load_waveform_for_selection(&source, Path::new("clip.wav"))
        .unwrap();
    controller.set_wav_entries_for_tests(vec![sample_entry("clip.wav", crate::sample_sources::Rating::NEUTRAL)]);
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
            .wav_index_for_path(Path::new("sub/clip_sel.wav"))
            .is_some()
    );
}
