use super::super::test_support::{dummy_controller, sample_entry, write_test_wav};
use super::super::*;
use crate::egui_app::controller::hotkeys;
use super::common::prepare_browser_sample;
use crate::egui_app::state::FocusContext;
use crate::sample_sources::Collection;
use crate::sample_sources::collections::CollectionMember;
use egui::Key;
use rand::rngs::StdRng;
use rand::seq::IteratorRandom;
use rand::SeedableRng;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

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
        clip_root: None,
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
    assert_eq!(action.gesture.first.key, Key::R);
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
    assert_eq!(action.gesture.first.key, Key::R);
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
    assert_eq!(action.gesture.first.key, Key::R);
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
