use super::super::test_support::{dummy_controller, sample_entry, write_test_wav};
use super::super::*;
use super::common::{prepare_browser_sample, visible_indices};
use crate::egui_app::controller::hotkeys;
use crate::egui_app::state::FocusContext;
use crate::sample_sources::Collection;
use crate::sample_sources::collections::CollectionMember;
use egui::Key;
use rand::SeedableRng;
use rand::rngs::StdRng;
use rand::seq::IteratorRandom;
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
fn find_similar_hotkey_is_registered() {
    let action = hotkeys::iter_actions()
        .find(|a| a.id == "find-similar")
        .expect("find-similar hotkey");
    assert_eq!(action.label, "Toggle find similar");
    assert!(!action.is_global());
    assert_eq!(action.gesture.first.key, Key::F);
    assert!(action.gesture.first.shift);
    assert!(!action.gesture.first.command);
    assert!(!action.gesture.first.alt);
    assert!(action.gesture.chord.is_none());
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
    controller.library.collections.push(collection.clone());
    controller.selection_state.ctx.selected_collection = Some(collection.id.clone());
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
    controller.library.sources.push(source.clone());
    controller.set_wav_entries_for_tests( vec![
        sample_entry("one.wav", SampleTag::Neutral),
        sample_entry("two.wav", SampleTag::Neutral),
        sample_entry("three.wav", SampleTag::Neutral),
    ]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    let mut rng = StdRng::seed_from_u64(99);
    let expected = visible_indices(&controller)
        .into_iter()
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
fn tag_neutral_hotkey_is_registered() {
    let action = hotkeys::iter_actions()
        .find(|a| a.id == "tag-neutral")
        .expect("tag-neutral hotkey");
    assert_eq!(action.label, "Neutral sample(s)");
    assert!(action.is_global());
    assert_eq!(action.gesture.first.key, Key::Quote);
    assert!(!action.gesture.first.shift);
    assert!(!action.gesture.first.command);
    assert!(!action.gesture.first.alt);
    assert!(action.gesture.chord.is_none());
}

#[test]
fn quote_hotkey_tags_selected_sample_neutral() {
    let (mut controller, source) = dummy_controller();
    prepare_browser_sample(&mut controller, &source, "neutral.wav");
    controller.wav_entries.entry_mut(0).unwrap().tag = SampleTag::Keep;
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.focus_browser_row(0);

    let action = hotkeys::iter_actions()
        .find(|a| a.id == "tag-neutral")
        .expect("tag-neutral hotkey");
    controller.handle_hotkey(action, FocusContext::None);

    assert_eq!(controller.wav_entry(0).unwrap().tag, SampleTag::Neutral);
}

#[test]
fn tag_hotkeys_apply_to_collection_focus() {
    let (mut controller, source) = dummy_controller();
    controller.cache_db(&source).unwrap();
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    write_test_wav(&source.root.join("col.wav"), &[0.1, 0.2]);

    let collection = Collection::new("Test");
    let collection_id = collection.id.clone();
    controller.library.collections.push(collection);
    controller.selection_state.ctx.selected_collection = Some(collection_id.clone());
    controller
        .add_sample_to_collection(&collection_id, Path::new("col.wav"))
        .unwrap();
    controller.refresh_collections_ui();
    controller.select_collection_sample(0);
    assert_eq!(controller.ui.focus.context, FocusContext::CollectionSample);

    let keep = hotkeys::iter_actions()
        .find(|a| a.id == "tag-keep")
        .unwrap();
    controller.handle_hotkey(keep, FocusContext::CollectionSample);
    assert_eq!(
        controller.ui.collections.samples[0].tag,
        SampleTag::Keep,
        "keep hotkey"
    );

    let neutral = hotkeys::iter_actions()
        .find(|a| a.id == "tag-neutral")
        .unwrap();
    controller.handle_hotkey(neutral, FocusContext::CollectionSample);
    assert_eq!(
        controller.ui.collections.samples[0].tag,
        SampleTag::Neutral,
        "neutral hotkey"
    );

    let trash = hotkeys::iter_actions()
        .find(|a| a.id == "tag-trash")
        .unwrap();
    controller.handle_hotkey(trash, FocusContext::CollectionSample);
    assert_eq!(
        controller.ui.collections.samples[0].tag,
        SampleTag::Trash,
        "trash hotkey"
    );
}

#[test]
fn trash_move_hotkey_moves_samples() -> Result<(), String> {
    let temp = tempdir().unwrap();
    let trash_root = temp.path().join("trash");
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.settings.trash_folder = Some(trash_root.clone());
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

    controller.set_wav_entries_for_tests( vec![sample_entry("trash.wav", SampleTag::Trash)]);
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
    controller.library.sources.push(source.clone());
    controller.set_wav_entries_for_tests( vec![
        sample_entry("one.wav", SampleTag::Neutral),
        sample_entry("two.wav", SampleTag::Neutral),
    ]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    let mut rng = StdRng::seed_from_u64(5);
    let first_expected = visible_indices(&controller)
        .into_iter()
        .enumerate()
        .choose(&mut rng)
        .map(|(row, _)| row);
    controller.play_random_visible_sample_with_seed(5);

    let mut rng = StdRng::seed_from_u64(9);
    let second_expected = visible_indices(&controller)
        .into_iter()
        .enumerate()
        .choose(&mut rng)
        .map(|(row, _)| row);
    controller.play_random_visible_sample_with_seed(9);

    assert_eq!(controller.ui.browser.selected_visible, second_expected);
    assert_eq!(controller.history.random_history.cursor, Some(1));

    controller.play_previous_random_sample();

    assert_eq!(controller.history.random_history.cursor, Some(0));
    assert_eq!(controller.ui.browser.selected_visible, first_expected);
}

#[test]
fn random_history_trims_to_limit() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    let total = RANDOM_HISTORY_LIMIT + 5;
    controller.set_wav_entries_for_tests( (0..total)
        .map(|i| sample_entry(&format!("{i}.wav"), SampleTag::Neutral))
        .collect());
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    for seed in 0..total as u64 {
        controller.play_random_visible_sample_with_seed(seed);
    }

    assert_eq!(
        controller.history.random_history.entries.len(),
        RANDOM_HISTORY_LIMIT
    );
    assert_eq!(
        controller.history.random_history.cursor,
        Some(
            controller
                .history
                .random_history
                .entries
                .len()
                .saturating_sub(1)
        )
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
