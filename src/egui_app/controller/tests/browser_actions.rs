use super::super::test_support::{
    dummy_controller, load_waveform_selection, prepare_with_source_and_wav_entries,
    sample_entry, write_test_wav,
};
use crate::sample_sources::Rating;
use super::super::*;
use super::common::{max_sample_amplitude, visible_indices};
use crate::egui_app::controller::library::collection_export;
use crate::egui_app::controller::ui::hotkeys;
use crate::egui_app::state::FocusContext;
use crate::sample_sources::Collection;
use crate::sample_sources::collections::CollectionMember;
use hound::WavReader;
use std::cell::RefCell;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use tempfile::tempdir;

#[test]
fn hotkey_tagging_applies_to_all_selected_rows() {
    let (mut controller, _source) = prepare_with_source_and_wav_entries(vec![
        sample_entry("one.wav", crate::sample_sources::Rating::NEUTRAL),
        sample_entry("two.wav", crate::sample_sources::Rating::NEUTRAL),
    ]);

    controller.focus_browser_row_only(0);
    controller.toggle_browser_row_selection(1);
    controller.tag_selected_left();

    assert_eq!(controller.wav_entry(0).unwrap().tag, crate::sample_sources::Rating::TRASH_3);
    assert_eq!(controller.wav_entry(1).unwrap().tag, crate::sample_sources::Rating::TRASH_3);
}

#[test]
fn focus_hotkey_does_not_autoplay_browser_sample() {
    let (mut controller, source) =
        prepare_with_source_and_wav_entries(vec![sample_entry("one.wav", crate::sample_sources::Rating::NEUTRAL)]);
    write_test_wav(&source.root.join("one.wav"), &[0.0, 0.1]);

    assert!(controller.settings.feature_flags.autoplay_selection);

    controller.focus_browser_list();

    assert_eq!(controller.ui.focus.context, FocusContext::SampleBrowser);
    assert_eq!(
        controller.sample_view.wav.selected_wav.as_deref(),
        Some(Path::new("one.wav"))
    );
    assert!(controller.runtime.jobs.pending_playback.is_none());
    assert_eq!(controller.ui.browser.selected_visible, Some(0));
}

#[test]
fn x_key_toggle_respects_focus() {
    let (mut controller, _source) = prepare_with_source_and_wav_entries(vec![
        sample_entry("one.wav", crate::sample_sources::Rating::NEUTRAL),
        sample_entry("two.wav", crate::sample_sources::Rating::NEUTRAL),
    ]);

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
    let (mut controller, _source) = prepare_with_source_and_wav_entries(vec![
        sample_entry("one.wav", crate::sample_sources::Rating::NEUTRAL),
        sample_entry("two.wav", crate::sample_sources::Rating::NEUTRAL),
        sample_entry("three.wav", crate::sample_sources::Rating::NEUTRAL),
    ]);
    controller.ui.browser.selected_paths =
        vec![PathBuf::from("one.wav"), PathBuf::from("three.wav")];

    let rows = controller.action_rows_from_primary(1);

    assert_eq!(rows, vec![0, 1, 2]);
}

#[test]
fn tag_actions_apply_to_all_selected_rows() {
    let (mut controller, _source) = prepare_with_source_and_wav_entries(vec![
        sample_entry("one.wav", crate::sample_sources::Rating::NEUTRAL),
        sample_entry("two.wav", crate::sample_sources::Rating::NEUTRAL),
    ]);

    controller.focus_browser_row(0);
    controller.toggle_browser_row_selection(1);
    let rows = controller.action_rows_from_primary(0);

    controller
        .tag_browser_samples(&rows, crate::sample_sources::Rating::KEEP_1, 0)
        .unwrap();

    assert_eq!(controller.wav_entry(0).unwrap().tag, crate::sample_sources::Rating::KEEP_1);
    assert_eq!(controller.wav_entry(1).unwrap().tag, crate::sample_sources::Rating::KEEP_1);
}

#[test]
fn delete_actions_apply_to_all_selected_rows() {
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![
        sample_entry("one.wav", crate::sample_sources::Rating::NEUTRAL),
        sample_entry("two.wav", crate::sample_sources::Rating::NEUTRAL),
        sample_entry("three.wav", crate::sample_sources::Rating::NEUTRAL),
    ]);
    write_test_wav(&source.root.join("one.wav"), &[0.0, 0.1]);
    write_test_wav(&source.root.join("two.wav"), &[0.0, 0.1]);
    write_test_wav(&source.root.join("three.wav"), &[0.0, 0.1]);

    controller.focus_browser_row_only(0);
    controller.toggle_browser_row_selection(1);
    controller.toggle_browser_row_selection(2);
    let rows = controller.action_rows_from_primary(0);

    controller.delete_browser_samples(&rows).unwrap();

    assert_eq!(controller.wav_entries_len(), 0);
    assert!(!source.root.join("one.wav").exists());
    assert!(!source.root.join("two.wav").exists());
    assert!(!source.root.join("three.wav").exists());
}

#[test]
fn delete_hotkey_applies_to_all_selected_rows() {
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![
        sample_entry("one.wav", crate::sample_sources::Rating::NEUTRAL),
        sample_entry("two.wav", crate::sample_sources::Rating::NEUTRAL),
        sample_entry("three.wav", crate::sample_sources::Rating::NEUTRAL),
    ]);
    write_test_wav(&source.root.join("one.wav"), &[0.0, 0.1]);
    write_test_wav(&source.root.join("two.wav"), &[0.0, 0.1]);
    write_test_wav(&source.root.join("three.wav"), &[0.0, 0.1]);

    controller.focus_browser_row_only(0);
    controller.toggle_browser_row_selection(1);
    controller.toggle_browser_row_selection(2);
    let action = hotkeys::iter_actions()
        .find(|a| a.id == "delete-browser")
        .expect("delete-browser hotkey");

    controller.handle_hotkey(action, FocusContext::SampleBrowser);

    assert_eq!(controller.wav_entries_len(), 0);
    assert!(!source.root.join("one.wav").exists());
    assert!(!source.root.join("two.wav").exists());
    assert!(!source.root.join("three.wav").exists());
}

#[test]
fn normalize_actions_apply_to_all_selected_rows() {
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![
        sample_entry("one.wav", crate::sample_sources::Rating::NEUTRAL),
        sample_entry("two.wav", crate::sample_sources::Rating::NEUTRAL),
    ]);
    write_test_wav(&source.root.join("one.wav"), &[0.0, 0.1]);
    write_test_wav(&source.root.join("two.wav"), &[0.0, 0.1]);

    controller.focus_browser_row_only(0);
    controller.toggle_browser_row_selection(1);
    let rows = controller.action_rows_from_primary(0);

    controller.normalize_browser_samples(&rows).unwrap();

    let entries = controller.wav_entries.pages.get(&0).expect("entries");
    assert!(entries.iter().all(|e| e.modified_ns > 0));
    assert!(entries.iter().all(|e| e.file_size > 0));
}

#[test]
fn selection_persists_when_nudging_focus() {
    let (mut controller, _source) = prepare_with_source_and_wav_entries(vec![
        sample_entry("one.wav", crate::sample_sources::Rating::NEUTRAL),
        sample_entry("two.wav", crate::sample_sources::Rating::NEUTRAL),
        sample_entry("three.wav", crate::sample_sources::Rating::NEUTRAL),
    ]);

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
    let (mut controller, _source) = prepare_with_source_and_wav_entries(vec![
        sample_entry("one.wav", crate::sample_sources::Rating::NEUTRAL),
        sample_entry("two.wav", crate::sample_sources::Rating::NEUTRAL),
    ]);

    controller.settings.controls.advance_after_rating = false;
    controller.nudge_selection(0);
    assert!(controller.ui.browser.selected_paths.is_empty());

    controller.tag_selected_left();

    assert_eq!(controller.wav_entry(0).unwrap().tag, crate::sample_sources::Rating::TRASH_3);
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
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());

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
            Some(crate::sample_sources::Rating::KEEP_1),
            true,
            true,
        )
        .unwrap();

    assert_eq!(entry.tag, crate::sample_sources::Rating::KEEP_1);
    assert_eq!(entry.relative_path, PathBuf::from("orig_sel.wav"));
    assert_eq!(controller.wav_entries_len(), 1);
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
    assert_eq!(saved.tag, crate::sample_sources::Rating::KEEP_1);
}

#[test]
fn browser_normalize_refreshes_exports() -> Result<(), String> {
    let temp = tempdir().unwrap();
    let root = temp.path().join("source");
    let export_root = temp.path().join("export");
    std::fs::create_dir_all(&root).unwrap();
    let renderer = WaveformRenderer::new(16, 16);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    controller.library.sources.push(source.clone());

    write_test_wav(&root.join("one.wav"), &[0.25, -0.5]);
    controller.set_wav_entries_for_tests(vec![sample_entry("one.wav", crate::sample_sources::Rating::NEUTRAL)]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller
        .load_waveform_for_selection(&source, Path::new("one.wav"))
        .unwrap();

    let mut collection = Collection::new("Export");
    let collection_id = collection.id.clone();
    let manual_dir = export_root.join("Delete");
    std::fs::create_dir_all(&manual_dir).unwrap();
    collection.export_path = Some(manual_dir.clone());
    collection.add_member(source.id.clone(), PathBuf::from("one.wav"));
    controller.library.collections.push(collection);

    let member = CollectionMember {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("one.wav"),
        clip_root: None,
    };
    controller.export_member_if_needed(&collection_id, &member)?;
    controller.normalize_browser_sample(0)?;

    let collection = controller
        .library
        .collections
        .iter()
        .find(|c| c.id == collection_id)
        .unwrap();
    let export_dir = collection_export::export_dir_for(collection, None)?;
    let exported_path = export_dir.join("one.wav");
    assert!(exported_path.is_file());
    assert!((max_sample_amplitude(&root.join("one.wav")) - 1.0).abs() < 1e-6);
    assert!((max_sample_amplitude(&exported_path) - 1.0).abs() < 1e-6);
    let loaded = controller
        .sample_view
        .wav
        .loaded_audio
        .as_ref()
        .expect("loaded audio");
    let max_loaded = WavReader::new(Cursor::new(loaded.bytes.as_slice()))
        .unwrap()
        .samples::<f32>()
        .map(|s| s.unwrap().abs())
        .fold(0.0, f32::max);
    assert!((max_loaded - 1.0).abs() < 1e-6);
    Ok(())
}

#[test]
fn browser_normalize_resumes_playback_when_playing() {
    let Some(player) = crate::audio::AudioPlayer::playing_for_tests() else {
        return;
    };
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "normalize_resume_browser.wav",
        crate::sample_sources::Rating::NEUTRAL,
    )]);
    controller.audio.player = Some(Rc::new(RefCell::new(player)));
    load_waveform_selection(
        &mut controller,
        &source,
        "normalize_resume_browser.wav",
        &[0.0, 0.2, -0.6, 0.3],
        SelectionRange::new(0.0, 1.0),
    );
    if controller.play_audio(false, None).is_err() || !controller.is_playing() {
        return;
    }
    controller.ui.waveform.playhead.position = 0.5;

    assert!(controller.normalize_browser_sample(0).is_ok());

    assert!(controller.is_playing());
    assert!((controller.ui.waveform.playhead.position - 0.5).abs() < 1e-6);
}

#[test]
fn browser_delete_prunes_collections_and_exports() -> Result<(), String> {
    let temp = tempdir().unwrap();
    let root = temp.path().join("source");
    let export_root = temp.path().join("export");
    std::fs::create_dir_all(&root).unwrap();
    let renderer = WaveformRenderer::new(10, 10);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    controller.library.sources.push(source.clone());

    write_test_wav(&root.join("delete.wav"), &[0.1, 0.2]);
    controller.set_wav_entries_for_tests(vec![sample_entry("delete.wav", crate::sample_sources::Rating::NEUTRAL)]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    let mut collection = Collection::new("Delete");
    let collection_id = collection.id.clone();
    let manual_dir = export_root.join("Delete");
    std::fs::create_dir_all(&manual_dir).unwrap();
    collection.export_path = Some(manual_dir.clone());
    collection.add_member(source.id.clone(), PathBuf::from("delete.wav"));
    controller.library.collections.push(collection);

    let member = CollectionMember {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("delete.wav"),
        clip_root: None,
    };
    controller.export_member_if_needed(&collection_id, &member)?;

    let export_path = manual_dir.join("delete.wav");
    assert!(export_path.is_file());
    controller.delete_browser_sample(0)?;
    assert!(!root.join("delete.wav").exists());
    assert!(!export_path.exists());
    assert!(
        controller
            .library
            .collections
            .iter()
            .find(|c| c.id == collection_id)
            .unwrap()
            .members
            .is_empty()
    );
    Ok(())
}

#[test]
fn browser_remove_dead_links_prunes_missing_rows() -> Result<(), String> {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());

    write_test_wav(&source.root.join("alive.wav"), &[0.0, 0.1, -0.1]);
    let mut dead = sample_entry("gone.wav", crate::sample_sources::Rating::NEUTRAL);
    dead.missing = true;
    controller.set_wav_entries_for_tests(vec![sample_entry("alive.wav", crate::sample_sources::Rating::NEUTRAL), dead]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    let visible = visible_indices(&controller);
    let missing_row = visible
        .iter()
        .enumerate()
        .find_map(|(row, &idx)| {
            controller
                .wav_entry(idx)
                .filter(|entry| entry.relative_path == std::path::PathBuf::from("gone.wav"))
                .map(|_| row)
        })
        .expect("missing row present");

    controller.remove_dead_link_browser_samples(&[missing_row])?;

    assert_eq!(controller.visible_browser_len(), 1);
    let remaining_idx = visible_indices(&controller)[0];
    let remaining = controller
        .wav_entry(remaining_idx)
        .expect("remaining entry");
    assert_eq!(
        remaining.relative_path,
        std::path::PathBuf::from("alive.wav")
    );
    assert!(!controller.sample_missing(&source.id, std::path::Path::new("alive.wav")));
    assert!(
        controller
            .wav_index_for_path(std::path::Path::new("gone.wav"))
            .is_none()
    );
    Ok(())
}

#[test]
fn removing_dead_links_for_source_prunes_missing_entries() -> Result<(), String> {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.cache_db(&source).unwrap();

    write_test_wav(&source.root.join("alive.wav"), &[0.0, 0.1, -0.1]);
    let mut dead = sample_entry("gone.wav", crate::sample_sources::Rating::NEUTRAL);
    dead.missing = true;
    controller.set_wav_entries_for_tests(vec![sample_entry("alive.wav", crate::sample_sources::Rating::NEUTRAL), dead]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    let mut missing = std::collections::HashSet::new();
    missing.insert(PathBuf::from("gone.wav"));
    controller
        .library
        .missing
        .wavs
        .insert(source.id.clone(), missing);

    let removed = controller.remove_dead_links_for_source_entries(&source)?;

    assert_eq!(removed, 1);
    assert_eq!(controller.wav_entries_len(), 1);
    assert!(
        controller
            .wav_entries
            .lookup
            .contains_key(Path::new("alive.wav"))
    );
    assert!(
        !controller
            .wav_entries
            .lookup
            .contains_key(Path::new("gone.wav"))
    );
    Ok(())
}

#[test]
fn deleting_browser_sample_moves_focus_forward() -> Result<(), String> {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    for name in ["a.wav", "b.wav", "c.wav"] {
        write_test_wav(&source.root.join(name), &[0.1, -0.1]);
    }
    controller.set_wav_entries_for_tests(vec![
        sample_entry("a.wav", crate::sample_sources::Rating::NEUTRAL),
        sample_entry("b.wav", crate::sample_sources::Rating::NEUTRAL),
        sample_entry("c.wav", crate::sample_sources::Rating::NEUTRAL),
    ]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.focus_browser_row_only(1);

    controller.delete_browser_sample(1)?;

    assert_eq!(
        controller.sample_view.wav.selected_wav.as_deref(),
        Some(Path::new("c.wav"))
    );
    assert_eq!(controller.ui.browser.selected_visible, Some(1));

    controller.delete_browser_sample(1)?;

    assert_eq!(
        controller.sample_view.wav.selected_wav.as_deref(),
        Some(Path::new("a.wav"))
    );
    assert_eq!(controller.ui.browser.selected_visible, Some(0));
    Ok(())
}

#[test]
fn rating_auto_advance_works() {
    let (mut controller, _) = prepare_with_source_and_wav_entries(vec![
        sample_entry("one.wav", Rating::NEUTRAL),
        sample_entry("two.wav", Rating::NEUTRAL),
        sample_entry("three.wav", Rating::NEUTRAL),
    ]);

    // Initial state: first row focused
    controller.focus_browser_row(0);
    assert_eq!(controller.ui.browser.selected_visible, Some(0));

    // Case 1: Advance is ON (default)
    controller.adjust_selected_rating(1);
    assert_eq!(controller.ui.browser.selected_visible, Some(1), "Should advance to next row");

    // Case 2: Advance is ON, rating again
    controller.adjust_selected_rating(-1);
    assert_eq!(controller.ui.browser.selected_visible, Some(2), "Should advance to next row again");

    // Case 3: Advance is ON, but at the end
    controller.adjust_selected_rating(1);
    assert_eq!(controller.ui.browser.selected_visible, Some(2), "Should stay at the last row");

    // Case 4: Advance is OFF
    controller.set_advance_after_rating(false);
    controller.focus_browser_row(0);
    controller.adjust_selected_rating(1);
    assert_eq!(controller.ui.browser.selected_visible, Some(0), "Should NOT advance when setting is off");

    // Case 5: tag_selected should also advance
    controller.set_advance_after_rating(true);
    controller.focus_browser_row(0);
    controller.tag_selected(Rating::KEEP_1);
    assert_eq!(controller.ui.browser.selected_visible, Some(1), "tag_selected should also advance");
}
