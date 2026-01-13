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

    
