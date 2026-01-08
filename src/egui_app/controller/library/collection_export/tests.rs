use super::super::*;
use super::resolved_export_dir;
use crate::app_dirs::ConfigBaseGuard;
use crate::egui_app::controller::test_support::write_test_wav;
use crate::sample_sources::Collection;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

#[test]
fn manual_export_path_updates_name_and_path() {
    let renderer = crate::waveform::WaveformRenderer::new(4, 4);
    let mut controller = EguiController::new(renderer, None);
    let collection = Collection::new("Original");
    let id = collection.id.clone();
    controller.library.collections.push(collection);
    let temp = tempdir().unwrap();
    let manual_dir = temp.path().join("Manual Name");
    controller
        .set_collection_export_path(&id, Some(manual_dir.clone()))
        .unwrap();
    let stored = controller
        .library
        .collections
        .iter()
        .find(|c| c.id == id)
        .expect("collection present");
    assert_eq!(stored.name, "Manual Name");
    assert_eq!(stored.export_path.as_ref(), Some(&manual_dir));
}

#[test]
fn resolved_export_dir_prefers_manual_override() {
    let renderer = crate::waveform::WaveformRenderer::new(4, 4);
    let mut controller = EguiController::new(renderer, None);
    let mut collection = Collection::new("Manual");
    collection.export_path = Some(PathBuf::from("custom/manual"));
    controller.library.collections.push(collection);
    let dir = resolved_export_dir(
        &controller.library.collections[0],
        Some(Path::new("global/root")),
    )
    .expect("dir");
    assert_eq!(dir, PathBuf::from("custom/manual"));
}

#[test]
fn resolved_export_dir_uses_global_root_when_missing_override() {
    let renderer = crate::waveform::WaveformRenderer::new(4, 4);
    let mut controller = EguiController::new(renderer, None);
    controller.settings.collection_export_root = Some(PathBuf::from("global"));
    let collection = Collection::new("Global Collection");
    controller.library.collections.push(collection);
    let dir = resolved_export_dir(
        &controller.library.collections[0],
        controller.settings.collection_export_root.as_deref(),
    )
    .expect("dir");
    assert_eq!(dir, PathBuf::from("global").join("Global Collection"));
}

#[test]
fn setting_export_root_syncs_direct_subfolders_to_collections() {
    let temp = tempdir().unwrap();
    let _guard = ConfigBaseGuard::set(temp.path().to_path_buf());
    let export_root = temp.path().join("export_root");
    std::fs::create_dir_all(export_root.join("A")).unwrap();
    std::fs::create_dir_all(export_root.join("B")).unwrap();
    std::fs::create_dir_all(export_root.join(".hidden")).unwrap();
    std::fs::write(export_root.join("not_a_dir.txt"), b"x").unwrap();

    let renderer = crate::waveform::WaveformRenderer::new(4, 4);
    let mut controller = EguiController::new(renderer, None);

    let normalized_root = crate::sample_sources::config::normalize_path(export_root.as_path());
    controller
        .set_collection_export_root(Some(normalized_root.clone()))
        .unwrap();

    assert_eq!(controller.library.collections.len(), 2);
    assert!(controller.library.collections.iter().any(|c| c.name == "A"));
    assert!(controller.library.collections.iter().any(|c| c.name == "B"));

    let expected_a = crate::sample_sources::config::normalize_path(export_root.join("A").as_path());
    let expected_b = crate::sample_sources::config::normalize_path(export_root.join("B").as_path());
    assert!(
        controller
            .library
            .collections
            .iter()
            .any(|c| c.name == "A" && c.export_path.as_ref() == Some(&expected_a))
    );
    assert!(
        controller
            .library
            .collections
            .iter()
            .any(|c| c.name == "B" && c.export_path.as_ref() == Some(&expected_b))
    );
}

#[test]
fn sync_updates_existing_collection_export_path_by_name() {
    let temp = tempdir().unwrap();
    let _guard = ConfigBaseGuard::set(temp.path().to_path_buf());
    let export_root = temp.path().join("export_root");
    std::fs::create_dir_all(export_root.join("Existing")).unwrap();

    let renderer = crate::waveform::WaveformRenderer::new(4, 4);
    let mut controller = EguiController::new(renderer, None);
    controller
        .library
        .collections
        .push(Collection::new("Existing"));

    let created = controller
        .sync_collections_from_export_root_path(export_root.as_path())
        .unwrap();
    assert_eq!(created, 0);
    assert_eq!(controller.library.collections.len(), 1);

    let expected =
        crate::sample_sources::config::normalize_path(export_root.join("Existing").as_path());
    assert_eq!(
        controller.library.collections[0].export_path.as_ref(),
        Some(&expected)
    );
}

#[test]
fn sync_export_adds_files_as_clip_members() {
    let temp = tempdir().unwrap();
    let export_dir = temp.path().join("exports");
    std::fs::create_dir_all(&export_dir).unwrap();
    write_test_wav(&export_dir.join("clip.wav"), &[0.1, -0.1]);

    let renderer = crate::waveform::WaveformRenderer::new(4, 4);
    let mut controller = EguiController::new(renderer, None);
    let mut collection = Collection::new("Sync");
    collection.export_path = Some(export_dir.clone());
    let id = collection.id.clone();
    controller.library.collections.push(collection);

    controller.sync_collection_export(&id);

    let collection = controller
        .library
        .collections
        .iter()
        .find(|c| c.id == id)
        .expect("collection present");
    assert_eq!(collection.members.len(), 1);
    let member = &collection.members[0];
    assert_eq!(member.relative_path, PathBuf::from("clip.wav"));
    assert_eq!(member.clip_root.as_ref(), Some(&export_dir));
    assert!(export_dir.join(&member.relative_path).is_file());
}
