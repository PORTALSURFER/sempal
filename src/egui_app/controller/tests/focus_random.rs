use super::super::test_support::{dummy_controller, sample_entry, write_test_wav};
use super::super::*;
use super::common::{prepare_browser_sample, visible_indices};
use crate::egui_app::controller::ui::hotkeys;
use crate::egui_app::state::FocusContext;
use crate::egui_app::state::SampleBrowserTab;
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
    assert!(action.is_global());
    assert_eq!(action.gesture.first.key, Key::F);
    assert!(action.gesture.first.shift);
    assert!(!action.gesture.first.command);
    assert!(!action.gesture.first.alt);
    assert!(action.gesture.chord.is_none());
}

#[test]
fn find_similar_from_map_switches_to_browser_list() {
    let (mut controller, source) = dummy_controller();
    prepare_browser_sample(&mut controller, &source, "map.wav");
    controller.focus_browser_row(0);
    controller.ui.browser.active_tab = SampleBrowserTab::Map;
    controller.ui.browser.similar_query = Some(crate::egui_app::state::SimilarQuery {
        sample_id: "test::map.wav".to_string(),
        label: "map.wav".to_string(),
        indices: vec![0],
        scores: vec![1.0],
        anchor_index: Some(0),
    });
    let action = hotkeys::iter_actions()
        .find(|a| a.id == "find-similar")
        .expect("find-similar hotkey");

    controller.handle_hotkey(action, FocusContext::SampleBrowser);

    assert_eq!(controller.ui.browser.active_tab, SampleBrowserTab::List);
    assert!(controller.ui.browser.similar_query.is_none());
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
fn focus_history_hotkeys_are_registered() {
    let previous = hotkeys::iter_actions()
        .find(|a| a.id == "focus-history-previous")
        .expect("focus-history-previous hotkey");
    assert_eq!(previous.label, "Previous focused sample");
    assert_eq!(previous.gesture.first.key, Key::ArrowLeft);
    assert!(!previous.gesture.first.shift);
    assert!(!previous.gesture.first.command);
    assert!(!previous.gesture.first.alt);
    assert!(previous.gesture.chord.is_none());

    let next = hotkeys::iter_actions()
        .find(|a| a.id == "focus-history-next")
        .expect("focus-history-next hotkey");
    assert_eq!(next.label, "Next focused sample");
    assert_eq!(next.gesture.first.key, Key::ArrowRight);
    assert!(!next.gesture.first.shift);
    assert!(!next.gesture.first.command);
    assert!(!next.gesture.first.alt);
    assert!(next.gesture.chord.is_none());
}

#[test]
fn focus_history_steps_backward_and_forward() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.cache_db(&source).unwrap();
    write_test_wav(&source.root.join("a.wav"), &[0.0, 0.1]);
    write_test_wav(&source.root.join("b.wav"), &[0.0, 0.1]);
    write_test_wav(&source.root.join("c.wav"), &[0.0, 0.1]);
    controller.set_wav_entries_for_tests(vec![
        sample_entry("a.wav", crate::sample_sources::Rating::NEUTRAL),
        sample_entry("b.wav", crate::sample_sources::Rating::NEUTRAL),
        sample_entry("c.wav", crate::sample_sources::Rating::NEUTRAL),
    ]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.focus_browser_row(0);
    controller.focus_browser_row(1);
    controller.focus_browser_row(2);
    assert_eq!(
        controller.sample_view.wav.selected_wav.as_deref(),
        Some(Path::new("c.wav"))
    );

    controller.focus_previous_sample_history();
    assert_eq!(
        controller.sample_view.wav.selected_wav.as_deref(),
        Some(Path::new("b.wav"))
    );

    controller.focus_previous_sample_history();
    assert_eq!(
        controller.sample_view.wav.selected_wav.as_deref(),
        Some(Path::new("a.wav"))
    );

    controller.focus_next_sample_history();
    assert_eq!(
        controller.sample_view.wav.selected_wav.as_deref(),
        Some(Path::new("b.wav"))
    );
}

#[test]
fn random_sample_selection_uses_seeded_rng() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.set_wav_entries_for_tests(vec![
        sample_entry("one.wav", crate::sample_sources::Rating::NEUTRAL),
        sample_entry("two.wav", crate::sample_sources::Rating::NEUTRAL),
        sample_entry("three.wav", crate::sample_sources::Rating::NEUTRAL),
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
    controller.wav_entries.entry_mut(0).unwrap().tag = crate::sample_sources::Rating::KEEP_1;
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.focus_browser_row(0);

    let action = hotkeys::iter_actions()
        .find(|a| a.id == "tag-neutral")
        .expect("tag-neutral hotkey");
    controller.handle_hotkey(action, FocusContext::None);

    assert_eq!(controller.wav_entry(0).unwrap().tag, crate::sample_sources::Rating::NEUTRAL);
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
    db.set_tag(Path::new("trash.wav"), crate::sample_sources::Rating::TRASH_3)
        .map_err(|err| format!("tag: {err}"))?;

    controller.set_wav_entries_for_tests(vec![sample_entry("trash.wav", crate::sample_sources::Rating::TRASH_3)]);
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
    controller.set_wav_entries_for_tests(vec![
        sample_entry("one.wav", crate::sample_sources::Rating::NEUTRAL),
        sample_entry("two.wav", crate::sample_sources::Rating::NEUTRAL),
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
    controller.set_wav_entries_for_tests(
        (0..total)
            .map(|i| sample_entry(&format!("{i}.wav"), crate::sample_sources::Rating::NEUTRAL))
            .collect(),
    );
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

#[test]
fn random_sample_navigation_avoids_repeats() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    controller.set_wav_entries_for_tests(vec![
        sample_entry("one.wav", crate::sample_sources::Rating::NEUTRAL),
        sample_entry("two.wav", crate::sample_sources::Rating::NEUTRAL),
        sample_entry("three.wav", crate::sample_sources::Rating::NEUTRAL),
    ]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    let mut played = std::collections::HashSet::new();

    // Play all 3 samples randomly
    for _ in 0..3 {
        controller.play_random_visible_sample();
        let selected = controller.ui.browser.selected_visible.expect("sample selected");
        let path = controller
            .visible_browser_index(selected)
            .and_then(|idx| controller.wav_entry(idx))
            .map(|e| e.relative_path.clone())
            .expect("path");
        assert!(played.insert(path), "Sample should not be repeated");
    }

    assert_eq!(played.len(), 3, "All samples should have been played");

    // The next one should be a repeat (since all were played)
    controller.play_random_visible_sample();
    let selected = controller.ui.browser.selected_visible.expect("sample selected");
    let path = controller
        .visible_browser_index(selected)
        .and_then(|idx| controller.wav_entry(idx))
        .map(|e| e.relative_path.clone())
        .expect("path");
    assert!(played.contains(&path), "Should repeat after all were played");
}
