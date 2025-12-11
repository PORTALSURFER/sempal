#![allow(clippy::cmp_owned, clippy::iter_cloned_collect)]
use super::selection_edits::SelectionEditRequest;
use super::test_support::{dummy_controller, sample_entry, write_test_wav};
use super::*;
use crate::egui_app::controller::collection_export;
use crate::egui_app::controller::hotkeys;
use crate::egui_app::state::{
    DestructiveSelectionEdit, DragPayload, FocusContext, SampleBrowserActionPrompt,
    TriageFlagColumn, TriageFlagFilter, WaveformView,
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

fn max_sample_amplitude(path: &Path) -> f32 {
    WavReader::open(path)
        .unwrap()
        .samples::<f32>()
        .map(|s| s.unwrap().abs())
        .fold(0.0, f32::max)
}

fn prepare_browser_sample(controller: &mut EguiController, source: &SampleSource, name: &str) {
    controller.sources.push(source.clone());
    write_test_wav(&source.root.join(name), &[0.0, 0.1, -0.1]);
    controller.wav_entries = vec![sample_entry(name, SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
}

#[test]
fn missing_source_is_marked_during_load() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    std::fs::remove_dir_all(&source.root).unwrap();
    controller.queue_wav_load();
    controller.poll_wav_loader();
    assert_eq!(controller.sources.len(), 1);
    assert!(controller.missing_sources.contains(&source.id));
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
    controller.sources.push(source.clone());
    controller.wav_entries = vec![
        sample_entry("a.wav", SampleTag::Neutral),
        sample_entry("b.wav", SampleTag::Neutral),
    ];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    assert!(!controller.label_cache.contains_key(&source.id));
    let label = controller.wav_label(1).unwrap();
    assert_eq!(label, "b");
    assert!(controller.label_cache.contains_key(&source.id));
}

#[test]
fn sample_browser_indices_track_tags() {
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
    controller.rebuild_browser_lists();

    assert_eq!(controller.browser_indices(TriageFlagColumn::Trash).len(), 1);
    assert_eq!(
        controller.browser_indices(TriageFlagColumn::Neutral).len(),
        1
    );
    assert_eq!(controller.browser_indices(TriageFlagColumn::Keep).len(), 1);
    assert_eq!(controller.visible_browser_indices(), &[0, 1, 2]);

    let selected = controller.ui.browser.selected.unwrap();
    assert_eq!(selected.column, TriageFlagColumn::Neutral);
    assert_eq!(controller.ui.browser.selected_visible, Some(1));
    let loaded = controller.ui.browser.loaded.unwrap();
    assert_eq!(loaded.column, TriageFlagColumn::Keep);
    assert_eq!(controller.ui.browser.loaded_visible, Some(2));
}

#[test]
fn dropping_sample_adds_to_collection_and_db() {
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

    controller.ui.drag.payload = Some(DragPayload::Sample {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("sample.wav"),
    });
    controller.ui.drag.hovering_collection = Some(collection_id.clone());

    controller.finish_active_drag();

    let collection = controller
        .collections
        .iter()
        .find(|c| c.id == collection_id)
        .unwrap();
    assert_eq!(collection.members.len(), 1);
    assert_eq!(collection.members[0].relative_path, Path::new("sample.wav"));

    let db = controller.database_for(&source).unwrap();
    let rows = db.list_files().unwrap();
    assert!(
        rows.iter()
            .any(|row| row.relative_path == Path::new("sample.wav"))
    );
}

#[test]
fn deleting_collection_removes_and_selects_next() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source);

    let first = Collection::new("First");
    let second = Collection::new("Second");
    let first_id = first.id.clone();
    let second_id = second.id.clone();
    controller.collections.push(first);
    controller.collections.push(second);
    controller.selected_collection = Some(first_id.clone());
    controller.refresh_collections_ui();

    controller.delete_collection(&first_id).unwrap();

    assert_eq!(controller.collections.len(), 1);
    assert_eq!(controller.collections[0].id, second_id.clone());
    assert_eq!(controller.selected_collection, Some(second_id.clone()));
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
    controller.sources.push(source);
    controller.wav_entries = vec![sample_entry("one.wav", SampleTag::Neutral)];
    controller.selected_wav = Some(PathBuf::from("one.wav"));
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.ui.collections.selected_sample = Some(0);
    controller.rebuild_browser_lists();
    assert!(!controller.ui.browser.autoscroll);
}

#[test]
fn browser_filter_limits_visible_rows() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source);
    controller.wav_entries = vec![
        sample_entry("trash.wav", SampleTag::Trash),
        sample_entry("neutral.wav", SampleTag::Neutral),
        sample_entry("keep.wav", SampleTag::Keep),
    ];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.set_browser_filter(TriageFlagFilter::Keep);
    assert_eq!(controller.visible_browser_indices(), &[2]);
    controller.set_browser_filter(TriageFlagFilter::Trash);
    assert_eq!(controller.visible_browser_indices(), &[0]);
    controller.set_browser_filter(TriageFlagFilter::Untagged);
    assert_eq!(controller.visible_browser_indices(), &[1]);
    controller.set_browser_filter(TriageFlagFilter::All);
    assert_eq!(controller.visible_browser_indices(), &[0, 1, 2]);
}

#[test]
fn browser_search_limits_visible_rows() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source);
    controller.wav_entries = vec![
        sample_entry("kick.wav", SampleTag::Neutral),
        sample_entry("snare.wav", SampleTag::Neutral),
        sample_entry("hat.wav", SampleTag::Neutral),
    ];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.set_browser_search("snr");

    assert_eq!(controller.visible_browser_indices(), &[1]);
}

#[test]
fn browser_search_orders_results_by_score_then_index() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source);
    controller.wav_entries = vec![
        sample_entry("abc.wav", SampleTag::Neutral),
        sample_entry("abc_extra.wav", SampleTag::Neutral),
        sample_entry("abdc.wav", SampleTag::Neutral),
    ];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.set_browser_search("abc");

    assert_eq!(controller.visible_browser_indices(), &[0, 1, 2]);
}

#[test]
fn tagging_keeps_selection_on_same_sample() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source);
    controller.wav_entries = vec![
        sample_entry("one.wav", SampleTag::Neutral),
        sample_entry("two.wav", SampleTag::Neutral),
    ];
    controller.selected_wav = Some(PathBuf::from("one.wav"));
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.tag_selected(SampleTag::Keep);

    assert_eq!(
        controller.selected_wav.as_deref(),
        Some(Path::new("one.wav"))
    );
    assert_eq!(controller.ui.browser.selected_visible, Some(0));
    assert_eq!(controller.wav_entries[0].tag, SampleTag::Keep);
}

#[test]
fn left_tagging_from_keep_untags_then_trashes() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source);
    controller.wav_entries = vec![
        sample_entry("one.wav", SampleTag::Keep),
        sample_entry("two.wav", SampleTag::Neutral),
    ];
    controller.selected_wav = Some(PathBuf::from("one.wav"));
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.tag_selected_left();
    assert_eq!(controller.wav_entries[0].tag, SampleTag::Neutral);

    controller.tag_selected_left();
    assert_eq!(controller.wav_entries[0].tag, SampleTag::Trash);
}

#[test]
fn tagging_under_filter_advances_focus_to_next_visible() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.cache_db(&source).unwrap();
    controller.wav_entries = vec![
        sample_entry("one.wav", SampleTag::Neutral),
        sample_entry("two.wav", SampleTag::Neutral),
        sample_entry("three.wav", SampleTag::Neutral),
    ];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.set_browser_filter(TriageFlagFilter::Untagged);

    controller.focus_browser_row_only(1);
    controller.tag_selected(SampleTag::Keep);

    assert_eq!(controller.visible_browser_indices(), &[0, 2]);
    assert_eq!(controller.ui.browser.selected_visible, Some(1));
    assert_eq!(
        controller.selected_wav.as_deref(),
        Some(Path::new("three.wav"))
    );
}

#[test]
fn tagging_under_filter_uses_random_focus_in_random_mode() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.cache_db(&source).unwrap();
    controller.wav_entries = vec![
        sample_entry("one.wav", SampleTag::Neutral),
        sample_entry("two.wav", SampleTag::Neutral),
        sample_entry("three.wav", SampleTag::Neutral),
    ];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.set_browser_filter(TriageFlagFilter::Untagged);
    controller.toggle_random_navigation_mode();

    controller.focus_browser_row_only(1);
    controller.tag_selected(SampleTag::Keep);

    assert_eq!(controller.visible_browser_indices(), &[0, 2]);
    assert_eq!(controller.random_history.len(), 1);
    assert_eq!(controller.random_history_cursor, Some(0));
    let Some(selected_visible) = controller.ui.browser.selected_visible else {
        panic!("expected a selected row");
    };
    assert!(selected_visible < controller.visible_browser_indices().len());
}

#[test]
fn hotkey_tagging_applies_to_all_selected_rows() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.cache_db(&source).unwrap();
    controller.wav_entries = vec![
        sample_entry("one.wav", SampleTag::Neutral),
        sample_entry("two.wav", SampleTag::Neutral),
    ];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.focus_browser_row_only(0);
    controller.toggle_browser_row_selection(1);
    controller.tag_selected_left();

    assert_eq!(controller.wav_entries[0].tag, SampleTag::Trash);
    assert_eq!(controller.wav_entries[1].tag, SampleTag::Trash);
}

#[test]
fn escape_clears_selection() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.cache_db(&source).unwrap();
    controller.wav_entries = vec![
        sample_entry("one.wav", SampleTag::Neutral),
        sample_entry("two.wav", SampleTag::Neutral),
    ];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.focus_browser_row_only(0);
    controller.toggle_browser_row_selection(1);
    assert_eq!(controller.ui.browser.selected_paths.len(), 2);

    controller.clear_browser_selection();

    assert!(controller.ui.browser.selected_paths.is_empty());
    assert!(controller.ui.browser.selection_anchor_visible.is_none());
}

#[test]
fn escape_handler_clears_waveform_and_browser_state() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.cache_db(&source).unwrap();
    controller.wav_entries = vec![sample_entry("one.wav", SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller
        .selection
        .set_range(Some(SelectionRange::new(0.2, 0.8)));
    controller.apply_selection(controller.selection.range());
    controller
        .ui
        .browser
        .selected_paths
        .push(PathBuf::from("one.wav"));
    controller.ui.browser.selection_anchor_visible = Some(0);

    controller.handle_escape();

    assert!(controller.selection.range().is_none());
    assert!(controller.ui.waveform.selection.is_none());
    assert!(controller.ui.browser.selected_paths.is_empty());
    assert!(controller.ui.browser.selection_anchor_visible.is_none());
}

#[test]
fn escape_clears_waveform_cursor_and_resets_start_marker() {
    let (mut controller, _source) = dummy_controller();
    controller.ui.waveform.cursor = Some(0.55);
    controller.ui.waveform.last_start_marker = Some(0.55);
    controller.ui.waveform.cursor_last_hover_at = Some(Instant::now());
    controller.ui.waveform.cursor_last_navigation_at = Some(Instant::now());

    controller.handle_escape();

    assert!(controller.ui.waveform.cursor.is_none());
    assert_eq!(controller.ui.waveform.last_start_marker, Some(0.0));
    assert!(controller.ui.waveform.cursor_last_hover_at.is_none());
    assert!(controller.ui.waveform.cursor_last_navigation_at.is_none());
}

#[test]
fn escape_stops_playback_before_clearing_selection() {
    let Some(player) = crate::audio::AudioPlayer::playing_for_tests() else {
        return;
    };
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.cache_db(&source).unwrap();
    controller.wav_entries = vec![sample_entry("one.wav", SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller
        .selection
        .set_range(Some(SelectionRange::new(0.25, 0.75)));
    controller.apply_selection(controller.selection.range());
    controller.player = Some(std::rc::Rc::new(std::cell::RefCell::new(player)));

    controller.handle_escape();

    assert!(controller.selection.range().is_some());
    assert!(controller.ui.waveform.selection.is_some());
    assert!(!controller.is_playing());
}

#[test]
fn click_clears_selection_and_focuses_row() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.cache_db(&source).unwrap();
    controller.wav_entries = vec![
        sample_entry("one.wav", SampleTag::Neutral),
        sample_entry("two.wav", SampleTag::Neutral),
        sample_entry("three.wav", SampleTag::Neutral),
    ];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.focus_browser_row(0);
    controller.toggle_browser_row_selection(1);
    assert_eq!(controller.ui.browser.selected_paths.len(), 2);

    controller.clear_browser_selection();
    controller.focus_browser_row_only(2);

    assert!(controller.ui.browser.selected_paths.is_empty());
    assert_eq!(controller.ui.browser.selected_visible, Some(2));
    assert_eq!(controller.ui.browser.selection_anchor_visible, Some(2));
}

#[test]
fn ctrl_click_toggles_selection_and_focuses_row() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source);
    controller.wav_entries = vec![
        sample_entry("one.wav", SampleTag::Neutral),
        sample_entry("two.wav", SampleTag::Neutral),
        sample_entry("three.wav", SampleTag::Neutral),
    ];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.focus_browser_row_only(0);
    assert!(controller.ui.browser.selected_paths.is_empty());
    assert_eq!(controller.ui.browser.selection_anchor_visible, Some(0));

    controller.toggle_browser_row_selection(2);

    let selected: Vec<_> = controller.ui.browser.selected_paths.to_vec();
    assert!(selected.contains(&PathBuf::from("one.wav")));
    assert!(selected.contains(&PathBuf::from("three.wav")));
    assert_eq!(controller.ui.browser.selected_visible, Some(2));
}

#[test]
fn shift_click_extends_selection_range() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source);
    controller.wav_entries = vec![
        sample_entry("one.wav", SampleTag::Neutral),
        sample_entry("two.wav", SampleTag::Neutral),
        sample_entry("three.wav", SampleTag::Neutral),
    ];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.focus_browser_row(0);
    controller.toggle_browser_row_selection(2);

    controller.extend_browser_selection_to_row(1);

    let selected: Vec<_> = controller.ui.browser.selected_paths.to_vec();
    assert_eq!(selected.len(), 2);
    assert!(selected.contains(&PathBuf::from("one.wav")));
    assert!(selected.contains(&PathBuf::from("two.wav")));
    assert!(!selected.contains(&PathBuf::from("three.wav")));
    assert_eq!(controller.ui.browser.selected_visible, Some(1));
    assert_eq!(controller.ui.browser.selection_anchor_visible, Some(0));
}

#[test]
fn ctrl_shift_click_adds_range_without_resetting_anchor() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source);
    controller.wav_entries = vec![
        sample_entry("one.wav", SampleTag::Neutral),
        sample_entry("two.wav", SampleTag::Neutral),
        sample_entry("three.wav", SampleTag::Neutral),
        sample_entry("four.wav", SampleTag::Neutral),
        sample_entry("five.wav", SampleTag::Neutral),
        sample_entry("six.wav", SampleTag::Neutral),
    ];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.focus_browser_row(0);
    controller.toggle_browser_row_selection(5);

    controller.add_range_browser_selection(2);

    let selected: Vec<_> = controller
        .ui
        .browser
        .selected_paths
        .iter()
        .cloned()
        .collect();
    assert_eq!(selected.len(), 4);
    assert!(selected.contains(&PathBuf::from("one.wav")));
    assert!(selected.contains(&PathBuf::from("two.wav")));
    assert!(selected.contains(&PathBuf::from("three.wav")));
    assert!(selected.contains(&PathBuf::from("six.wav")));
    assert_eq!(controller.ui.browser.selection_anchor_visible, Some(0));
    assert_eq!(controller.ui.browser.selected_visible, Some(2));
}

#[test]
fn shift_arrow_grows_selection() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source);
    controller.wav_entries = vec![
        sample_entry("one.wav", SampleTag::Neutral),
        sample_entry("two.wav", SampleTag::Neutral),
        sample_entry("three.wav", SampleTag::Neutral),
    ];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.focus_browser_row(1);
    controller.grow_selection(1);

    let selected: Vec<_> = controller
        .ui
        .browser
        .selected_paths
        .iter()
        .cloned()
        .collect();
    assert!(selected.contains(&PathBuf::from("two.wav")));
    assert!(selected.contains(&PathBuf::from("three.wav")));
    assert_eq!(controller.ui.browser.selection_anchor_visible, Some(1));
    assert_eq!(controller.ui.browser.selected_visible, Some(2));
}

#[test]
fn focus_hotkey_does_not_autoplay_browser_sample() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    write_test_wav(&source.root.join("one.wav"), &[0.0, 0.1]);
    controller.wav_entries = vec![sample_entry("one.wav", SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    assert!(controller.feature_flags.autoplay_selection);

    controller.focus_browser_list();

    assert_eq!(controller.ui.focus.context, FocusContext::SampleBrowser);
    assert_eq!(
        controller.selected_wav.as_deref(),
        Some(Path::new("one.wav"))
    );
    assert!(controller.pending_playback.is_none());
    assert_eq!(controller.ui.browser.selected_visible, Some(0));
}

#[test]
fn x_key_toggle_respects_focus() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source);
    controller.wav_entries = vec![
        sample_entry("one.wav", SampleTag::Neutral),
        sample_entry("two.wav", SampleTag::Neutral),
    ];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.focus_browser_row(0);
    controller.toggle_focused_selection();
    assert!(controller.ui.browser.selected_paths.is_empty());
    assert_eq!(controller.ui.browser.selected_visible, Some(0));

    controller.toggle_focused_selection();
    assert!(
        controller
            .ui
            .browser
            .selected_paths
            .iter()
            .any(|p| p == &PathBuf::from("one.wav"))
    );
    assert_eq!(controller.ui.browser.selection_anchor_visible, Some(0));
}

#[test]
fn action_rows_include_selection_and_primary() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source);
    controller.wav_entries = vec![
        sample_entry("one.wav", SampleTag::Neutral),
        sample_entry("two.wav", SampleTag::Neutral),
        sample_entry("three.wav", SampleTag::Neutral),
    ];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.ui.browser.selected_paths =
        vec![PathBuf::from("one.wav"), PathBuf::from("three.wav")];

    let rows = controller.action_rows_from_primary(1);

    assert_eq!(rows, vec![0, 1, 2]);
}

#[test]
fn tag_actions_apply_to_all_selected_rows() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.cache_db(&source).unwrap();
    controller.wav_entries = vec![
        sample_entry("one.wav", SampleTag::Neutral),
        sample_entry("two.wav", SampleTag::Neutral),
    ];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.focus_browser_row(0);
    controller.toggle_browser_row_selection(1);
    let rows = controller.action_rows_from_primary(0);

    controller
        .tag_browser_samples(&rows, SampleTag::Keep, 0)
        .unwrap();

    assert_eq!(controller.wav_entries[0].tag, SampleTag::Keep);
    assert_eq!(controller.wav_entries[1].tag, SampleTag::Keep);
}

#[test]
fn selection_persists_when_nudging_focus() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.cache_db(&source).unwrap();
    controller.wav_entries = vec![
        sample_entry("one.wav", SampleTag::Neutral),
        sample_entry("two.wav", SampleTag::Neutral),
        sample_entry("three.wav", SampleTag::Neutral),
    ];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.focus_browser_row(0);
    controller.toggle_browser_row_selection(1);
    controller.nudge_selection(1);

    let selected = &controller.ui.browser.selected_paths;
    assert!(selected.contains(&PathBuf::from("one.wav")));
    assert!(selected.contains(&PathBuf::from("two.wav")));
    // Focus moved, but selection stayed intact.
    assert_eq!(controller.ui.browser.selected_visible, Some(2));
}

#[test]
fn focused_row_actions_work_without_explicit_selection() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.cache_db(&source).unwrap();
    controller.wav_entries = vec![
        sample_entry("one.wav", SampleTag::Neutral),
        sample_entry("two.wav", SampleTag::Neutral),
    ];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.nudge_selection(0);
    assert!(controller.ui.browser.selected_paths.is_empty());

    controller.tag_selected_left();

    assert_eq!(controller.wav_entries[0].tag, SampleTag::Trash);
    assert_eq!(controller.ui.browser.selected_visible, Some(0));
}

#[test]
fn exporting_selection_updates_entries_and_db() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("source");
    std::fs::create_dir_all(&root).unwrap();
    let renderer = WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());

    let orig = root.join("orig.wav");
    write_test_wav(&orig, &[0.0, 0.25, 0.5, 0.75]);

    controller
        .load_waveform_for_selection(&source, Path::new("orig.wav"))
        .unwrap();

    let entry = controller
        .export_selection_clip(
            &source.id,
            Path::new("orig.wav"),
            SelectionRange::new(0.0, 0.5),
            Some(SampleTag::Keep),
            true,
        )
        .unwrap();

    assert_eq!(entry.tag, SampleTag::Keep);
    assert_eq!(entry.relative_path, PathBuf::from("orig_sel.wav"));
    assert_eq!(controller.wav_entries.len(), 1);
    assert_eq!(controller.ui.browser.visible.len(), 1);
    let exported_path = root.join(&entry.relative_path);
    assert!(exported_path.exists());
    let exported: Vec<f32> = hound::WavReader::open(&exported_path)
        .unwrap()
        .samples::<f32>()
        .map(|s| s.unwrap())
        .collect();
    assert_eq!(exported, vec![0.0, 0.25]);

    let db = controller.database_for(&source).unwrap();
    let rows = db.list_files().unwrap();
    let saved = rows
        .iter()
        .find(|row| row.relative_path == entry.relative_path)
        .unwrap();
    assert_eq!(saved.tag, SampleTag::Keep);
}

#[test]
fn waveform_image_resizes_to_view() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    let wav_path = source.root.join("resize.wav");
    write_test_wav(&wav_path, &[0.0, 0.25, -0.5, 0.75]);

    controller
        .load_waveform_for_selection(&source, Path::new("resize.wav"))
        .unwrap();
    controller.update_waveform_size(24, 8);

    let size = controller.ui.waveform.image.as_ref().unwrap().image.size;
    assert_eq!(size, [24, 8]);
}

#[test]
fn removing_selected_source_clears_waveform_view() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.cache_db(&source).unwrap();
    let wav_path = source.root.join("one.wav");
    write_test_wav(&wav_path, &[0.1, -0.1]);
    controller.wav_entries = vec![sample_entry("one.wav", SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller
        .load_waveform_for_selection(&source, Path::new("one.wav"))
        .unwrap();

    controller.remove_source(0);

    assert!(controller.ui.waveform.image.is_none());
    assert!(controller.ui.waveform.selection.is_none());
    assert!(controller.loaded_audio.is_none());
    assert!(controller.loaded_wav.is_none());
}

#[test]
fn switching_sources_resets_waveform_state() {
    let (mut controller, first) = dummy_controller();
    controller.sources.push(first.clone());
    controller.cache_db(&first).unwrap();
    let wav_path = first.root.join("a.wav");
    write_test_wav(&wav_path, &[0.0, 0.1]);
    controller.wav_entries = vec![sample_entry("a.wav", SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller
        .load_waveform_for_selection(&first, Path::new("a.wav"))
        .unwrap();

    let second_dir = tempdir().unwrap();
    let second_root = second_dir.path().join("second");
    std::fs::create_dir_all(&second_root).unwrap();
    mem::forget(second_dir);
    let second = SampleSource::new(second_root);
    controller.sources.push(second.clone());

    controller.select_source(Some(second.id.clone()));

    assert!(controller.ui.waveform.image.is_none());
    assert!(controller.ui.waveform.notice.is_none());
    assert!(controller.loaded_audio.is_none());
}

#[test]
fn pruning_missing_selection_clears_waveform_view() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.cache_db(&source).unwrap();
    let wav_path = source.root.join("gone.wav");
    write_test_wav(&wav_path, &[0.2, -0.2]);
    controller.wav_entries = vec![sample_entry("gone.wav", SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.selected_wav = Some(PathBuf::from("gone.wav"));
    controller
        .load_waveform_for_selection(&source, Path::new("gone.wav"))
        .unwrap();

    controller.wav_entries.clear();
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    assert!(controller.ui.waveform.image.is_none());
    assert!(controller.ui.waveform.selection.is_none());
    assert!(controller.loaded_audio.is_none());
    assert!(controller.loaded_wav.is_none());
}

#[test]
fn cropping_selection_overwrites_file() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    controller.cache_db(&source).unwrap();
    let wav_path = source.root.join("edit.wav");
    write_test_wav(&wav_path, &[0.1, 0.2, 0.3, 0.4]);
    controller.wav_entries = vec![sample_entry("edit.wav", SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller
        .load_waveform_for_selection(&source, Path::new("edit.wav"))
        .unwrap();
    controller.ui.waveform.selection = Some(SelectionRange::new(0.25, 0.75));

    controller.crop_waveform_selection().unwrap();

    let samples: Vec<f32> = hound::WavReader::open(&wav_path)
        .unwrap()
        .samples::<f32>()
        .map(|s| s.unwrap())
        .collect();
    assert_eq!(samples, vec![0.2, 0.3]);
    assert!(controller.ui.waveform.selection.is_none());
    assert_eq!(controller.ui.status.badge_label, "Info");
}

#[test]
fn trimming_selection_removes_span() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    controller.cache_db(&source).unwrap();
    let wav_path = source.root.join("trim.wav");
    write_test_wav(&wav_path, &[0.0, 0.1, 0.2, 0.3]);
    controller.wav_entries = vec![sample_entry("trim.wav", SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller
        .load_waveform_for_selection(&source, Path::new("trim.wav"))
        .unwrap();
    controller.ui.waveform.selection = Some(SelectionRange::new(0.25, 0.75));

    controller.trim_waveform_selection().unwrap();

    let samples: Vec<f32> = hound::WavReader::open(&wav_path)
        .unwrap()
        .samples::<f32>()
        .map(|s| s.unwrap())
        .collect();
    assert_eq!(samples, vec![0.0, 0.3]);
    assert!(controller.ui.waveform.selection.is_none());
    let entry = controller.wav_entries.first().unwrap();
    assert!(entry.file_size > 0);
}

#[test]
fn destructive_edit_request_prompts_without_yolo_mode() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    controller.cache_db(&source).unwrap();
    let wav_path = source.root.join("warn.wav");
    write_test_wav(&wav_path, &[0.0, 0.1, 0.2, 0.3]);
    controller.wav_entries = vec![sample_entry("warn.wav", SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller
        .load_waveform_for_selection(&source, Path::new("warn.wav"))
        .unwrap();
    controller.ui.waveform.selection = Some(SelectionRange::new(0.25, 0.75));

    let outcome = controller
        .request_destructive_selection_edit(DestructiveSelectionEdit::CropSelection)
        .unwrap();

    assert!(matches!(outcome, SelectionEditRequest::Prompted));
    assert!(controller.ui.waveform.pending_destructive.is_some());
    let samples: Vec<f32> = hound::WavReader::open(&wav_path)
        .unwrap()
        .samples::<f32>()
        .map(|s| s.unwrap())
        .collect();
    assert_eq!(samples.len(), 4);
}

#[test]
fn yolo_mode_applies_destructive_edit_immediately() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    controller.cache_db(&source).unwrap();
    let wav_path = source.root.join("yolo.wav");
    write_test_wav(&wav_path, &[0.1, 0.2, 0.3, 0.4]);
    controller.wav_entries = vec![sample_entry("yolo.wav", SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller
        .load_waveform_for_selection(&source, Path::new("yolo.wav"))
        .unwrap();
    controller.ui.waveform.selection = Some(SelectionRange::new(0.25, 0.75));
    controller.set_destructive_yolo_mode(true);

    let outcome = controller
        .request_destructive_selection_edit(DestructiveSelectionEdit::CropSelection)
        .unwrap();

    assert!(matches!(outcome, SelectionEditRequest::Applied));
    assert!(controller.ui.waveform.pending_destructive.is_none());
    let samples: Vec<f32> = hound::WavReader::open(&wav_path)
        .unwrap()
        .samples::<f32>()
        .map(|s| s.unwrap())
        .collect();
    assert_eq!(samples, vec![0.2, 0.3]);
}

#[test]
fn confirming_pending_destructive_edit_clears_prompt() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    controller.cache_db(&source).unwrap();
    let wav_path = source.root.join("confirm.wav");
    write_test_wav(&wav_path, &[0.0, 0.1, 0.2, 0.3]);
    controller.wav_entries = vec![sample_entry("confirm.wav", SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller
        .load_waveform_for_selection(&source, Path::new("confirm.wav"))
        .unwrap();
    controller.ui.waveform.selection = Some(SelectionRange::new(0.25, 0.75));
    controller
        .request_destructive_selection_edit(DestructiveSelectionEdit::TrimSelection)
        .unwrap();
    let prompt = controller.ui.waveform.pending_destructive.clone().unwrap();

    controller.apply_confirmed_destructive_edit(prompt.edit);

    assert!(controller.ui.waveform.pending_destructive.is_none());
    let samples: Vec<f32> = hound::WavReader::open(&wav_path)
        .unwrap()
        .samples::<f32>()
        .map(|s| s.unwrap())
        .collect();
    assert_eq!(samples, vec![0.0, 0.3]);
}

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
    controller.ui.drag.hovering_collection = Some(collection_id.clone());
    controller.ui.drag.hovering_drop_zone = true;
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
    controller.ui.drag.hovering_browser = Some(TriageFlagColumn::Keep);
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
    controller.ui.drag.hovering_collection = Some(collection_id.clone());
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
fn sample_drop_to_folder_moves_and_updates_state() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("source");
    std::fs::create_dir_all(root.join("dest")).unwrap();
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

    let mut collection = Collection::new("Dest");
    let collection_id = collection.id.clone();
    collection.add_member(source.id.clone(), PathBuf::from("one.wav"));
    controller.collections.push(collection);
    controller.selected_collection = Some(collection_id.clone());
    controller.refresh_collections_ui();

    controller.ui.drag.payload = Some(DragPayload::Sample {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("one.wav"),
    });
    controller.ui.drag.hovering_folder = Some(PathBuf::from("dest"));
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
    let root = temp.path().join("source");
    let dest = root.join("dest");
    std::fs::create_dir_all(&dest).unwrap();
    write_test_wav(&root.join("one.wav"), &[0.1, 0.2]);
    write_test_wav(&dest.join("one.wav"), &[0.3, 0.4]);
    let renderer = WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    controller.cache_db(&source).unwrap();

    controller.wav_entries = vec![sample_entry("one.wav", SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.ui.drag.payload = Some(DragPayload::Sample {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("one.wav"),
    });
    controller.ui.drag.hovering_folder = Some(PathBuf::from("dest"));
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

#[test]
fn creating_folder_tracks_manual_entry() -> Result<(), String> {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.refresh_folder_browser();
    assert!(controller.ui.sources.folders.rows[0].is_root);

    controller.create_folder(Path::new(""), "NewFolder")?;

    assert!(source.root.join("NewFolder").is_dir());
    assert!(
        controller
            .ui
            .sources
            .folders
            .rows
            .iter()
            .any(|row| row.path == PathBuf::from("NewFolder"))
    );
    Ok(())
}

#[test]
fn folder_browser_includes_root_entry() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    controller.refresh_folder_browser();

    let rows = &controller.ui.sources.folders.rows;
    assert!(
        rows.first()
            .is_some_and(|row| row.is_root && row.path.as_os_str().is_empty())
    );
}

#[test]
fn root_entry_stays_above_real_folders() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    let folder = source.root.join("rooted");
    std::fs::create_dir_all(&folder).unwrap();
    write_test_wav(&folder.join("clip.wav"), &[0.2, -0.2]);
    controller.wav_entries = vec![sample_entry("rooted/clip.wav", SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.refresh_folder_browser();

    let rows = &controller.ui.sources.folders.rows;
    assert!(rows.first().is_some_and(|row| row.is_root));
    assert!(
        rows.get(1)
            .is_some_and(|row| row.path == PathBuf::from("rooted"))
    );
}

#[test]
fn start_new_folder_at_root_sets_root_parent() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    controller.refresh_folder_browser();

    controller.start_new_folder_at_root();

    match &controller.ui.sources.folders.pending_action {
        Some(crate::egui_app::state::FolderActionPrompt::Create { parent, .. }) => {
            assert!(parent.as_os_str().is_empty())
        }
        other => panic!("unexpected action: {other:?}"),
    }
}

#[test]
fn selecting_root_clears_folder_selection() -> Result<(), String> {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    let folder = source.root.join("rooted");
    std::fs::create_dir_all(&folder).unwrap();
    write_test_wav(&folder.join("clip.wav"), &[0.2, -0.2]);
    controller.wav_entries = vec![sample_entry("rooted/clip.wav", SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.refresh_folder_browser();
    let folder_index = controller
        .ui
        .sources
        .folders
        .rows
        .iter()
        .position(|row| row.path == PathBuf::from("rooted"))
        .unwrap();

    controller.replace_folder_selection(folder_index);
    assert!(!controller.selected_folder_paths().is_empty());

    controller.replace_folder_selection(0);

    assert!(controller.selected_folder_paths().is_empty());
    assert_eq!(controller.ui.sources.folders.focused, Some(0));
    Ok(())
}

#[test]
fn renaming_folder_updates_entries_and_tree() -> Result<(), String> {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    let folder = source.root.join("old");
    std::fs::create_dir_all(&folder).unwrap();
    write_test_wav(&folder.join("clip.wav"), &[0.1, -0.1]);
    controller.wav_entries = vec![sample_entry("old/clip.wav", SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.refresh_folder_browser();

    controller.rename_folder(Path::new("old"), "new")?;

    assert!(!folder.exists());
    assert!(source.root.join("new/clip.wav").is_file());
    assert_eq!(
        controller.wav_entries[0].relative_path,
        PathBuf::from("new/clip.wav")
    );
    assert!(
        controller
            .ui
            .sources
            .folders
            .rows
            .iter()
            .any(|row| row.path == PathBuf::from("new"))
    );
    Ok(())
}

#[test]
fn cancelling_folder_rename_clears_prompt() {
    let (mut controller, _source) = dummy_controller();
    controller.ui.sources.folders.pending_action =
        Some(crate::egui_app::state::FolderActionPrompt::Rename {
            target: PathBuf::from("folder"),
            name: "folder".into(),
        });
    controller.ui.sources.folders.rename_focus_requested = true;

    controller.cancel_folder_rename();

    assert!(controller.ui.sources.folders.pending_action.is_none());
    assert!(!controller.ui.sources.folders.rename_focus_requested);
}

#[test]
fn deleting_folder_removes_wavs() -> Result<(), String> {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    let target = source.root.join("gone");
    std::fs::create_dir_all(&target).unwrap();
    write_test_wav(&target.join("sample.wav"), &[0.0, 0.2]);
    controller.wav_entries = vec![sample_entry("gone/sample.wav", SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.refresh_folder_browser();
    if let Some(index) = controller
        .ui
        .sources
        .folders
        .rows
        .iter()
        .position(|row| row.path == PathBuf::from("gone"))
    {
        controller.focus_folder_row(index);
    }

    controller.delete_focused_folder();

    assert!(!target.exists());
    assert!(controller.wav_entries.is_empty());
    assert!(
        controller
            .ui
            .sources
            .folders
            .rows
            .iter()
            .all(|row| row.path != PathBuf::from("gone"))
    );
    Ok(())
}

#[test]
fn deleting_folder_moves_focus_to_next_available() -> Result<(), String> {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    for folder in ["a", "b", "c"] {
        let path = source.root.join(folder);
        std::fs::create_dir_all(&path).unwrap();
        write_test_wav(&path.join(format!("{folder}.wav")), &[0.0, 0.2]);
    }
    controller.wav_entries = vec![
        sample_entry("a/a.wav", SampleTag::Neutral),
        sample_entry("b/b.wav", SampleTag::Neutral),
        sample_entry("c/c.wav", SampleTag::Neutral),
    ];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.refresh_folder_browser();
    let focus_row = controller
        .ui
        .sources
        .folders
        .rows
        .iter()
        .position(|row| row.path == PathBuf::from("b"))
        .unwrap();
    controller.focus_folder_row(focus_row);

    controller.delete_focused_folder();

    let focused = controller.ui.sources.folders.focused.unwrap();
    assert_eq!(
        controller.ui.sources.folders.rows[focused].path,
        PathBuf::from("c")
    );

    controller.delete_focused_folder();

    let focused = controller.ui.sources.folders.focused.unwrap();
    assert_eq!(
        controller.ui.sources.folders.rows[focused].path,
        PathBuf::from("a")
    );
    Ok(())
}

#[test]
fn folder_focus_clears_when_context_changes() -> Result<(), String> {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    controller.wav_entries = vec![sample_entry("one/sample.wav", SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.refresh_folder_browser();
    let row_index = controller
        .ui
        .sources
        .folders
        .rows
        .iter()
        .position(|row| row.path == PathBuf::from("one"))
        .unwrap();

    controller.replace_folder_selection(row_index);
    assert_eq!(controller.ui.sources.folders.focused, Some(row_index));

    controller.focus_browser_context();

    assert!(controller.ui.sources.folders.focused.is_none());
    controller.refresh_folder_browser();
    assert!(controller.ui.sources.folders.focused.is_none());
    assert_eq!(
        controller.selected_folder_paths(),
        vec![PathBuf::from("one")]
    );
    Ok(())
}

#[test]
fn clearing_folder_selection_shows_all_samples() -> Result<(), String> {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    std::fs::create_dir_all(source.root.join("a")).unwrap();
    std::fs::create_dir_all(source.root.join("b")).unwrap();
    controller.wav_entries = vec![
        sample_entry("a/one.wav", SampleTag::Neutral),
        sample_entry("b/two.wav", SampleTag::Neutral),
    ];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.refresh_folder_browser();

    let folder_a = controller
        .ui
        .sources
        .folders
        .rows
        .iter()
        .position(|row| row.path == PathBuf::from("a"))
        .unwrap();
    controller.replace_folder_selection(folder_a);

    assert_eq!(controller.selected_folder_paths(), vec![PathBuf::from("a")]);
    assert_eq!(controller.visible_browser_indices(), &[0]);

    controller.clear_folder_selection();

    assert!(controller.selected_folder_paths().is_empty());
    assert_eq!(controller.visible_browser_indices(), &[0, 1]);
    Ok(())
}

#[test]
fn fuzzy_search_filters_folders() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    let kick = source.root.join("kick");
    let snare = source.root.join("snare");
    std::fs::create_dir_all(&kick).unwrap();
    std::fs::create_dir_all(&snare).unwrap();
    controller.wav_entries = vec![
        sample_entry("kick/one.wav", SampleTag::Neutral),
        sample_entry("snare/two.wav", SampleTag::Neutral),
    ];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.refresh_folder_browser();

    controller.set_folder_search("snr".to_string());

    assert!(
        controller
            .ui
            .sources
            .folders
            .rows
            .iter()
            .all(|row| row.path.starts_with(Path::new("snare")))
    );
}

#[test]
fn browser_normalize_refreshes_exports() -> Result<(), String> {
    let temp = tempdir().unwrap();
    let root = temp.path().join("source");
    let export_root = temp.path().join("export");
    std::fs::create_dir_all(&root).unwrap();
    let renderer = WaveformRenderer::new(16, 16);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.selected_source = Some(source.id.clone());
    controller.sources.push(source.clone());

    write_test_wav(&root.join("one.wav"), &[0.25, -0.5]);
    controller.wav_entries = vec![sample_entry("one.wav", SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller
        .load_waveform_for_selection(&source, Path::new("one.wav"))
        .unwrap();

    let mut collection = Collection::new("Export");
    let collection_id = collection.id.clone();
    let manual_dir = export_root.join("Delete");
    std::fs::create_dir_all(&manual_dir).unwrap();
    collection.export_path = Some(manual_dir.clone());
    collection.add_member(source.id.clone(), PathBuf::from("one.wav"));
    controller.collections.push(collection);

    let member = CollectionMember {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("one.wav"),
    };
    controller.export_member_if_needed(&collection_id, &member)?;
    controller.normalize_browser_sample(0)?;

    let collection = controller
        .collections
        .iter()
        .find(|c| c.id == collection_id)
        .unwrap();
    let export_dir = collection_export::export_dir_for(collection, None)?;
    let exported_path = export_dir.join("one.wav");
    assert!(exported_path.is_file());
    assert!((max_sample_amplitude(&root.join("one.wav")) - 1.0).abs() < 1e-6);
    assert!((max_sample_amplitude(&exported_path) - 1.0).abs() < 1e-6);
    let loaded = controller.loaded_audio.as_ref().expect("loaded audio");
    let max_loaded = WavReader::new(Cursor::new(loaded.bytes.as_slice()))
        .unwrap()
        .samples::<f32>()
        .map(|s| s.unwrap().abs())
        .fold(0.0, f32::max);
    assert!((max_loaded - 1.0).abs() < 1e-6);
    Ok(())
}

#[test]
fn browser_delete_prunes_collections_and_exports() -> Result<(), String> {
    let temp = tempdir().unwrap();
    let root = temp.path().join("source");
    let export_root = temp.path().join("export");
    std::fs::create_dir_all(&root).unwrap();
    let renderer = WaveformRenderer::new(10, 10);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.selected_source = Some(source.id.clone());
    controller.sources.push(source.clone());

    write_test_wav(&root.join("delete.wav"), &[0.1, 0.2]);
    controller.wav_entries = vec![sample_entry("delete.wav", SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    let mut collection = Collection::new("Delete");
    let collection_id = collection.id.clone();
    let manual_dir = export_root.join("Delete");
    std::fs::create_dir_all(&manual_dir).unwrap();
    collection.export_path = Some(manual_dir.clone());
    collection.add_member(source.id.clone(), PathBuf::from("delete.wav"));
    controller.collections.push(collection);

    let member = CollectionMember {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("delete.wav"),
    };
    controller.export_member_if_needed(&collection_id, &member)?;
    controller.delete_browser_sample(0)?;

    assert!(!root.join("delete.wav").exists());
    let db = controller.database_for(&source).expect("open db");
    assert!(db.list_files().unwrap().is_empty());
    let collection = controller
        .collections
        .iter()
        .find(|c| c.id == collection_id)
        .unwrap();
    assert!(collection.members.is_empty());
    let export_dir = collection_export::export_dir_for(collection, None)?;
    assert!(!export_dir.join("delete.wav").exists());
    assert!(controller.wav_entries.is_empty());
    assert!(controller.ui.browser.visible.is_empty());
    Ok(())
}

#[test]
fn deleting_browser_sample_moves_focus_forward() -> Result<(), String> {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    for name in ["a.wav", "b.wav", "c.wav"] {
        write_test_wav(&source.root.join(name), &[0.1, -0.1]);
    }
    controller.wav_entries = vec![
        sample_entry("a.wav", SampleTag::Neutral),
        sample_entry("b.wav", SampleTag::Neutral),
        sample_entry("c.wav", SampleTag::Neutral),
    ];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.focus_browser_row_only(1);
    controller.delete_browser_sample(1)?;

    assert_eq!(controller.selected_wav.as_deref(), Some(Path::new("c.wav")));
    assert_eq!(controller.ui.browser.selected_visible, Some(1));

    controller.delete_browser_sample(1)?;

    assert_eq!(controller.selected_wav.as_deref(), Some(Path::new("a.wav")));
    assert_eq!(controller.ui.browser.selected_visible, Some(0));
    Ok(())
}

#[test]
fn moving_trashed_samples_moves_and_prunes_state() -> Result<(), String> {
    let temp = tempdir().unwrap();
    let trash_root = temp.path().join("trash");
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.trash_folder = Some(trash_root.clone());
    controller.ui.trash_folder = Some(trash_root.clone());

    let trash_file = source.root.join("trash.wav");
    let keep_file = source.root.join("keep.wav");
    write_test_wav(&trash_file, &[0.1, -0.1]);
    write_test_wav(&keep_file, &[0.2, -0.2]);

    let db = controller.database_for(&source).unwrap();
    db.upsert_file(Path::new("trash.wav"), 4, 1).unwrap();
    db.upsert_file(Path::new("keep.wav"), 4, 1).unwrap();
    db.set_tag(Path::new("trash.wav"), SampleTag::Trash)
        .unwrap();
    db.set_tag(Path::new("keep.wav"), SampleTag::Keep).unwrap();

    controller.wav_entries = vec![
        sample_entry("trash.wav", SampleTag::Trash),
        sample_entry("keep.wav", SampleTag::Keep),
    ];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.move_all_trashed_to_folder();

    assert!(trash_root.join("trash.wav").is_file());
    assert!(!source.root.join("trash.wav").exists());
    let rows = controller
        .database_for(&source)
        .unwrap()
        .list_files()
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].relative_path, PathBuf::from("keep.wav"));
    assert_eq!(rows[0].tag, SampleTag::Keep);
    assert_eq!(controller.wav_entries.len(), 1);
    assert!(
        controller
            .wav_entries
            .iter()
            .all(|entry| entry.relative_path != PathBuf::from("trash.wav"))
    );
    assert!(controller.ui.browser.trash.is_empty());
    Ok(())
}

#[test]
fn moving_trashed_samples_can_cancel_midway() -> Result<(), String> {
    let temp = tempdir().unwrap();
    let trash_root = temp.path().join("trash");
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.trash_folder = Some(trash_root.clone());
    controller.ui.trash_folder = Some(trash_root.clone());

    {
        let db = controller.database_for(&source).unwrap();
        for name in ["one.wav", "two.wav"] {
            let path = source.root.join(name);
            write_test_wav(&path, &[0.2, -0.2]);
            db.upsert_file(Path::new(name), 4, 1).unwrap();
            db.set_tag(Path::new(name), SampleTag::Trash).unwrap();
        }
    }

    controller.wav_entries = vec![
        sample_entry("one.wav", SampleTag::Trash),
        sample_entry("two.wav", SampleTag::Trash),
    ];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.progress_cancel_after = Some(1);

    controller.move_all_trashed_to_folder();

    assert!(trash_root.join("one.wav").is_file());
    assert!(!source.root.join("one.wav").exists());
    assert!(source.root.join("two.wav").exists());
    assert!(!controller.ui.progress.visible);
    assert!(controller.ui.status.text.contains("Canceled trash move"));
    Ok(())
}

#[test]
fn taking_out_trash_deletes_files() {
    let temp = tempdir().unwrap();
    let trash_root = temp.path().join("trash");
    std::fs::create_dir_all(trash_root.join("nested")).unwrap();
    std::fs::write(trash_root.join("junk.wav"), b"junk").unwrap();
    std::fs::write(trash_root.join("nested").join("more.wav"), b"more").unwrap();

    let (mut controller, _source) = dummy_controller();
    controller.trash_folder = Some(trash_root.clone());
    controller.ui.trash_folder = Some(trash_root.clone());

    controller.take_out_trash();

    assert!(trash_root.is_dir());
    assert!(!trash_root.join("junk.wav").exists());
    assert!(!trash_root.join("nested").join("more.wav").exists());
    let remaining: Vec<_> = std::fs::read_dir(&trash_root).unwrap().collect();
    assert!(remaining.is_empty());
}

#[test]
fn selecting_missing_sample_sets_waveform_notice() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    controller.wav_entries = vec![WavEntry {
        relative_path: PathBuf::from("one.wav"),
        file_size: 1,
        modified_ns: 1,
        tag: SampleTag::Neutral,
        missing: true,
    }];
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
    assert!(controller.loaded_audio.is_none());
}

#[test]
fn collection_views_flag_missing_members() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    controller.wav_entries = vec![WavEntry {
        relative_path: PathBuf::from("one.wav"),
        file_size: 1,
        modified_ns: 1,
        tag: SampleTag::Neutral,
        missing: true,
    }];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.rebuild_missing_lookup_for_source(&source.id);

    let mut collection = Collection::new("Test");
    collection.members.push(CollectionMember {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("one.wav"),
    });
    controller.selected_collection = Some(collection.id.clone());
    controller.collections.push(collection);
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
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    let rel = PathBuf::from("gone.wav");
    controller.wav_entries = vec![WavEntry {
        relative_path: rel.clone(),
        file_size: 1,
        modified_ns: 1,
        tag: SampleTag::Neutral,
        missing: false,
    }];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    let err = controller
        .load_waveform_for_selection(&source, &rel)
        .unwrap_err();
    assert!(err.contains("Failed to read"));
    assert!(controller.sample_missing(&source.id, &rel));
    assert!(controller.wav_entries[0].missing);
    assert!(
        controller
            .missing_wavs
            .get(&source.id)
            .is_some_and(|set| set.contains(&rel))
    );
}

#[test]
fn focusing_browser_row_updates_focus_context() {
    let (mut controller, source) = dummy_controller();
    prepare_browser_sample(&mut controller, &source, "focus.wav");
    controller.focus_browser_row(0);
    assert_eq!(controller.ui.focus.context, FocusContext::SampleBrowser);
}

#[test]
fn hotkey_search_browser_requests_focus() {
    let (mut controller, source) = dummy_controller();
    prepare_browser_sample(&mut controller, &source, "find.wav");
    controller.ui.browser.search_focus_requested = false;
    let action = hotkeys::iter_actions()
        .find(|a| a.id == "search-browser")
        .expect("search-browser hotkey");

    controller.handle_hotkey(action, FocusContext::SampleBrowser);

    assert!(controller.ui.browser.search_focus_requested);
    assert_eq!(controller.ui.focus.context, FocusContext::SampleBrowser);
}

#[test]
fn hotkey_focus_waveform_sets_context() {
    let (mut controller, source) = dummy_controller();
    prepare_browser_sample(&mut controller, &source, "wave.wav");
    controller.select_wav_by_path(Path::new("wave.wav"));
    let action = hotkeys::iter_actions()
        .find(|a| a.id == "focus-waveform")
        .expect("focus-waveform hotkey");
    controller.handle_hotkey(action, FocusContext::None);
    assert_eq!(controller.ui.focus.context, FocusContext::Waveform);
}

#[test]
fn selecting_collection_sample_updates_focus_context() {
    let (mut controller, source) = dummy_controller();
    prepare_browser_sample(&mut controller, &source, "col.wav");
    let mut collection = Collection::new("Test");
    collection.members.push(CollectionMember {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("col.wav"),
    });
    controller.collections.push(collection.clone());
    controller.selected_collection = Some(collection.id.clone());
    controller.refresh_collections_ui();
    controller.select_collection_sample(0);
    assert_eq!(controller.ui.focus.context, FocusContext::CollectionSample);
}

#[test]
fn hotkey_toggle_selection_dispatches_in_browser_context() {
    let (mut controller, source) = dummy_controller();
    prepare_browser_sample(&mut controller, &source, "toggle.wav");
    controller.focus_browser_row(0);
    assert_eq!(controller.ui.browser.selected_paths.len(), 1);
    let action = hotkeys::iter_actions()
        .find(|a| a.id == "toggle-select")
        .expect("toggle-select hotkey");
    controller.handle_hotkey(action, FocusContext::SampleBrowser);
    assert!(controller.ui.browser.selected_paths.is_empty());
}

#[test]
fn random_sample_selection_uses_seeded_rng() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.wav_entries = vec![
        sample_entry("one.wav", SampleTag::Neutral),
        sample_entry("two.wav", SampleTag::Neutral),
        sample_entry("three.wav", SampleTag::Neutral),
    ];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    let mut rng = StdRng::seed_from_u64(99);
    let expected = controller
        .visible_browser_indices()
        .iter()
        .enumerate()
        .choose(&mut rng)
        .map(|(row, _)| row);

    controller.play_random_visible_sample_with_seed(99);

    assert_eq!(controller.ui.browser.selected_visible, expected);
    assert_eq!(controller.ui.browser.selection_anchor_visible, expected);
    assert_eq!(controller.ui.focus.context, FocusContext::SampleBrowser);
    assert!(controller.ui.browser.autoscroll);
}

#[test]
fn random_sample_hotkey_is_registered() {
    let action = hotkeys::iter_actions()
        .find(|a| a.id == "play-random-sample")
        .expect("play-random-sample hotkey");
    assert_eq!(action.label, "Play random sample");
    assert!(action.is_global());
    assert_eq!(action.gesture.first.key, egui::Key::R);
    assert!(action.gesture.first.shift);
    assert!(!action.gesture.first.command);
    assert!(action.gesture.chord.is_none());
}

#[test]
fn random_history_hotkey_is_registered() {
    let action = hotkeys::iter_actions()
        .find(|a| a.id == "play-previous-random-sample")
        .expect("play-previous-random-sample hotkey");
    assert_eq!(action.label, "Play previous random sample");
    assert!(action.is_global());
    assert_eq!(action.gesture.first.key, egui::Key::R);
    assert!(action.gesture.first.shift);
    assert!(action.gesture.first.command);
    assert!(action.gesture.chord.is_none());
}

#[test]
fn random_navigation_toggle_hotkey_is_registered() {
    let action = hotkeys::iter_actions()
        .find(|a| a.id == "toggle-random-navigation-mode")
        .expect("toggle-random-navigation-mode hotkey");
    assert_eq!(action.label, "Toggle random navigation mode");
    assert!(action.is_global());
    assert_eq!(action.gesture.first.key, egui::Key::R);
    assert!(action.gesture.first.alt);
    assert!(!action.gesture.first.shift);
    assert!(!action.gesture.first.command);
    assert!(action.gesture.chord.is_none());
}

#[test]
fn trash_move_hotkeys_are_registered() {
    let base = hotkeys::iter_actions()
        .find(|a| a.id == "move-trashed-to-folder")
        .expect("move-trashed-to-folder hotkey");
    assert_eq!(base.label, "Move trashed samples to folder");
    assert!(base.is_global());
    assert_eq!(base.gesture.first.key, egui::Key::P);
    assert!(!base.gesture.first.shift);

    let shifted = hotkeys::iter_actions()
        .find(|a| a.id == "move-trashed-to-folder-shift")
        .expect("move-trashed-to-folder-shift hotkey");
    assert_eq!(shifted.label, "Move trashed samples to folder");
    assert!(shifted.is_global());
    assert_eq!(shifted.gesture.first.key, egui::Key::P);
    assert!(shifted.gesture.first.shift);
}

#[test]
fn trash_move_hotkey_moves_samples() -> Result<(), String> {
    let temp = tempdir().unwrap();
    let trash_root = temp.path().join("trash");
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.trash_folder = Some(trash_root.clone());
    controller.ui.trash_folder = Some(trash_root.clone());

    let trash_file = source.root.join("trash.wav");
    write_test_wav(&trash_file, &[0.1, -0.1]);

    let db = controller
        .database_for(&source)
        .map_err(|err| format!("open db: {err}"))?;
    db.upsert_file(Path::new("trash.wav"), 4, 1)
        .map_err(|err| format!("upsert: {err}"))?;
    db.set_tag(Path::new("trash.wav"), SampleTag::Trash)
        .map_err(|err| format!("tag: {err}"))?;

    controller.wav_entries = vec![sample_entry("trash.wav", SampleTag::Trash)];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    let action = hotkeys::iter_actions()
        .find(|a| a.id == "move-trashed-to-folder")
        .expect("move-trashed-to-folder hotkey");
    controller.handle_hotkey(action, FocusContext::None);

    assert!(trash_root.join("trash.wav").is_file());
    assert!(!trash_file.exists());
    assert!(controller.ui.browser.trash.is_empty());
    Ok(())
}

#[test]
fn random_history_steps_backward() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.wav_entries = vec![
        sample_entry("one.wav", SampleTag::Neutral),
        sample_entry("two.wav", SampleTag::Neutral),
    ];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    let mut rng = StdRng::seed_from_u64(5);
    let first_expected = controller
        .visible_browser_indices()
        .iter()
        .enumerate()
        .choose(&mut rng)
        .map(|(row, _)| row);
    controller.play_random_visible_sample_with_seed(5);

    let mut rng = StdRng::seed_from_u64(9);
    let second_expected = controller
        .visible_browser_indices()
        .iter()
        .enumerate()
        .choose(&mut rng)
        .map(|(row, _)| row);
    controller.play_random_visible_sample_with_seed(9);

    assert_eq!(controller.ui.browser.selected_visible, second_expected);
    assert_eq!(controller.random_history_cursor, Some(1));

    controller.play_previous_random_sample();

    assert_eq!(controller.random_history_cursor, Some(0));
    assert_eq!(controller.ui.browser.selected_visible, first_expected);
}

#[test]
fn random_history_trims_to_limit() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    let total = RANDOM_HISTORY_LIMIT + 5;
    controller.wav_entries = (0..total)
        .map(|i| sample_entry(&format!("{i}.wav"), SampleTag::Neutral))
        .collect();
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    for seed in 0..total as u64 {
        controller.play_random_visible_sample_with_seed(seed);
    }

    assert_eq!(controller.random_history.len(), RANDOM_HISTORY_LIMIT);
    assert_eq!(
        controller.random_history_cursor,
        Some(controller.random_history.len().saturating_sub(1))
    );
}

#[test]
fn random_sample_handles_empty_lists() {
    let (mut controller, _source) = dummy_controller();

    controller.play_random_visible_sample_with_seed(7);

    assert_eq!(
        controller.ui.status.text,
        "No samples available to randomize"
    );
    assert_eq!(controller.ui.browser.selected_visible, None);
}

#[test]
fn random_navigation_mode_toggles_state_and_status() {
    let (mut controller, _source) = dummy_controller();

    assert!(!controller.random_navigation_mode_enabled());

    controller.toggle_random_navigation_mode();

    assert!(controller.random_navigation_mode_enabled());
    assert_eq!(
        controller.ui.status.text,
        "Random navigation on: Up/Down jump to random samples"
    );

    controller.toggle_random_navigation_mode();

    assert!(!controller.random_navigation_mode_enabled());
    assert_eq!(controller.ui.status.text, "Random navigation off");
}

#[test]
fn cursor_step_size_tracks_view_zoom() {
    let (mut controller, source) = dummy_controller();
    prepare_browser_sample(&mut controller, &source, "zoom.wav");
    controller.update_waveform_size(200, 10);
    controller.select_wav_by_path(Path::new("zoom.wav"));
    controller.decoded_waveform = Some(DecodedWaveform {
        samples: vec![0.0; 10_000],
        duration_seconds: 1.0,
        sample_rate: 48_000,
        channels: 1,
    });
    controller.ui.waveform.playhead.position = 0.1;
    controller.ui.waveform.playhead.visible = true;
    controller.set_waveform_cursor(0.5);

    controller.move_playhead_steps(1, false, false);
    assert!((controller.ui.waveform.cursor.unwrap() - 0.66).abs() < 0.001);
    assert!((controller.ui.waveform.playhead.position - 0.1).abs() < 0.001);

    controller.zoom_waveform(true);
    controller.move_playhead_steps(1, false, false);
    assert!((controller.ui.waveform.cursor.unwrap() - 0.804).abs() < 0.001);
    assert!((controller.ui.waveform.playhead.position - 0.1).abs() < 0.001);
}

#[test]
fn batched_zoom_matches_sequential_steps() {
    let (mut batched, source_a) = dummy_controller();
    prepare_browser_sample(&mut batched, &source_a, "zoom.wav");
    batched.update_waveform_size(240, 24);
    batched.select_wav_by_path(Path::new("zoom.wav"));
    batched.ui.waveform.playhead.position = 0.4;
    batched.ui.waveform.playhead.visible = true;

    let (mut stepped, source_b) = dummy_controller();
    prepare_browser_sample(&mut stepped, &source_b, "zoom.wav");
    stepped.update_waveform_size(240, 24);
    stepped.select_wav_by_path(Path::new("zoom.wav"));
    stepped.ui.waveform.playhead.position = 0.4;
    stepped.ui.waveform.playhead.visible = true;

    batched.zoom_waveform_steps(true, 3, None);
    for _ in 0..3 {
        stepped.zoom_waveform(true);
    }

    let view_a = batched.ui.waveform.view;
    let view_b = stepped.ui.waveform.view;
    assert!((view_a.start - view_b.start).abs() < 1e-6);
    assert!((view_a.end - view_b.end).abs() < 1e-6);
}

#[test]
fn mouse_zoom_prefers_pointer_over_playhead() {
    let (mut controller, _source) = dummy_controller();
    controller.waveform_size = [240, 24];
    controller.decoded_waveform = Some(DecodedWaveform {
        samples: vec![0.0; 10_000],
        duration_seconds: 1.0,
        sample_rate: 48_000,
        channels: 1,
    });
    controller.ui.waveform.playhead.position = 0.1;
    controller.ui.waveform.playhead.visible = true;

    controller.zoom_waveform_steps_with_factor(true, 1, Some(0.8), Some(0.5), false, false);

    let center = (controller.ui.waveform.view.start + controller.ui.waveform.view.end) * 0.5;
    let playhead_dist = (center - 0.1).abs();
    let pointer_dist = (center - 0.8).abs();
    assert!(
        pointer_dist < playhead_dist,
        "zoom centered closer to playhead ({playhead_dist}) than pointer ({pointer_dist}), center {center}"
    );
    assert!(controller.ui.waveform.view.start < controller.ui.waveform.view.end);
}

#[test]
fn last_start_marker_clamps_and_resets() {
    let (mut controller, source) = dummy_controller();
    prepare_browser_sample(&mut controller, &source, "marker.wav");

    controller.record_play_start(-0.25);
    assert_eq!(controller.ui.waveform.last_start_marker, Some(0.0));

    controller.record_play_start(0.75);
    assert_eq!(controller.ui.waveform.last_start_marker, Some(0.75));

    controller.clear_waveform_view();
    assert!(controller.ui.waveform.last_start_marker.is_none());
}

#[test]
fn selecting_new_sample_clears_last_start_marker() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    write_test_wav(&source.root.join("a.wav"), &[0.0, 0.1]);
    write_test_wav(&source.root.join("b.wav"), &[0.2, -0.2]);
    controller.wav_entries = vec![
        sample_entry("a.wav", SampleTag::Neutral),
        sample_entry("b.wav", SampleTag::Neutral),
    ];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.select_wav_by_path(Path::new("a.wav"));
    controller.record_play_start(0.25);
    controller.select_wav_by_path(Path::new("b.wav"));

    assert!(controller.ui.waveform.last_start_marker.is_none());
}

#[test]
fn replay_from_last_start_requeues_pending_playback() {
    let (mut controller, source) = dummy_controller();
    prepare_browser_sample(&mut controller, &source, "marker.wav");
    controller.select_wav_by_path(Path::new("marker.wav"));
    controller.record_play_start(0.42);
    controller.ui.waveform.playhead.visible = true;
    controller.ui.waveform.playhead.position = 0.1;

    let handled = controller.replay_from_last_start();
    assert!(handled);
    let pending = controller
        .pending_playback
        .as_ref()
        .expect("pending playback request");
    assert_eq!(pending.start_override, Some(0.42));
}

#[test]
fn replay_from_last_start_falls_back_to_cursor() {
    let (mut controller, source) = dummy_controller();
    prepare_browser_sample(&mut controller, &source, "marker.wav");
    controller.select_wav_by_path(Path::new("marker.wav"));
    controller.ui.waveform.cursor = Some(0.25);
    controller.ui.waveform.last_start_marker = None;

    let handled = controller.replay_from_last_start();

    assert!(handled);
    let pending = controller
        .pending_playback
        .as_ref()
        .expect("pending playback request");
    assert_eq!(pending.start_override, Some(0.25));
    assert_eq!(controller.ui.waveform.last_start_marker, Some(0.25));
}

#[test]
fn play_from_cursor_prefers_cursor_position() {
    let (mut controller, source) = dummy_controller();
    prepare_browser_sample(&mut controller, &source, "cursor.wav");
    controller.select_wav_by_path(Path::new("cursor.wav"));
    controller.decoded_waveform = Some(DecodedWaveform {
        samples: vec![0.0; 10_000],
        duration_seconds: 1.0,
        sample_rate: 48_000,
        channels: 1,
    });
    controller.ui.waveform.cursor = Some(0.33);
    controller.ui.waveform.last_start_marker = Some(0.1);

    let handled = controller.play_from_cursor();

    assert!(handled);
    let pending = controller
        .pending_playback
        .as_ref()
        .expect("pending playback request");
    assert_eq!(pending.start_override, Some(0.33));
    assert_eq!(controller.ui.waveform.last_start_marker, Some(0.33));
}

#[test]
fn cursor_alpha_fades_before_reset() {
    let (mut controller, source) = dummy_controller();
    prepare_browser_sample(&mut controller, &source, "cursor.wav");
    controller.decoded_waveform = Some(DecodedWaveform {
        samples: vec![0.0; 10_000],
        duration_seconds: 1.0,
        sample_rate: 48_000,
        channels: 1,
    });
    controller.ui.waveform.cursor = Some(0.4);
    controller.ui.waveform.cursor_last_navigation_at =
        Some(Instant::now() - Duration::from_millis(250));

    let alpha = controller.waveform_cursor_alpha(false);

    assert!((alpha - 0.5).abs() < 0.15);
    assert_eq!(controller.ui.waveform.cursor, Some(0.4));
}

#[test]
fn cursor_alpha_resets_after_idle_timeout() {
    let (mut controller, source) = dummy_controller();
    prepare_browser_sample(&mut controller, &source, "cursor.wav");
    controller.decoded_waveform = Some(DecodedWaveform {
        samples: vec![0.0; 10_000],
        duration_seconds: 1.0,
        sample_rate: 48_000,
        channels: 1,
    });
    controller.ui.waveform.cursor = Some(0.4);
    controller.ui.waveform.cursor_last_navigation_at =
        Some(Instant::now() - Duration::from_millis(600));

    let alpha = controller.waveform_cursor_alpha(false);

    assert!(alpha <= f32::EPSILON);
    assert_eq!(controller.ui.waveform.cursor, Some(0.0));
}

#[test]
fn cursor_does_not_fade_when_waveform_focused() {
    let (mut controller, source) = dummy_controller();
    prepare_browser_sample(&mut controller, &source, "cursor.wav");
    controller.decoded_waveform = Some(DecodedWaveform {
        samples: vec![0.0; 10_000],
        duration_seconds: 1.0,
        sample_rate: 48_000,
        channels: 1,
    });
    controller.ui.waveform.cursor = Some(0.4);
    controller.ui.waveform.cursor_last_navigation_at =
        Some(Instant::now() - Duration::from_millis(800));
    controller.ui.focus.context = FocusContext::Waveform;

    let alpha = controller.waveform_cursor_alpha(false);

    assert_eq!(alpha, 1.0);
    assert_eq!(controller.ui.waveform.cursor, Some(0.4));
}

#[test]
fn navigation_steps_anchor_to_cursor_instead_of_playhead() {
    let (mut controller, source) = dummy_controller();
    prepare_browser_sample(&mut controller, &source, "nav.wav");
    controller.update_waveform_size(200, 10);
    controller.select_wav_by_path(Path::new("nav.wav"));
    controller.decoded_waveform = Some(DecodedWaveform {
        samples: vec![0.0; 10_000],
        duration_seconds: 1.0,
        sample_rate: 48_000,
        channels: 1,
    });
    controller.ui.waveform.playhead.position = 0.7;
    controller.ui.waveform.playhead.visible = true;
    controller.ui.waveform.cursor = Some(0.2);
    controller.ui.waveform.last_start_marker = Some(0.2);

    controller.move_playhead_steps(1, false, false);

    let cursor = controller.ui.waveform.cursor.expect("cursor set");
    assert!((cursor - 0.36).abs() < 0.001);
    assert!((controller.ui.waveform.playhead.position - 0.7).abs() < 0.001);
}

#[test]
fn waveform_refresh_respects_view_slice_and_caps_width() {
    let (mut controller, _source) = dummy_controller();
    controller.waveform_size = [100, 10];
    controller.ui.waveform.view = WaveformView {
        start: 0.25,
        end: 0.5,
    };
    controller.decoded_waveform = Some(DecodedWaveform {
        samples: (0..1000).map(|i| i as f32).collect(),
        duration_seconds: 1.0,
        sample_rate: 48_000,
        channels: 1,
    });
    controller.waveform_render_meta = None;
    controller.refresh_waveform_image();
    let image = controller
        .ui
        .waveform
        .image
        .as_ref()
        .expect("waveform image");
    assert!((image.view_start - 0.25).abs() < 1e-6);
    assert!((image.view_end - 0.5).abs() < 1e-6);
    let expected_width =
        (controller.waveform_size[0] as f32 * (1.0f32 / 0.25).min(64.0f32)).ceil() as usize;
    let samples_in_view = (0.5 - 0.25) * 1000.0;
    let upper = (samples_in_view as usize)
        .min(crate::egui_app::controller::wavs::MAX_TEXTURE_WIDTH as usize)
        .max(1);
    let lower = controller.waveform_size[0]
        .min(crate::egui_app::controller::wavs::MAX_TEXTURE_WIDTH) as usize;
    let clamped = expected_width.min(upper).max(lower);
    assert_eq!(image.image.size[0], clamped);
    assert_eq!(image.image.size[1], 10);
}

#[test]
fn waveform_render_meta_rejects_small_shifts_when_zoomed_in() {
    let base = super::wavs::WaveformRenderMeta {
        view_start: 0.10,
        view_end: 0.1009,
        size: [240, 32],
        samples_len: 10_000,
        texture_width: 8_000,
        channel_view: crate::waveform::WaveformChannelView::Mono,
        channels: 2,
    };
    let shifted = super::wavs::WaveformRenderMeta {
        view_start: 0.10095,
        view_end: 0.10185,
        ..base
    };
    assert!(!base.matches(&shifted));
}

#[test]
fn waveform_render_meta_allows_small_shifts_on_full_view() {
    let base = super::wavs::WaveformRenderMeta {
        view_start: 0.0,
        view_end: 1.0,
        size: [240, 32],
        samples_len: 10_000,
        texture_width: 2_000,
        channel_view: crate::waveform::WaveformChannelView::Mono,
        channels: 1,
    };
    let minor_shift = super::wavs::WaveformRenderMeta {
        view_start: 0.0005,
        view_end: 1.0005,
        ..base
    };
    assert!(base.matches(&minor_shift));
}

#[test]
fn waveform_rerenders_after_same_length_edit() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    controller.waveform_size = [32, 8];
    let path = source.root.join("edit.wav");
    write_test_wav(&path, &[0.1, 0.1, 0.1, 0.1]);

    controller
        .load_waveform_for_selection(&source, Path::new("edit.wav"))
        .unwrap();
    let before = controller
        .ui
        .waveform
        .image
        .as_ref()
        .expect("waveform image")
        .image
        .clone();

    write_test_wav(&path, &[1.0, -1.0, 1.0, -1.0]);
    controller.refresh_waveform_for_sample(&source, Path::new("edit.wav"));
    let after = controller
        .ui
        .waveform
        .image
        .as_ref()
        .expect("refreshed waveform image")
        .image
        .clone();

    assert_ne!(before.pixels, after.pixels);
}

#[test]
fn stale_audio_results_are_ignored() {
    let (mut controller, source) = dummy_controller();
    controller.feature_flags.autoplay_selection = false;
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    write_test_wav(&source.root.join("a.wav"), &[0.0, 0.1]);
    write_test_wav(&source.root.join("b.wav"), &[0.0, -0.1]);
    controller.wav_entries = vec![
        sample_entry("a.wav", SampleTag::Neutral),
        sample_entry("b.wav", SampleTag::Neutral),
    ];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.select_wav_by_path(Path::new("a.wav"));
    controller.select_wav_by_path(Path::new("b.wav"));

    for _ in 0..20 {
        controller.poll_audio_loader();
        if controller.loaded_wav.as_deref() == Some(Path::new("b.wav")) {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }

    assert_eq!(controller.loaded_wav.as_deref(), Some(Path::new("b.wav")));
    assert_eq!(
        controller.ui.loaded_wav.as_deref(),
        Some(Path::new("b.wav"))
    );
    assert!(controller.pending_audio.is_none());
}

#[test]
fn play_request_is_deferred_until_audio_ready() {
    let (mut controller, source) = dummy_controller();
    controller.feature_flags.autoplay_selection = false;
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    write_test_wav(&source.root.join("wait.wav"), &[0.0, 0.2, -0.2]);
    controller.wav_entries = vec![sample_entry("wait.wav", SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.select_wav_by_path(Path::new("wait.wav"));
    assert!(controller.pending_playback.is_none());
    let result = controller.play_audio(false, None);
    assert!(result.is_ok());
    let pending = controller
        .pending_playback
        .as_ref()
        .expect("pending playback to be queued");
    assert_eq!(pending.relative_path, PathBuf::from("wait.wav"));
    assert_eq!(pending.source_id, source.id);
    assert!(!pending.looped);
}

#[test]
fn loading_flag_clears_after_audio_load() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    let rel = PathBuf::from("load.wav");
    write_test_wav(&source.root.join(&rel), &[0.0, 0.5, -0.5]);
    controller.wav_entries = vec![sample_entry("load.wav", SampleTag::Neutral)];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller
        .queue_audio_load_for(&source, &rel, AudioLoadIntent::Selection, None)
        .expect("queue load");
    assert_eq!(
        controller.ui.waveform.loading.as_deref(),
        Some(rel.as_path())
    );

    for _ in 0..50 {
        controller.poll_audio_loader();
        if controller.loaded_wav.as_deref() == Some(rel.as_path()) {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }

    assert_eq!(controller.loaded_wav.as_deref(), Some(rel.as_path()));
    assert!(controller.pending_audio.is_none());
    assert!(controller.ui.waveform.loading.is_none());
    assert!(controller.loaded_audio.is_some());
}
