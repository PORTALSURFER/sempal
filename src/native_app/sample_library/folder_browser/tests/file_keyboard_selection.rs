use super::*;
use crate::native_app::sample_library::folder_browser::projection::VisibleSampleQuery;

#[test]
fn file_keyboard_navigation_moves_audio_selection_without_leaving_folder() {
    let root = temp_source_root("wavecrate-gui-file-keyboard");
    let drums = root.join("drums");
    fs::create_dir_all(&drums).expect("create drums folder");
    let hat = drums.join("hat.wav");
    let kick = drums.join("kick.wav");
    let snare = drums.join("snare.wav");
    fs::write(&hat, [0_u8; 8]).expect("write hat");
    fs::write(&kick, [0_u8; 8]).expect("write kick");
    fs::write(&snare, [0_u8; 8]).expect("write snare");
    let mut browser = FolderBrowserState::from_root(root.clone());
    browser.activate_folder(path_id(&drums));
    browser.select_file(path_id(&hat));

    assert_eq!(browser.navigate_vertical(1, false), Some(path_id(&kick)));
    browser.select_file(path_id(&kick));
    assert_eq!(browser.navigate_vertical(1, false), Some(path_id(&snare)));
    browser.select_file(path_id(&snare));
    assert_eq!(browser.navigate_vertical(1, false), None);
    assert_eq!(browser.selected_file_id(), Some(path_id(&snare).as_str()));
    assert_eq!(browser.navigate_vertical(-1, false), Some(path_id(&kick)));
    assert_eq!(browser.selection.selected_folder, path_id(&drums));

    let _ = fs::remove_dir_all(root);
}
#[test]
fn file_keyboard_navigation_can_extend_audio_selection() {
    let root = temp_source_root("wavecrate-gui-file-keyboard-extend");
    let drums = root.join("drums");
    fs::create_dir_all(&drums).expect("create drums folder");
    let hat = drums.join("hat.wav");
    let kick = drums.join("kick.wav");
    let snare = drums.join("snare.wav");
    fs::write(&hat, [0_u8; 8]).expect("write hat");
    fs::write(&kick, [0_u8; 8]).expect("write kick");
    fs::write(&snare, [0_u8; 8]).expect("write snare");
    let mut browser = FolderBrowserState::from_root(root.clone());
    browser.activate_folder(path_id(&drums));
    browser.select_file(path_id(&hat));

    assert_eq!(browser.navigate_vertical(1, true), Some(path_id(&kick)));
    assert_eq!(browser.navigate_vertical(1, true), Some(path_id(&snare)));

    assert_eq!(
        browser.selected_file_paths(),
        vec![hat.clone(), kick.clone(), snare.clone()]
    );
    assert_eq!(browser.selected_file_id(), Some(path_id(&snare).as_str()));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn file_keyboard_navigation_clears_shift_range_after_shift_release() {
    let root = temp_source_root("wavecrate-gui-file-keyboard-release-shift");
    let drums = root.join("drums");
    fs::create_dir_all(&drums).expect("create drums folder");
    let hat = drums.join("hat.wav");
    let kick = drums.join("kick.wav");
    let snare = drums.join("snare.wav");
    let tom = drums.join("tom.wav");
    for file in [&hat, &kick, &snare, &tom] {
        fs::write(file, [0_u8; 8]).expect("write wav");
    }
    let mut browser = FolderBrowserState::from_root(root.clone());
    browser.activate_folder(path_id(&drums));
    browser.select_file(path_id(&hat));

    let tags_by_file = std::collections::HashMap::new();
    assert_eq!(
        browser.navigate_vertical_matching_tags(1, true, false, &tags_by_file),
        Some(path_id(&kick))
    );
    assert_eq!(
        browser.navigate_vertical_matching_tags(1, true, false, &tags_by_file),
        Some(path_id(&snare))
    );

    assert_eq!(
        browser.navigate_vertical_matching_tags(1, false, false, &tags_by_file),
        Some(path_id(&tom))
    );
    assert_eq!(browser.selected_file_paths(), vec![tom.clone()]);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn toggle_focused_sample_selection_marks_without_advancing() {
    let root = temp_source_root("wavecrate-gui-toggle-mark-stationary");
    let drums = root.join("drums");
    fs::create_dir_all(&drums).expect("create drums folder");
    let hat = drums.join("hat.wav");
    let kick = drums.join("kick.wav");
    let snare = drums.join("snare.wav");
    for file in [&hat, &kick, &snare] {
        fs::write(file, [0_u8; 8]).expect("write wav");
    }
    let mut browser = FolderBrowserState::from_root(root.clone());
    browser.activate_folder(path_id(&drums));
    browser.select_file(path_id(&hat));

    let result = browser
        .toggle_focused_sample_selection(&Default::default())
        .expect("toggle focused sample");

    assert_eq!(result.toggled_id, path_id(&hat));
    assert!(result.toggled_selected);
    assert_eq!(result.focused_id, path_id(&hat));
    assert_eq!(browser.selected_file_id(), Some(path_id(&hat).as_str()));
    assert_eq!(browser.selected_file_paths(), vec![hat.clone()]);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn toggle_focused_sample_selection_marks_visible_row_as_explicit_selection() {
    let root = temp_source_root("wavecrate-gui-toggle-mark-visible-explicit");
    let drums = root.join("drums");
    fs::create_dir_all(&drums).expect("create drums folder");
    let hat = drums.join("hat.wav");
    let kick = drums.join("kick.wav");
    for file in [&hat, &kick] {
        fs::write(file, [0_u8; 8]).expect("write wav");
    }
    let tags_by_file = std::collections::HashMap::new();
    let cached_sample_paths = std::collections::HashSet::new();
    let mut browser = FolderBrowserState::from_root(root.clone());
    browser.activate_folder(path_id(&drums));
    browser.apply_file_view_window_change(radiant::prelude::VirtualListWindowChange {
        offset_y: 0.0,
        row_height: 22.0,
        window: radiant::prelude::VirtualListWindow {
            total_items: 2,
            viewport_start: 0,
            viewport_end: 2,
            window_start: 0,
            window_end: 2,
        },
    });
    browser.select_file(path_id(&hat));

    let before = browser.visible_samples(VisibleSampleQuery {
        tags_by_file: &tags_by_file,
        cached_sample_paths: &cached_sample_paths,
    });
    let focused_before = before
        .rows
        .iter()
        .find(|row| row.file.id == path_id(&hat))
        .expect("focused row before toggle");

    assert_eq!(browser.selected_file_paths(), vec![hat.clone()]);
    assert!(
        focused_before.focused,
        "pointer selection should paint transient focus while retaining navigation ownership"
    );
    assert!(focused_before.focus_alpha > 0);
    assert!(
        focused_before.selected,
        "pointer selection should keep persistent selected-row chrome after release"
    );
    assert!(!browser.selected_file_ids_explicit_for_diagnostics());

    browser
        .toggle_focused_sample_selection(&Default::default())
        .expect("toggle focused sample");

    let after = browser.visible_samples(VisibleSampleQuery {
        tags_by_file: &tags_by_file,
        cached_sample_paths: &cached_sample_paths,
    });
    let focused_after = after
        .rows
        .iter()
        .find(|row| row.file.id == path_id(&hat))
        .expect("focused row after toggle");

    assert_eq!(browser.selected_file_paths(), vec![hat.clone()]);
    assert!(focused_after.focused);
    assert!(browser.selected_file_ids_explicit_for_diagnostics());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn repeated_toggle_focused_sample_selection_toggles_same_row() {
    let root = temp_source_root("wavecrate-gui-toggle-repeat-stationary");
    let drums = root.join("drums");
    fs::create_dir_all(&drums).expect("create drums folder");
    let hat = drums.join("hat.wav");
    let kick = drums.join("kick.wav");
    for file in [&hat, &kick] {
        fs::write(file, [0_u8; 8]).expect("write wav");
    }
    let mut browser = FolderBrowserState::from_root(root.clone());
    browser.activate_folder(path_id(&drums));
    browser.select_file(path_id(&hat));

    let first = browser
        .toggle_focused_sample_selection(&Default::default())
        .expect("select focused sample");
    let second = browser
        .toggle_focused_sample_selection(&Default::default())
        .expect("deselect focused sample");

    assert_eq!(first.toggled_id, path_id(&hat));
    assert!(first.toggled_selected);
    assert_eq!(second.toggled_id, path_id(&hat));
    assert!(!second.toggled_selected);
    assert_eq!(browser.selected_file_id(), Some(path_id(&hat).as_str()));
    assert!(browser.selected_file_paths().is_empty());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn file_keyboard_navigation_preserves_toggle_marked_samples() {
    let root = temp_source_root("wavecrate-gui-toggle-mark-arrow-preserve");
    let drums = root.join("drums");
    fs::create_dir_all(&drums).expect("create drums folder");
    let hat = drums.join("hat.wav");
    let kick = drums.join("kick.wav");
    let snare = drums.join("snare.wav");
    let tom = drums.join("tom.wav");
    for file in [&hat, &kick, &snare, &tom] {
        fs::write(file, [0_u8; 8]).expect("write wav");
    }
    let mut browser = FolderBrowserState::from_root(root.clone());
    browser.activate_folder(path_id(&drums));
    browser.select_file(path_id(&hat));

    browser
        .toggle_focused_sample_selection(&Default::default())
        .expect("mark first sample");
    assert_eq!(browser.selected_file_id(), Some(path_id(&hat).as_str()));

    let tags_by_file = std::collections::HashMap::new();
    assert_eq!(
        browser.navigate_vertical_matching_tags(1, false, false, &tags_by_file),
        Some(path_id(&kick))
    );
    assert_eq!(browser.selected_file_paths(), vec![hat.clone()]);

    browser
        .toggle_focused_sample_selection(&Default::default())
        .expect("mark second sample");

    assert_eq!(
        browser.selected_file_paths(),
        vec![hat.clone(), kick.clone()]
    );
    assert_eq!(browser.selected_file_id(), Some(path_id(&kick).as_str()));

    assert_eq!(
        browser.navigate_vertical_matching_tags(1, false, false, &tags_by_file),
        Some(path_id(&snare))
    );
    assert_eq!(
        browser.selected_file_paths(),
        vec![hat.clone(), kick.clone()]
    );
    assert_eq!(browser.selected_file_id(), Some(path_id(&snare).as_str()));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn collection_keyboard_navigation_reuses_cached_id_projection() {
    let root = temp_source_root("wavecrate-gui-collection-keyboard-cache");
    let first = root.join("a_first.wav");
    let second = root.join("b_second.wav");
    let third = root.join("c_third.wav");
    for file in [&first, &second, &third] {
        fs::write(file, [0_u8; 8]).expect("write wav");
    }
    let collection = SampleCollection::new(0).expect("collection");
    let mut browser = FolderBrowserState::from_root(root.clone());
    for file in [&first, &second, &third] {
        browser.set_file_collection_state(file, collection);
    }
    browser.apply_message(FolderBrowserMessage::ActivateCollection(collection));
    browser.selection.set_focus_file_set(path_id(&first));

    let before_navigation = browser.selected_audio_projection_cache_len_for_tests();
    assert_eq!(
        browser.navigate_vertical_matching_tags(1, false, false, &Default::default()),
        Some(path_id(&second))
    );
    let after_first_navigation = browser.selected_audio_projection_cache_len_for_tests();
    assert!(
        after_first_navigation > before_navigation,
        "first collection navigation should populate the ordered id projection"
    );

    assert_eq!(
        browser.navigate_vertical_matching_tags(1, false, false, &Default::default()),
        Some(path_id(&third))
    );
    assert_eq!(
        browser.selected_audio_projection_cache_len_for_tests(),
        after_first_navigation,
        "subsequent collection navigation should reuse the cached id projection"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn folder_keyboard_navigation_uses_fast_index_when_tags_are_metadata_only() {
    let root = temp_source_root("wavecrate-gui-folder-keyboard-fast-index");
    let drums = root.join("drums");
    fs::create_dir_all(&drums).expect("create drums folder");
    let hat = drums.join("a_hat.wav");
    let kick = drums.join("b_kick.wav");
    let snare = drums.join("c_snare.wav");
    for file in [&hat, &kick, &snare] {
        fs::write(file, [0_u8; 8]).expect("write wav");
    }
    let mut browser = FolderBrowserState::from_root(root.clone());
    browser.activate_folder(path_id(&drums));
    browser.select_file(path_id(&hat));
    let tags_by_file = std::collections::HashMap::from([(
        path_id(&kick),
        vec![String::from("drum"), String::from("transient")],
    )]);

    assert_eq!(
        browser.selected_audio_file_index_fast_matching_tags(&tags_by_file),
        Some(Some(0)),
        "metadata tags alone should not force keyboard navigation through a filtered snapshot"
    );
    let before_navigation = browser.selected_audio_projection_cache_len_for_tests();
    assert_eq!(
        browser.navigate_vertical_matching_tags(1, false, false, &tags_by_file),
        Some(path_id(&kick))
    );
    assert_eq!(
        browser.selected_audio_file_index_fast_matching_tags(&tags_by_file),
        Some(Some(1))
    );
    assert_eq!(
        browser.selected_audio_projection_cache_len_for_tests(),
        before_navigation,
        "plain folder navigation should reuse the existing audio-index projection"
    );
    assert_eq!(
        browser.navigate_vertical_matching_tags(1, false, false, &tags_by_file),
        Some(path_id(&snare))
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn toggle_focused_sample_selection_unmarks_without_advancing() {
    let root = temp_source_root("wavecrate-gui-toggle-unmark-stationary");
    let drums = root.join("drums");
    fs::create_dir_all(&drums).expect("create drums folder");
    let hat = drums.join("hat.wav");
    let kick = drums.join("kick.wav");
    let snare = drums.join("snare.wav");
    for file in [&hat, &kick, &snare] {
        fs::write(file, [0_u8; 8]).expect("write wav");
    }
    let mut browser = FolderBrowserState::from_root(root.clone());
    browser.activate_folder(path_id(&drums));
    browser.select_file(path_id(&hat));
    browser
        .toggle_focused_sample_selection(&Default::default())
        .expect("mark first sample");
    browser.select_file_with_modifiers(
        path_id(&kick),
        PointerModifiers {
            command: true,
            ..Default::default()
        },
    );

    let result = browser
        .toggle_focused_sample_selection(&Default::default())
        .expect("toggle focused sample");

    assert_eq!(result.toggled_id, path_id(&kick));
    assert!(!result.toggled_selected);
    assert_eq!(result.focused_id, path_id(&kick));
    assert_eq!(browser.selected_file_id(), Some(path_id(&kick).as_str()));
    assert_eq!(browser.selected_file_paths(), vec![hat.clone()]);

    let _ = fs::remove_dir_all(root);
}
#[test]
fn toggle_focused_sample_selection_stays_on_last_visible_sample() {
    let root = temp_source_root("wavecrate-gui-toggle-mark-last");
    let drums = root.join("drums");
    fs::create_dir_all(&drums).expect("create drums folder");
    let hat = drums.join("hat.wav");
    let kick = drums.join("kick.wav");
    for file in [&hat, &kick] {
        fs::write(file, [0_u8; 8]).expect("write wav");
    }
    let mut browser = FolderBrowserState::from_root(root.clone());
    browser.activate_folder(path_id(&drums));
    browser.select_file(path_id(&kick));

    let result = browser
        .toggle_focused_sample_selection(&Default::default())
        .expect("toggle focused sample");

    assert_eq!(result.toggled_id, path_id(&kick));
    assert!(result.toggled_selected);
    assert_eq!(browser.selected_file_id(), Some(path_id(&kick).as_str()));
    assert_eq!(browser.selected_file_paths(), vec![kick.clone()]);

    let _ = fs::remove_dir_all(root);
}
#[test]
fn toggle_focused_sample_selection_stays_put_through_tag_filtered_rows() {
    let root = temp_source_root("wavecrate-gui-toggle-mark-filtered-stationary");
    let drums = root.join("drums");
    fs::create_dir_all(&drums).expect("create drums folder");
    let hat = drums.join("hat.wav");
    let kick = drums.join("kick.wav");
    let snare = drums.join("snare.wav");
    for file in [&hat, &kick, &snare] {
        fs::write(file, [0_u8; 8]).expect("write wav");
    }
    let tags_by_file = std::collections::HashMap::from([
        (path_id(&hat), vec![String::from("drum")]),
        (path_id(&kick), vec![String::from("ignore")]),
        (path_id(&snare), vec![String::from("drum")]),
    ]);
    let mut browser = FolderBrowserState::from_root(root.clone());
    browser.activate_folder(path_id(&drums));
    browser.apply_message(FolderBrowserMessage::TagFilterInput(
        TextInputMessage::Changed {
            value: String::from("drum"),
        },
    ));
    browser.select_file(path_id(&hat));

    let result = browser
        .toggle_focused_sample_selection(&tags_by_file)
        .expect("toggle focused sample");

    assert_eq!(result.toggled_id, path_id(&hat));
    assert_eq!(result.focused_id, path_id(&hat));
    assert_eq!(browser.selected_file_id(), Some(path_id(&hat).as_str()));
    assert_eq!(browser.selected_file_paths(), vec![hat.clone()]);

    let _ = fs::remove_dir_all(root);
}
