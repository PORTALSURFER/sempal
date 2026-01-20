use super::super::test_support::{dummy_controller, sample_entry, write_test_wav};
use super::super::*;
use super::common::visible_indices;
use crate::egui_app::state::FocusContext;
use std::path::{Path, PathBuf};

fn visible_paths(controller: &mut EguiController) -> Vec<PathBuf> {
    (0..controller.visible_browser_len())
        .filter_map(|row| controller.browser_path_for_visible(row))
        .collect()
}

#[test]
fn creating_folder_tracks_manual_entry() -> Result<(), String> {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.refresh_folder_browser();
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
fn folder_browser_includes_root_entry() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    controller.refresh_folder_browser();

    let rows = &controller.ui.sources.folders.rows;
    assert!(
        rows.first()
            .is_some_and(|row| row.is_root && row.path.as_os_str().is_empty())
    );
}

#[test]
fn folder_browser_lists_empty_folders() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    let empty = source.root.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    controller.refresh_folder_browser();

    assert!(
        controller
            .ui
            .sources
            .folders
            .rows
            .iter()
            .any(|row| row.path == PathBuf::from("empty"))
    );
}

#[test]
fn root_entry_stays_above_real_folders() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    let folder = source.root.join("rooted");
    std::fs::create_dir_all(&folder).unwrap();
    write_test_wav(&folder.join("clip.wav"), &[0.2, -0.2]);
    controller.set_wav_entries_for_tests(vec![sample_entry("rooted/clip.wav", crate::sample_sources::Rating::NEUTRAL)]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.refresh_folder_browser();

    let rows = &controller.ui.sources.folders.rows;
    assert!(rows.first().is_some_and(|row| row.is_root));
    assert!(
        rows.get(1)
            .is_some_and(|row| row.path == PathBuf::from("rooted"))
    );
}

#[test]
fn start_new_folder_at_root_sets_root_parent() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    controller.refresh_folder_browser();

    controller.start_new_folder_at_root();

    let new_folder = controller.ui.sources.folders.new_folder.as_ref().unwrap();
    assert!(new_folder.parent.as_os_str().is_empty());
}

#[test]
fn start_new_folder_uses_focused_parent() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    let folder = source.root.join("clips");
    std::fs::create_dir_all(&folder).unwrap();
    write_test_wav(&folder.join("clip.wav"), &[0.2, -0.2]);
    controller.set_wav_entries_for_tests(vec![sample_entry("clips/clip.wav", crate::sample_sources::Rating::NEUTRAL)]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.refresh_folder_browser();
    let folder_index = controller
        .ui
        .sources
        .folders
        .rows
        .iter()
        .position(|row| row.path == PathBuf::from("clips"))
        .unwrap();

    controller.focus_folder_row(folder_index);
    controller.start_new_folder();

    let new_folder = controller.ui.sources.folders.new_folder.as_ref().unwrap();
    assert_eq!(new_folder.parent, PathBuf::from("clips"));
    assert!(new_folder.focus_requested);
}

#[test]
fn start_new_folder_clears_search_query() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    controller.refresh_folder_browser();
    controller.set_folder_search("kick".to_string());
    assert_eq!(controller.ui.sources.folders.search_query, "kick");

    controller.start_new_folder();

    assert!(controller.ui.sources.folders.search_query.is_empty());
    assert!(controller.ui.sources.folders.new_folder.is_some());
}

#[test]
fn cancelling_new_folder_creation_clears_state() {
    let (mut controller, _) = dummy_controller();
    controller.ui.sources.folders.new_folder = Some(crate::egui_app::state::InlineFolderCreation {
        parent: PathBuf::new(),
        name: "temp".into(),
        focus_requested: false,
    });

    controller.cancel_new_folder_creation();

    assert!(controller.ui.sources.folders.new_folder.is_none());
}

#[test]
fn selecting_root_filters_to_root_files() -> Result<(), String> {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    let folder = source.root.join("rooted");
    std::fs::create_dir_all(&folder).unwrap();
    write_test_wav(&source.root.join("root.wav"), &[0.2, -0.2]);
    write_test_wav(&folder.join("clip.wav"), &[0.2, -0.2]);
    controller.set_wav_entries_for_tests(vec![
        sample_entry("root.wav", crate::sample_sources::Rating::NEUTRAL),
        sample_entry("rooted/clip.wav", crate::sample_sources::Rating::NEUTRAL),
    ]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.refresh_folder_browser();
    let folder_index = controller
        .ui
        .sources
        .folders
        .rows
        .iter()
        .position(|row| row.path == PathBuf::from("rooted"))
        .unwrap();

    controller.replace_folder_selection(folder_index);
    assert_eq!(
        visible_paths(&mut controller),
        vec![PathBuf::from("rooted/clip.wav")]
    );

    controller.replace_folder_selection(0);
    assert_eq!(
        visible_paths(&mut controller),
        vec![PathBuf::from("root.wav"), PathBuf::from("rooted/clip.wav")]
    );
    assert_eq!(controller.ui.sources.folders.focused, Some(0));

    controller.replace_folder_selection(0);
    assert_eq!(
        visible_paths(&mut controller),
        vec![PathBuf::from("root.wav")]
    );

    controller.toggle_folder_row_selection(folder_index);
    assert_eq!(
        visible_paths(&mut controller),
        vec![PathBuf::from("root.wav"), PathBuf::from("rooted/clip.wav")]
    );
    Ok(())
}

#[test]
fn renaming_folder_updates_entries_and_tree() -> Result<(), String> {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    let folder = source.root.join("old");
    std::fs::create_dir_all(&folder).unwrap();
    write_test_wav(&folder.join("clip.wav"), &[0.1, -0.1]);
    controller.set_wav_entries_for_tests(vec![sample_entry("old/clip.wav", crate::sample_sources::Rating::NEUTRAL)]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.refresh_folder_browser();

    controller.rename_folder(Path::new("old"), "new")?;

    assert!(!folder.exists());
    assert!(source.root.join("new/clip.wav").is_file());
    assert_eq!(
        controller.wav_entry(0).unwrap().relative_path,
        PathBuf::from("new/clip.wav")
    );
    assert!(
        controller
            .ui
            .sources
            .folders
            .rows
            .iter()
            .any(|row| row.path == PathBuf::from("new"))
    );
    Ok(())
}

#[test]
fn cancelling_folder_rename_clears_prompt() {
    let (mut controller, _source) = dummy_controller();
    controller.ui.sources.folders.pending_action =
        Some(crate::egui_app::state::FolderActionPrompt::Rename {
            target: PathBuf::from("folder"),
            name: "folder".into(),
        });
    controller.ui.sources.folders.rename_focus_requested = true;

    controller.cancel_folder_rename();

    assert!(controller.ui.sources.folders.pending_action.is_none());
    assert!(!controller.ui.sources.folders.rename_focus_requested);
}

#[test]
fn deleting_folder_removes_wavs() -> Result<(), String> {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    let target = source.root.join("gone");
    std::fs::create_dir_all(&target).unwrap();
    write_test_wav(&target.join("sample.wav"), &[0.0, 0.2]);
    controller.set_wav_entries_for_tests(vec![sample_entry("gone/sample.wav", crate::sample_sources::Rating::NEUTRAL)]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.refresh_folder_browser();
    if let Some(index) = controller
        .ui
        .sources
        .folders
        .rows
        .iter()
        .position(|row| row.path == PathBuf::from("gone"))
    {
        controller.focus_folder_row(index);
    }

    controller.delete_focused_folder();

    assert!(!target.exists());
    assert_eq!(controller.wav_entries_len(), 0);
    assert!(
        controller
            .ui
            .sources
            .folders
            .rows
            .iter()
            .all(|row| row.path != PathBuf::from("gone"))
    );
    Ok(())
}

#[test]
fn deleting_folder_rolls_back_on_db_failure() -> Result<(), String> {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    let target = source.root.join("gone");
    std::fs::create_dir_all(&target).unwrap();
    write_test_wav(&target.join("sample.wav"), &[0.0, 0.2]);
    controller.set_wav_entries_for_tests(vec![sample_entry("gone/sample.wav", crate::sample_sources::Rating::NEUTRAL)]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.refresh_folder_browser();
    if let Some(index) = controller
        .ui
        .sources
        .folders
        .rows
        .iter()
        .position(|row| row.path == PathBuf::from("gone"))
    {
        controller.focus_folder_row(index);
    }
    controller.runtime.fail_next_folder_delete_db = true;

    controller.delete_focused_folder();

    assert!(target.exists());
    assert_eq!(controller.wav_entries_len(), 1);
    assert!(
        controller
            .ui
            .sources
            .folders
            .rows
            .iter()
            .any(|row| row.path == PathBuf::from("gone"))
    );
    let db = crate::sample_sources::SourceDatabase::open(&source.root).unwrap();
    assert_eq!(db.count_files().unwrap(), 1);
    Ok(())
}

#[test]
fn deleting_folder_moves_focus_to_next_available() -> Result<(), String> {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    for folder in ["a", "b", "c"] {
        let path = source.root.join(folder);
        std::fs::create_dir_all(&path).unwrap();
        write_test_wav(&path.join(format!("{folder}.wav")), &[0.0, 0.2]);
    }
    controller.set_wav_entries_for_tests(vec![
        sample_entry("a/a.wav", crate::sample_sources::Rating::NEUTRAL),
        sample_entry("b/b.wav", crate::sample_sources::Rating::NEUTRAL),
        sample_entry("c/c.wav", crate::sample_sources::Rating::NEUTRAL),
    ]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.refresh_folder_browser();
    let focus_row = controller
        .ui
        .sources
        .folders
        .rows
        .iter()
        .position(|row| row.path == PathBuf::from("b"))
        .unwrap();
    controller.focus_folder_row(focus_row);

    controller.delete_focused_folder();

    let focused = controller.ui.sources.folders.focused.unwrap();
    assert_eq!(
        controller.ui.sources.folders.rows[focused].path,
        PathBuf::from("c")
    );

    controller.delete_focused_folder();

    let focused = controller.ui.sources.folders.focused.unwrap();
    assert_eq!(
        controller.ui.sources.folders.rows[focused].path,
        PathBuf::from("a")
    );
    Ok(())
}

#[test]
fn folder_focus_clears_when_context_changes() -> Result<(), String> {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    let folder = source.root.join("one");
    std::fs::create_dir_all(&folder).unwrap();
    write_test_wav(&folder.join("sample.wav"), &[0.2, -0.2]);
    controller.set_wav_entries_for_tests(vec![sample_entry("one/sample.wav", crate::sample_sources::Rating::NEUTRAL)]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.refresh_folder_browser();
    let row_index = controller
        .ui
        .sources
        .folders
        .rows
        .iter()
        .position(|row| row.path == PathBuf::from("one"))
        .unwrap();

    controller.replace_folder_selection(row_index);
    assert_eq!(controller.ui.sources.folders.focused, Some(row_index));

    controller.focus_browser_context();

    assert!(controller.ui.sources.folders.focused.is_none());
    controller.refresh_folder_browser();
    assert!(controller.ui.sources.folders.focused.is_none());
    assert_eq!(
        controller.selected_folder_paths(),
        vec![PathBuf::from("one")]
    );
    Ok(())
}

#[test]
fn clearing_folder_selection_shows_all_samples() -> Result<(), String> {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    std::fs::create_dir_all(source.root.join("a")).unwrap();
    std::fs::create_dir_all(source.root.join("b")).unwrap();
    controller.set_wav_entries_for_tests(vec![
        sample_entry("a/one.wav", crate::sample_sources::Rating::NEUTRAL),
        sample_entry("b/two.wav", crate::sample_sources::Rating::NEUTRAL),
    ]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.refresh_folder_browser();

    let folder_a = controller
        .ui
        .sources
        .folders
        .rows
        .iter()
        .position(|row| row.path == PathBuf::from("a"))
        .unwrap();
    controller.replace_folder_selection(folder_a);

    assert_eq!(controller.selected_folder_paths(), vec![PathBuf::from("a")]);
    assert_eq!(visible_indices(&controller), vec![0]);

    controller.clear_folder_selection();

    assert!(controller.selected_folder_paths().is_empty());
    assert_eq!(visible_indices(&controller), vec![0, 1]);
    Ok(())
}

#[test]
fn negated_folder_hides_samples() -> Result<(), String> {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    std::fs::create_dir_all(source.root.join("a")).unwrap();
    std::fs::create_dir_all(source.root.join("b")).unwrap();
    controller.set_wav_entries_for_tests(vec![
        sample_entry("a/one.wav", crate::sample_sources::Rating::NEUTRAL),
        sample_entry("b/two.wav", crate::sample_sources::Rating::NEUTRAL),
    ]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.refresh_folder_browser();

    let folder_a = controller
        .ui
        .sources
        .folders
        .rows
        .iter()
        .position(|row| row.path == PathBuf::from("a"))
        .unwrap();
    controller.toggle_folder_row_negation(folder_a);

    assert_eq!(
        visible_paths(&mut controller),
        vec![PathBuf::from("b/two.wav")]
    );
    Ok(())
}

#[test]
fn negated_root_hides_only_root_samples() -> Result<(), String> {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    std::fs::create_dir_all(source.root.join("sub")).unwrap();
    controller.set_wav_entries_for_tests(vec![
        sample_entry("root.wav", crate::sample_sources::Rating::NEUTRAL),
        sample_entry("sub/child.wav", crate::sample_sources::Rating::NEUTRAL),
    ]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.refresh_folder_browser();

    controller.toggle_folder_row_negation(0);

    assert_eq!(
        visible_paths(&mut controller),
        vec![PathBuf::from("sub/child.wav")]
    );
    Ok(())
}

#[test]
fn escape_does_not_clear_folder_filter_without_folder_focus() -> Result<(), String> {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    std::fs::create_dir_all(source.root.join("a")).unwrap();
    controller.set_wav_entries_for_tests(vec![sample_entry("a/one.wav", crate::sample_sources::Rating::NEUTRAL)]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.refresh_folder_browser();

    let folder_a = controller
        .ui
        .sources
        .folders
        .rows
        .iter()
        .position(|row| row.path == PathBuf::from("a"))
        .unwrap();
    controller.replace_folder_selection(folder_a);
    controller.ui.focus.context = FocusContext::SampleBrowser;

    controller.handle_escape();

    assert_eq!(controller.selected_folder_paths(), vec![PathBuf::from("a")]);
    Ok(())
}
