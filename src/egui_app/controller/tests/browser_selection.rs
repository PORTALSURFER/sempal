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
