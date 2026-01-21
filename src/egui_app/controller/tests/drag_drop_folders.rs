use super::super::test_support::{dummy_controller, sample_entry, write_test_wav};
use super::super::*;
use crate::app_dirs::ConfigBaseGuard;
use crate::egui_app::state::{DragPayload, DragSource, DragTarget};
use std::path::{Path, PathBuf};
use tempfile::tempdir;

#[test]
fn sample_drop_to_folder_moves_and_updates_state() {
    let temp = tempdir().unwrap();
    let _guard = ConfigBaseGuard::set(temp.path().to_path_buf());
    let root = temp.path().join("source");
    std::fs::create_dir_all(root.join("dest")).unwrap();
    let renderer = WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    controller.cache_db(&source).unwrap();

    write_test_wav(&root.join("one.wav"), &[0.1, 0.2]);
    let metadata = std::fs::metadata(root.join("one.wav")).unwrap();
    let modified_ns = metadata
        .modified()
        .unwrap()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as i64;
    let db = controller.database_for(&source).unwrap();
    db.upsert_file(Path::new("one.wav"), metadata.len(), modified_ns)
        .unwrap();
    controller.set_wav_entries_for_tests(vec![sample_entry("one.wav", crate::sample_sources::Rating::NEUTRAL)]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.ui.drag.payload = Some(DragPayload::Sample {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("one.wav"),
    });
    controller.ui.drag.set_target(
        DragSource::Folders,
        DragTarget::FolderPanel {
            folder: Some(PathBuf::from("dest")),
        },
    );
    controller.finish_active_drag();

    assert!(!root.join("one.wav").exists());
    assert!(root.join("dest").join("one.wav").is_file());
    assert!(
        controller
            .wav_index_for_path(Path::new("dest").join("one.wav").as_path())
            .is_some()
    );
}

#[test]
fn sample_drop_to_folder_rejects_conflicts() {
    let temp = tempdir().unwrap();
    let _guard = ConfigBaseGuard::set(temp.path().to_path_buf());
    let root = temp.path().join("source");
    let dest = root.join("dest");
    std::fs::create_dir_all(&dest).unwrap();
    write_test_wav(&root.join("one.wav"), &[0.1, 0.2]);
    write_test_wav(&dest.join("one.wav"), &[0.3, 0.4]);
    let renderer = WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    controller.cache_db(&source).unwrap();

    controller.set_wav_entries_for_tests(vec![sample_entry("one.wav", crate::sample_sources::Rating::NEUTRAL)]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.ui.drag.payload = Some(DragPayload::Sample {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("one.wav"),
    });
    controller.ui.drag.set_target(
        DragSource::Folders,
        DragTarget::FolderPanel {
            folder: Some(PathBuf::from("dest")),
        },
    );
    controller.finish_active_drag();

    assert!(root.join("one.wav").is_file());
    assert!(dest.join("one.wav").is_file());
    assert!(
        controller
            .wav_index_for_path(Path::new("one.wav"))
            .is_some()
    );
}

#[test]
fn folder_drop_to_folder_moves_tree() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    controller.cache_db(&source).unwrap();

    let src_folder = source.root.join("one");
    let dest_folder = source.root.join("dest");
    std::fs::create_dir_all(&src_folder).unwrap();
    std::fs::create_dir_all(&dest_folder).unwrap();
    write_test_wav(&src_folder.join("clip.wav"), &[0.1, 0.2]);

    controller.set_wav_entries_for_tests(vec![sample_entry("one/clip.wav", crate::sample_sources::Rating::NEUTRAL)]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.refresh_folder_browser();

    controller.ui.drag.payload = Some(DragPayload::Folder {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("one"),
    });
    controller.ui.drag.set_target(
        DragSource::Folders,
        DragTarget::FolderPanel {
            folder: Some(PathBuf::from("dest")),
        },
    );
    controller.finish_active_drag();

    assert!(!src_folder.exists());
    assert!(source.root.join("dest/one/clip.wav").is_file());
    assert_eq!(
        controller.wav_entry(0).unwrap().relative_path,
        PathBuf::from("dest/one/clip.wav")
    );
}
