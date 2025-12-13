use hound::WavReader;
use super::super::test_support::{dummy_controller, sample_entry, write_test_wav};
use super::super::*;
use crate::egui_app::state::FocusContext;
use std::path::{Path, PathBuf};
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
fn delete_actions_apply_to_all_selected_rows() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.cache_db(&source).unwrap();
    write_test_wav(&source.root.join("one.wav"), &[0.0, 0.1]);
    write_test_wav(&source.root.join("two.wav"), &[0.0, 0.1]);
    write_test_wav(&source.root.join("three.wav"), &[0.0, 0.1]);
    controller.wav_entries = vec![
        sample_entry("one.wav", SampleTag::Neutral),
        sample_entry("two.wav", SampleTag::Neutral),
        sample_entry("three.wav", SampleTag::Neutral),
    ];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.focus_browser_row_only(0);
    controller.toggle_browser_row_selection(1);
    controller.toggle_browser_row_selection(2);
    let rows = controller.action_rows_from_primary(0);

    controller.delete_browser_samples(&rows).unwrap();

    assert!(controller.wav_entries.is_empty());
    assert!(!source.root.join("one.wav").exists());
    assert!(!source.root.join("two.wav").exists());
    assert!(!source.root.join("three.wav").exists());
}

#[test]
fn normalize_actions_apply_to_all_selected_rows() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.cache_db(&source).unwrap();
    write_test_wav(&source.root.join("one.wav"), &[0.0, 0.1]);
    write_test_wav(&source.root.join("two.wav"), &[0.0, 0.1]);
    controller.wav_entries = vec![
        sample_entry("one.wav", SampleTag::Neutral),
        sample_entry("two.wav", SampleTag::Neutral),
    ];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.focus_browser_row_only(0);
    controller.toggle_browser_row_selection(1);
    let rows = controller.action_rows_from_primary(0);

    controller.normalize_browser_samples(&rows).unwrap();

    assert!(controller.wav_entries.iter().all(|e| e.modified_ns > 0));
    assert!(controller.wav_entries.iter().all(|e| e.file_size > 0));
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
            true,
        )
        .unwrap();

    assert_eq!(entry.tag, SampleTag::Keep);
    assert_eq!(entry.relative_path, PathBuf::from("orig_sel.wav"));
    assert_eq!(controller.wav_entries.len(), 1);
    assert_eq!(controller.ui.browser.visible.len(), 1);
    let exported_path = root.join(&entry.relative_path);
    assert!(exported_path.exists());
    let exported: Vec<f32> = WavReader::open(&exported_path)
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
