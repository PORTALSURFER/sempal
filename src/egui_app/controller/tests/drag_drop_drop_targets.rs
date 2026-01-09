use super::super::test_support::write_test_wav;
use super::super::*;
use crate::app_dirs::ConfigBaseGuard;
use crate::egui_app::state::{DragPayload, DragSource, DragTarget};
use crate::sample_sources::config::DropTargetConfig;
use crate::sample_sources::{Rating, SampleSource};
use std::path::{Path, PathBuf};
use tempfile::tempdir;

#[test]
fn drop_target_copy_duplicates_sample() {
    let temp = tempdir().unwrap();
    let _guard = ConfigBaseGuard::set(temp.path().to_path_buf());
    let root = temp.path().join("source");
    let dest = root.join("dest");
    std::fs::create_dir_all(&dest).unwrap();
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
    db.set_tag(Path::new("one.wav"), Rating::KEEP_1).unwrap();

    controller.ui.drag.payload = Some(DragPayload::Sample {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("one.wav"),
    });
    controller.ui.drag.copy_on_drop = true;
    controller.ui.drag.set_target(
        DragSource::DropTargets,
        DragTarget::DropTarget { path: dest.clone() },
    );
    controller.finish_active_drag();

    assert!(root.join("one.wav").is_file());
    assert!(dest.join("one.wav").is_file());

    let entries = db.list_files().unwrap();
    assert!(entries.iter().any(|entry| entry.relative_path == PathBuf::from("one.wav")));
    assert!(entries.iter().any(|entry| {
        entry.relative_path == PathBuf::from("dest/one.wav") && entry.tag == Rating::KEEP_1
    }));
}

#[test]
fn drop_target_panel_accepts_folder_drag() {
    let temp = tempdir().unwrap();
    let _guard = ConfigBaseGuard::set(temp.path().to_path_buf());
    let root = temp.path().join("source");
    let target = root.join("targets");
    std::fs::create_dir_all(&target).unwrap();
    let renderer = WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.library.sources.push(source.clone());

    controller.ui.drag.payload = Some(DragPayload::Folder {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("targets"),
    });
    controller.ui.drag.set_target(DragSource::DropTargets, DragTarget::DropTargetsPanel);
    controller.finish_active_drag();

    assert_eq!(controller.settings.drop_targets.len(), 1);
    assert_eq!(
        controller.settings.drop_targets[0].path,
        root.join("targets")
    );
}

#[test]
fn drop_target_drag_reorders_list() {
    let temp = tempdir().unwrap();
    let _guard = ConfigBaseGuard::set(temp.path().to_path_buf());
    let root = temp.path().join("source");
    let a = root.join("a");
    let b = root.join("b");
    let c = root.join("c");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    std::fs::create_dir_all(&c).unwrap();
    let renderer = WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.library.sources.push(source);
    controller.settings.drop_targets = vec![
        DropTargetConfig::new(a.clone()),
        DropTargetConfig::new(b.clone()),
        DropTargetConfig::new(c.clone()),
    ];
    controller.refresh_drop_targets_ui();

    controller.ui.drag.payload = Some(DragPayload::DropTargetReorder { path: a.clone() });
    controller.ui.drag.set_target(
        DragSource::DropTargets,
        DragTarget::DropTarget { path: c.clone() },
    );
    controller.finish_active_drag();

    assert_eq!(controller.settings.drop_targets[0].path, b);
    assert_eq!(controller.settings.drop_targets[1].path, a);
    assert_eq!(controller.settings.drop_targets[2].path, c);

    controller.ui.drag.payload = Some(DragPayload::DropTargetReorder { path: a.clone() });
    controller
        .ui
        .drag
        .set_target(DragSource::DropTargets, DragTarget::DropTargetsPanel);
    controller.finish_active_drag();

    assert_eq!(controller.settings.drop_targets[0].path, b);
    assert_eq!(controller.settings.drop_targets[1].path, c);
    assert_eq!(controller.settings.drop_targets[2].path, a);
}
