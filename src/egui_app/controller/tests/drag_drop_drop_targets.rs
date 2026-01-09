use super::super::test_support::write_test_wav;
use super::super::*;
use crate::app_dirs::ConfigBaseGuard;
use crate::egui_app::state::{DragPayload, DragSource, DragTarget};
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
