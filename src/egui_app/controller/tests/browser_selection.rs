use super::super::test_support::{dummy_controller, sample_entry, write_test_wav};
use super::super::*;
use crate::app_dirs::ConfigBaseGuard;
use crate::egui_app::controller::collection_export;
use crate::egui_app::state::FocusContext;
use crate::sample_sources::Collection;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tempfile::tempdir;

#[test]
fn hotkey_tagging_applies_to_all_selected_rows() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.cache_db(&source).unwrap();
    controller.set_wav_entries_for_tests( vec![
        sample_entry("one.wav", SampleTag::Neutral),
        sample_entry("two.wav", SampleTag::Neutral),
    ]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.focus_browser_row_only(0);
    controller.toggle_browser_row_selection(1);
    controller.tag_selected_left();

    assert_eq!(controller.wav_entry(0).unwrap().tag, SampleTag::Trash);
    assert_eq!(controller.wav_entry(1).unwrap().tag, SampleTag::Trash);
}

#[test]
fn collection_hotkey_moves_selected_samples() {
    let temp = tempdir().unwrap();
    let _guard = ConfigBaseGuard::set(temp.path().to_path_buf());
    let export_root = temp.path().join("export");
    std::fs::create_dir_all(&export_root).unwrap();
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.cache_db(&source).unwrap();
    controller.settings.collection_export_root = Some(export_root.clone());
    controller.ui.collection_export_root = Some(export_root.clone());
    for name in ["one.wav", "two.wav"] {
        write_test_wav(&source.root.join(name), &[0.0]);
    }
    controller.set_wav_entries_for_tests(vec![
        sample_entry("one.wav", SampleTag::Neutral),
        sample_entry("two.wav", SampleTag::Neutral),
    ]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    let mut collection = Collection::new("Hotkey");
    collection.hotkey = Some(1);
    let collection_id = collection.id.clone();
    let export_dir = export_root.join(collection_export::collection_folder_name(&collection));
    controller.library.collections.push(collection);
    controller.focus_browser_row_only(0);
    controller.toggle_browser_row_selection(1);
    let handled = controller.apply_collection_hotkey(1, FocusContext::SampleBrowser);
    assert!(handled);
    await_collection_move(&mut controller);
    let collection = controller
        .library
        .collections
        .iter()
        .find(|item| item.id == collection_id)
        .unwrap();
    assert_eq!(collection.members.len(), 2);
    assert!(export_dir.join("one.wav").exists());
    assert!(export_dir.join("two.wav").exists());
    assert!(!source.root.join("one.wav").exists());
    assert!(!source.root.join("two.wav").exists());
    assert!(controller.wav_index_for_path(&PathBuf::from("one.wav")).is_none());
    assert!(controller.wav_index_for_path(&PathBuf::from("two.wav")).is_none());
}

fn await_collection_move(controller: &mut EguiController) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while controller.runtime.jobs.collection_move_in_progress() && Instant::now() < deadline {
        controller.poll_background_jobs();
        std::thread::sleep(Duration::from_millis(5));
    }
    controller.poll_background_jobs();
    assert!(
        !controller.runtime.jobs.collection_move_in_progress(),
        "collection move job did not finish"
    );
}

#[test]
fn escape_clears_selection() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.cache_db(&source).unwrap();
    controller.set_wav_entries_for_tests( vec![
        sample_entry("one.wav", SampleTag::Neutral),
        sample_entry("two.wav", SampleTag::Neutral),
    ]);
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
fn update_selection_paths_rewrites_browser_selected_paths() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.ui.browser.selected_paths = vec![
        PathBuf::from("old.wav"),
        PathBuf::from("keep.wav"),
    ];

    controller.update_selection_paths(
        &source,
        Path::new("old.wav"),
        Path::new("new.wav"),
    );

    assert_eq!(
        controller.ui.browser.selected_paths,
        vec![PathBuf::from("new.wav"), PathBuf::from("keep.wav")]
    );
}

#[test]
fn update_cached_entry_replaces_old_path_in_lookup() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.set_wav_entries_for_tests(vec![sample_entry("old.wav", SampleTag::Neutral)]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.ui.browser.selected_paths = vec![PathBuf::from("old.wav")];

    let mut updated = sample_entry("new.wav", SampleTag::Neutral);
    updated.file_size = 10;
    updated.modified_ns = 7;
    controller.update_cached_entry(&source, Path::new("old.wav"), updated);

    assert!(controller.wav_index_for_path(Path::new("old.wav")).is_none());
    assert!(controller.wav_index_for_path(Path::new("new.wav")).is_some());
    assert_eq!(
        controller.ui.browser.selected_paths,
        vec![PathBuf::from("new.wav")]
    );
}

#[test]
fn select_all_populates_visible_browser_paths() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source);
    controller.set_wav_entries_for_tests( vec![
        sample_entry("one.wav", SampleTag::Neutral),
        sample_entry("two.wav", SampleTag::Neutral),
        sample_entry("three.wav", SampleTag::Neutral),
    ]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.select_all_browser_rows();

    assert_eq!(controller.ui.browser.selected_paths.len(), 3);
    assert_eq!(controller.ui.browser.selection_anchor_visible, Some(0));
}

#[test]
fn escape_handler_clears_waveform_and_browser_state() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.cache_db(&source).unwrap();
    controller.set_wav_entries_for_tests( vec![sample_entry("one.wav", SampleTag::Neutral)]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller
        .selection_state
        .range
        .set_range(Some(SelectionRange::new(0.2, 0.8)));
    controller.apply_selection(controller.selection_state.range.range());
    controller
        .ui
        .browser
        .selected_paths
        .push(PathBuf::from("one.wav"));
    controller.ui.browser.selection_anchor_visible = Some(0);

    controller.handle_escape();

    assert!(controller.selection_state.range.range().is_none());
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
    controller.library.sources.push(source.clone());
    controller.cache_db(&source).unwrap();
    controller.set_wav_entries_for_tests( vec![sample_entry("one.wav", SampleTag::Neutral)]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller
        .selection_state
        .range
        .set_range(Some(SelectionRange::new(0.25, 0.75)));
    controller.apply_selection(controller.selection_state.range.range());
    controller.audio.player = Some(std::rc::Rc::new(std::cell::RefCell::new(player)));

    controller.handle_escape();

    assert!(controller.selection_state.range.range().is_some());
    assert!(controller.ui.waveform.selection.is_some());
    assert!(!controller.is_playing());
}

#[test]
fn click_clears_selection_and_focuses_row() {
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
    controller.library.sources.push(source);
    controller.set_wav_entries_for_tests( vec![
        sample_entry("one.wav", SampleTag::Neutral),
        sample_entry("two.wav", SampleTag::Neutral),
        sample_entry("three.wav", SampleTag::Neutral),
    ]);
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
    controller.library.sources.push(source);
    controller.set_wav_entries_for_tests( vec![
        sample_entry("one.wav", SampleTag::Neutral),
        sample_entry("two.wav", SampleTag::Neutral),
        sample_entry("three.wav", SampleTag::Neutral),
    ]);
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
    controller.library.sources.push(source);
    controller.set_wav_entries_for_tests( vec![
        sample_entry("one.wav", SampleTag::Neutral),
        sample_entry("two.wav", SampleTag::Neutral),
        sample_entry("three.wav", SampleTag::Neutral),
        sample_entry("four.wav", SampleTag::Neutral),
        sample_entry("five.wav", SampleTag::Neutral),
        sample_entry("six.wav", SampleTag::Neutral),
    ]);
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
    controller.library.sources.push(source);
    controller.set_wav_entries_for_tests( vec![
        sample_entry("one.wav", SampleTag::Neutral),
        sample_entry("two.wav", SampleTag::Neutral),
        sample_entry("three.wav", SampleTag::Neutral),
    ]);
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
