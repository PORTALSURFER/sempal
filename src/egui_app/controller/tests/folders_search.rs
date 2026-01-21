use super::super::test_support::{dummy_controller, sample_entry};
use std::path::{Path, PathBuf};

#[test]
fn creating_folder_tracks_manual_entry() -> Result<(), String> {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.refresh_folder_browser_for_tests();
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
fn fuzzy_search_filters_folders() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    let kick = source.root.join("kick");
    let snare = source.root.join("snare");
    std::fs::create_dir_all(&kick).unwrap();
    std::fs::create_dir_all(&snare).unwrap();
    controller.set_wav_entries_for_tests(vec![
        sample_entry("kick/one.wav", crate::sample_sources::Rating::NEUTRAL),
        sample_entry("snare/two.wav", crate::sample_sources::Rating::NEUTRAL),
    ]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.refresh_folder_browser_for_tests();

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
