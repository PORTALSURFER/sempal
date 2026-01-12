use super::super::test_support::{dummy_controller, write_test_wav};
use std::path::PathBuf;
use tempfile::tempdir;

#[test]
fn import_external_files_to_source_folder_copies_into_subfolder_and_db() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.cache_db(&source).unwrap();

    let temp = tempdir().unwrap();
    let input_path = temp.path().join("kick.wav");
    write_test_wav(&input_path, &[0.0, 0.25, -0.25]);

    controller.import_external_files_to_source_folder(
        PathBuf::from("Drums"),
        vec![input_path.clone()],
    );

    let expected_relative = PathBuf::from("Drums").join("kick.wav");
    let expected_absolute = source.root.join(&expected_relative);
    assert!(expected_absolute.is_file());

    let db = controller.database_for(&source).unwrap();
    let entries = db.list_files().unwrap();
    assert!(entries
        .iter()
        .any(|entry| entry.relative_path == expected_relative));
}
