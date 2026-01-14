use super::super::test_support::{dummy_controller, sample_entry};
use crate::sample_sources::Rating;
use std::path::PathBuf;



#[test]
fn adjust_rating_skips_neutral_from_rated() {
    // Setup - we need to mock the selection
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    
    // Helper macro to setup a single file and select it
    macro_rules! setup_file {
        ($name:expr, $rating:expr) => {
            let entry = sample_entry($name, $rating);
            controller.set_wav_entries_for_tests(vec![entry]);
            controller.rebuild_wav_lookup();
            controller.rebuild_browser_lists();
            // Select the row
            controller.sample_view.wav.selected_wav = Some(PathBuf::from($name));
        }
    }

    // Helper to find row
    let find_row = |rows: &[crate::sample_sources::WavEntry], name: &str| {
        rows.iter().find(|r| r.relative_path.to_string_lossy() == name).expect("file not found").clone()
    };

    // Case 1: Keep 1 -> Decrement -> Trash 1 (skip Neutral)
    setup_file!("keep1.wav", Rating::KEEP_1);
    controller.adjust_selected_rating(-1);
    let rows = controller.database_for(&source).unwrap().list_files().unwrap();
    assert_eq!(find_row(&rows, "keep1.wav").tag, Rating::TRASH_1, "Decreasing Keep 1 should go to Trash 1");

    // Case 2: Trash 1 -> Increment -> Keep 1 (skip Neutral)
    setup_file!("trash1.wav", Rating::TRASH_1);
    controller.adjust_selected_rating(1);
    let rows = controller.database_for(&source).unwrap().list_files().unwrap();
    assert_eq!(find_row(&rows, "trash1.wav").tag, Rating::KEEP_1, "Increasing Trash 1 should go to Keep 1");

    // Case 3: Neutral -> Increment -> Keep 1 (Normal behavior)
    setup_file!("neutral_inc.wav", Rating::NEUTRAL);
    controller.adjust_selected_rating(1);
    let rows = controller.database_for(&source).unwrap().list_files().unwrap();
    assert_eq!(find_row(&rows, "neutral_inc.wav").tag, Rating::KEEP_1, "Increasing Neutral should go to Keep 1");

    // Case 4: Neutral -> Decrement -> Trash 1 (Normal behavior)
    setup_file!("neutral_dec.wav", Rating::NEUTRAL);
    controller.adjust_selected_rating(-1);
    let rows = controller.database_for(&source).unwrap().list_files().unwrap();
    assert_eq!(find_row(&rows, "neutral_dec.wav").tag, Rating::TRASH_1, "Decreasing Neutral should go to Trash 1");
}

#[test]
fn advance_after_rating_respects_random_navigation() {
    // Setup 3 files
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    let entries = vec![
        sample_entry("a.wav", Rating::NEUTRAL),
        sample_entry("b.wav", Rating::NEUTRAL),
        sample_entry("c.wav", Rating::NEUTRAL),
    ];
    controller.set_wav_entries_for_tests(entries);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    // Enable random nav
    controller.ui.browser.random_navigation_mode = true;
    controller.settings.controls.advance_after_rating = true;

    // Mark a.wav and b.wav as played so random choices are forced to c.wav
    // We need to resolve source id and path
    let id = source.id.clone();
    controller.history.random_history.mark_played(&id, &PathBuf::from("a.wav"));
    controller.history.random_history.mark_played(&id, &PathBuf::from("b.wav"));

    // Select A (index 0)
    controller.focus_browser_row(0);
    assert_eq!(controller.selected_row_index(), Some(0));

    // Rate A (which triggers advance)
    // adjust_selected_rating checks if random is enabled
    controller.adjust_selected_rating(1); // Increment rating

    // Expectation:
    // Linear advance would go to B (index 1).
    // Random advance (constrained to unvisited) should go to C (index 2).
    
    // Check selection
    let selected_path = controller.sample_view.wav.selected_wav.as_ref().expect("selection");
    assert_eq!(selected_path, &PathBuf::from("c.wav"));
}
