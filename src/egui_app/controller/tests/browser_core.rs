use super::super::test_support::{dummy_controller, sample_entry};
use super::super::*;
use super::common::visible_indices;
use crate::egui_app::state::{
    DragPayload, DragSource, DragTarget, TriageFlagColumn, TriageFlagFilter,
};
use crate::sample_sources::Collection;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

#[test]
fn missing_source_is_marked_during_load() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    std::fs::remove_dir_all(&source.root).unwrap();
    controller.queue_wav_load();
    controller.poll_background_jobs();
    assert_eq!(controller.library.sources.len(), 1);
    assert!(controller.library.missing.sources.contains(&source.id));
    assert!(
        controller
            .ui
            .sources
            .rows
            .first()
            .is_some_and(|row| row.missing)
    );
}

#[test]
fn label_cache_builds_on_first_lookup() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.set_wav_entries_for_tests( vec![
        sample_entry("a.wav", SampleTag::Neutral),
        sample_entry("b.wav", SampleTag::Neutral),
    ]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    assert!(!controller.ui_cache.browser.labels.contains_key(&source.id));
    let label = controller.wav_label(1).unwrap();
    assert_eq!(label, "b");
    assert!(controller.ui_cache.browser.labels.contains_key(&source.id));
}

#[test]
fn label_cache_clears_after_rename() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.cache_db(&source).unwrap();
    controller.set_wav_entries_for_tests(vec![
        sample_entry("a.wav", SampleTag::Neutral),
        sample_entry("b.wav", SampleTag::Neutral),
    ]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    assert_eq!(controller.wav_label(0).unwrap(), "a");
    assert!(controller.ui_cache.browser.labels.contains_key(&source.id));

    controller.update_cached_entry(
        &source,
        Path::new("a.wav"),
        sample_entry("renamed.wav", SampleTag::Neutral),
    );

    assert!(!controller.ui_cache.browser.labels.contains_key(&source.id));
}

#[test]
fn sample_browser_indices_track_tags() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source);
    controller.set_wav_entries_for_tests( vec![
        sample_entry("trash.wav", SampleTag::Trash),
        sample_entry("neutral.wav", SampleTag::Neutral),
        sample_entry("keep.wav", SampleTag::Keep),
    ]);
    controller.sample_view.wav.selected_wav = Some(PathBuf::from("neutral.wav"));
    controller.sample_view.wav.loaded_wav = Some(PathBuf::from("keep.wav"));
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    assert_eq!(controller.browser_indices(TriageFlagColumn::Trash).len(), 1);
    assert_eq!(
        controller.browser_indices(TriageFlagColumn::Neutral).len(),
        1
    );
    assert_eq!(controller.browser_indices(TriageFlagColumn::Keep).len(), 1);
    assert_eq!(visible_indices(&controller), vec![0, 1, 2]);

    let selected = controller.ui.browser.selected.unwrap();
    assert_eq!(selected.column, TriageFlagColumn::Neutral);
    assert_eq!(controller.ui.browser.selected_visible, Some(1));
    let loaded = controller.ui.browser.loaded.unwrap();
    assert_eq!(loaded.column, TriageFlagColumn::Keep);
    assert_eq!(controller.ui.browser.loaded_visible, Some(2));
}

#[test]
fn dropping_sample_moves_to_collection_export() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("source");
    let export_root = temp.path().join("exports");
    std::fs::create_dir_all(&root).unwrap();
    let renderer = WaveformRenderer::new(10, 10);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    controller.library.sources.push(source.clone());
    controller.settings.collection_export_root = Some(export_root.clone());

    let file_path = root.join("sample.wav");
    std::fs::write(&file_path, b"data").unwrap();

    let collection = Collection::new("Test");
    let collection_id = collection.id.clone();
    controller.library.collections.push(collection);
    controller.selection_state.ctx.selected_collection = Some(collection_id.clone());

    controller.ui.drag.payload = Some(DragPayload::Sample {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("sample.wav"),
    });
    controller.ui.drag.set_target(
        DragSource::Collections,
        DragTarget::CollectionsRow(collection_id.clone()),
    );

    controller.finish_active_drag();

    let collection = controller
        .library
        .collections
        .iter()
        .find(|c| c.id == collection_id)
        .unwrap();
    assert_eq!(collection.members.len(), 1);
    let member = &collection.members[0];
    let expected_root = export_root.join(collection.export_folder_name());
    assert_eq!(member.clip_root.as_ref(), Some(&expected_root));
    assert!(expected_root.join(&member.relative_path).exists());
    assert!(!file_path.exists());
}

#[test]
fn deleting_collection_removes_and_selects_next() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source);

    let first = Collection::new("First");
    let second = Collection::new("Second");
    let first_id = first.id.clone();
    let second_id = second.id.clone();
    controller.library.collections.push(first);
    controller.library.collections.push(second);
    controller.selection_state.ctx.selected_collection = Some(first_id.clone());
    controller.refresh_collections_ui();

    controller.delete_collection(&first_id).unwrap();

    assert_eq!(controller.library.collections.len(), 1);
    assert_eq!(controller.library.collections[0].id, second_id.clone());
    assert_eq!(
        controller.selection_state.ctx.selected_collection,
        Some(second_id.clone())
    );
    assert!(controller.ui.collections.selected_sample.is_none());
    assert!(
        controller
            .ui
            .collections
            .rows
            .iter()
            .any(|row| row.id == second_id)
    );
}

#[test]
fn browser_autoscroll_disabled_when_collection_selected() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source);
    controller.set_wav_entries_for_tests( vec![sample_entry("one.wav", SampleTag::Neutral)]);
    controller.sample_view.wav.selected_wav = Some(PathBuf::from("one.wav"));
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.ui.collections.selected_sample = Some(0);
    controller.rebuild_browser_lists();
    assert!(!controller.ui.browser.autoscroll);
}

#[test]
fn browser_filter_limits_visible_rows() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source);
    controller.set_wav_entries_for_tests( vec![
        sample_entry("trash.wav", SampleTag::Trash),
        sample_entry("neutral.wav", SampleTag::Neutral),
        sample_entry("keep.wav", SampleTag::Keep),
    ]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.set_browser_filter(TriageFlagFilter::Keep);
    assert_eq!(visible_indices(&controller), vec![2]);
    controller.set_browser_filter(TriageFlagFilter::Trash);
    assert_eq!(visible_indices(&controller), vec![0]);
    controller.set_browser_filter(TriageFlagFilter::Untagged);
    assert_eq!(visible_indices(&controller), vec![1]);
    controller.set_browser_filter(TriageFlagFilter::All);
    assert_eq!(visible_indices(&controller), vec![0, 1, 2]);
}

#[test]
fn browser_search_limits_visible_rows() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source);
    controller.set_wav_entries_for_tests( vec![
        sample_entry("kick.wav", SampleTag::Neutral),
        sample_entry("snare.wav", SampleTag::Neutral),
        sample_entry("hat.wav", SampleTag::Neutral),
    ]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.set_browser_search("snr");

    assert_eq!(visible_indices(&controller), vec![1]);
}

#[test]
fn browser_search_orders_results_by_score_then_index() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source);
    controller.set_wav_entries_for_tests( vec![
        sample_entry("abc.wav", SampleTag::Neutral),
        sample_entry("abc_extra.wav", SampleTag::Neutral),
        sample_entry("abdc.wav", SampleTag::Neutral),
    ]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.set_browser_search("abc");

    assert_eq!(visible_indices(&controller), vec![0, 1, 2]);
}

#[test]
fn tagging_keeps_selection_on_same_sample() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source);
    controller.set_wav_entries_for_tests( vec![
        sample_entry("one.wav", SampleTag::Neutral),
        sample_entry("two.wav", SampleTag::Neutral),
    ]);
    controller.sample_view.wav.selected_wav = Some(PathBuf::from("one.wav"));
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.tag_selected(SampleTag::Keep);

    assert_eq!(
        controller.sample_view.wav.selected_wav.as_deref(),
        Some(Path::new("one.wav"))
    );
    assert_eq!(controller.ui.browser.selected_visible, Some(0));
    assert_eq!(controller.wav_entry(0).unwrap().tag, SampleTag::Keep);
}

#[test]
fn left_tagging_from_keep_untags_then_trashes() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source);
    controller.set_wav_entries_for_tests( vec![
        sample_entry("one.wav", SampleTag::Keep),
        sample_entry("two.wav", SampleTag::Neutral),
    ]);
    controller.sample_view.wav.selected_wav = Some(PathBuf::from("one.wav"));
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.tag_selected_left();
    assert_eq!(controller.wav_entry(0).unwrap().tag, SampleTag::Neutral);

    controller.tag_selected_left();
    assert_eq!(controller.wav_entry(0).unwrap().tag, SampleTag::Trash);
}

#[test]
fn tagging_under_filter_advances_focus_to_next_visible() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.cache_db(&source).unwrap();
    controller.set_wav_entries_for_tests( vec![
        sample_entry("one.wav", SampleTag::Neutral),
        sample_entry("two.wav", SampleTag::Neutral),
        sample_entry("three.wav", SampleTag::Neutral),
    ]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.set_browser_filter(TriageFlagFilter::Untagged);

    controller.focus_browser_row_only(1);
    controller.tag_selected(SampleTag::Keep);

    assert_eq!(visible_indices(&controller), vec![0, 2]);
    assert_eq!(controller.ui.browser.selected_visible, Some(1));
    assert_eq!(
        controller.sample_view.wav.selected_wav.as_deref(),
        Some(Path::new("three.wav"))
    );
}

#[test]
fn tagging_under_filter_uses_random_focus_in_random_mode() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.cache_db(&source).unwrap();
    controller.set_wav_entries_for_tests( vec![
        sample_entry("one.wav", SampleTag::Neutral),
        sample_entry("two.wav", SampleTag::Neutral),
        sample_entry("three.wav", SampleTag::Neutral),
    ]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.set_browser_filter(TriageFlagFilter::Untagged);
    controller.toggle_random_navigation_mode();

    controller.focus_browser_row_only(1);
    controller.tag_selected(SampleTag::Keep);

    assert_eq!(visible_indices(&controller), vec![0, 2]);
    assert_eq!(controller.history.random_history.entries.len(), 1);
    assert_eq!(controller.history.random_history.cursor, Some(0));
    let Some(selected_visible) = controller.ui.browser.selected_visible else {
        panic!("expected a selected row");
    };
    assert!(selected_visible < controller.visible_browser_len());
}

#[test]
fn undo_tagging_refocuses_original_sample_under_filter() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.cache_db(&source).unwrap();
    controller.set_wav_entries_for_tests( vec![
        sample_entry("one.wav", SampleTag::Neutral),
        sample_entry("two.wav", SampleTag::Neutral),
        sample_entry("three.wav", SampleTag::Neutral),
    ]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.set_browser_filter(TriageFlagFilter::Untagged);

    controller.focus_browser_row_only(1);
    controller.tag_selected(SampleTag::Keep);
    assert_eq!(visible_indices(&controller), vec![0, 2]);
    assert_eq!(
        controller.sample_view.wav.selected_wav.as_deref(),
        Some(Path::new("three.wav"))
    );

    controller.undo();

    assert_eq!(visible_indices(&controller), vec![0, 1, 2]);
    assert_eq!(controller.wav_entry(1).unwrap().tag, SampleTag::Neutral);
    assert_eq!(
        controller.sample_view.wav.selected_wav.as_deref(),
        Some(Path::new("two.wav"))
    );
    assert_eq!(controller.ui.browser.selected_visible, Some(1));
}

#[test]
fn browser_selection_is_cleared_when_focus_leaves_browser() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source);
    controller.set_wav_entries_for_tests( vec![
        sample_entry("one.wav", SampleTag::Neutral),
        sample_entry("two.wav", SampleTag::Neutral),
    ]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.focus_browser_row(0);
    assert_eq!(controller.ui.browser.selected_visible, Some(0));
    assert!(controller.ui.browser.selected.is_some());

    controller.focus_collections_list_context();
    controller.blur_browser_focus();

    assert!(controller.ui.browser.selected_visible.is_none());
    assert!(controller.ui.browser.selected.is_none());
    assert!(controller.ui.browser.selected_paths.is_empty());
}

#[test]
fn browser_selection_is_retained_when_waveform_focused() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source);
    controller.set_wav_entries_for_tests( vec![
        sample_entry("one.wav", SampleTag::Neutral),
        sample_entry("two.wav", SampleTag::Neutral),
    ]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.focus_browser_row(0);
    assert_eq!(
        controller.sample_view.wav.selected_wav.as_deref(),
        Some(Path::new("one.wav"))
    );
    assert_eq!(controller.ui.browser.selected_visible, Some(0));

    controller.focus_waveform_context();
    controller.blur_browser_focus();

    controller.rebuild_browser_lists();
    assert_eq!(
        controller.sample_view.wav.selected_wav.as_deref(),
        Some(Path::new("one.wav"))
    );
    let visible_row = controller.visible_row_for_path(Path::new("one.wav"));
    let selected_visible = controller.ui.browser.selected_visible;
    assert_eq!(selected_visible, visible_row);
}
