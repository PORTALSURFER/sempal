use super::super::FolderEntry;
use super::*;
use crate::native_app::sample_library::folder_browser::load_folder_at_path_with_browser_metadata;
use std::os::unix::fs as unix_fs;
use std::{collections::HashMap, sync::atomic::AtomicBool};

struct SymlinkFixture {
    root: PathBuf,
    outside: PathBuf,
    nested: PathBuf,
    loop_link: PathBuf,
    outside_link: PathBuf,
    file_link: PathBuf,
}

impl SymlinkFixture {
    fn new(name: &str) -> Self {
        let root = temp_source_root(&format!("{name}-root"));
        let outside = temp_source_root(&format!("{name}-outside"));
        let nested = root.join("nested");
        fs::create_dir_all(&nested).expect("create ordinary nested directory");
        fs::write(root.join("root.wav"), b"root").expect("write root sample");
        fs::write(nested.join("nested.wav"), b"nested").expect("write nested sample");
        fs::write(outside.join("outside.wav"), b"outside").expect("write outside sample");

        let loop_link = nested.join("loop-to-root");
        let outside_link = root.join("outside-link");
        let file_link = root.join("linked.wav");
        unix_fs::symlink(&root, &loop_link).expect("create ancestor loop link");
        unix_fs::symlink(&outside, &outside_link).expect("create outside directory link");
        unix_fs::symlink(outside.join("outside.wav"), &file_link)
            .expect("create outside file link");

        Self {
            root,
            outside,
            nested,
            loop_link,
            outside_link,
            file_link,
        }
    }

    fn assert_safe_tree(&self, folder: &FolderEntry) {
        assert!(
            folder.find(&path_id(&self.nested)).is_some(),
            "ordinary nested directories must remain visible"
        );
        assert!(folder.find(&path_id(&self.loop_link)).is_none());
        assert!(folder.find(&path_id(&self.outside_link)).is_none());
        assert!(folder.find_file(&path_id(&self.file_link)).is_none());

        let mut names = folder
            .all_files()
            .into_iter()
            .filter(|file| file.name.ends_with(".wav"))
            .map(|file| file.name.as_str())
            .collect::<Vec<_>>();
        names.sort_unstable();
        assert_eq!(names, vec!["nested.wav", "root.wav"]);
    }
}

impl Drop for SymlinkFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
        let _ = fs::remove_dir_all(&self.outside);
    }
}

#[test]
fn source_scanning_initial_load_and_tree_refresh_skip_symlink_entries() {
    let fixture = SymlinkFixture::new("wavecrate-browser-symlink-load");
    let mut browser = FolderBrowserState::from_root(fixture.root.clone());
    let loaded = browser
        .find_folder(&path_id(&fixture.root))
        .expect("source root should load");
    fixture.assert_safe_tree(loaded);

    let refresh = refresh_folder_tree_only(FolderTreeRefreshRequest {
        source_id: browser.selected_source_id().to_string(),
        label: String::from("Assets"),
        root: fixture.root.clone(),
        database_root: fixture.root.clone(),
    });
    assert!(refresh.source_root_available);
    assert_eq!(refresh.folder_count, 2);
    assert!(
        refresh.folder.find(&path_id(&fixture.nested)).is_some(),
        "ordinary nested directories must remain visible in folder-only refreshes"
    );
    assert!(refresh.folder.find(&path_id(&fixture.loop_link)).is_none());
    assert!(
        refresh
            .folder
            .find(&path_id(&fixture.outside_link))
            .is_none()
    );
    let _ = browser.apply_folder_tree_refresh_result(refresh);
    let refreshed = browser
        .find_folder(&path_id(&fixture.root))
        .expect("refreshed source root should remain available");
    fixture.assert_safe_tree(refreshed);
}

#[test]
fn worker_projection_opens_directory_targets_without_following_links() {
    let fixture = SymlinkFixture::new("wavecrate-browser-worker-symlink");
    let folder = load_folder_at_path_with_browser_metadata(
        &fixture.root,
        &fixture.root,
        &HashMap::new(),
        wavecrate::sample_sources::SourceTraversalPolicy::default(),
        &AtomicBool::new(false),
    )
    .expect("source root should project");
    fixture.assert_safe_tree(&folder);
}

#[test]
fn source_scanning_progress_agrees_with_authoritative_scanner_on_symlink_visibility() {
    let fixture = SymlinkFixture::new("wavecrate-browser-symlink-progress");
    let mut browser = FolderBrowserState::load_default();
    let request = browser
        .begin_add_source_path(fixture.root.clone(), 123)
        .expect("source scan request");
    let result = scan_source_with_progress(request, |_| {}, |_| {});

    assert_eq!(result.source_db_error, None);
    assert_eq!(result.folder_count, 1);
    assert_eq!(result.file_count, result.folder.all_files().len());
    fixture.assert_safe_tree(&result.folder);

    let database = SourceDatabase::open_for_test_fixture_source_write(&fixture.root)
        .expect("open source database");
    let mut manifest_paths = database
        .list_files()
        .expect("list authoritative manifest")
        .into_iter()
        .map(|entry| entry.relative_path)
        .collect::<Vec<_>>();
    manifest_paths.sort_unstable();
    assert_eq!(
        manifest_paths,
        vec![
            PathBuf::from("nested/nested.wav"),
            PathBuf::from("root.wav")
        ]
    );
}

#[test]
fn source_scanning_direct_verification_skips_new_symlink_entries() {
    let root = temp_source_root("wavecrate-browser-symlink-verify-root");
    let outside = temp_source_root("wavecrate-browser-symlink-verify-outside");
    fs::write(root.join("keep.wav"), b"keep").expect("write in-root sample");
    fs::write(outside.join("outside.wav"), b"outside").expect("write outside sample");
    let mut browser = FolderBrowserState::from_root(root.clone());

    unix_fs::symlink(&outside, root.join("outside-link")).expect("create outside directory link");
    unix_fs::symlink(outside.join("outside.wav"), root.join("linked.wav"))
        .expect("create outside file link");
    let request = browser
        .selected_folder_verify_request()
        .expect("selected root should be verifiable");
    let result =
        crate::native_app::sample_library::folder_browser::scan::verify_direct_folder(request);

    assert!(
        !browser.apply_direct_folder_verify_result(result),
        "symlink-only additions must not change the browser projection"
    );
    assert_eq!(
        browser
            .selected_files()
            .iter()
            .map(|file| file.name.as_str())
            .collect::<Vec<_>>(),
        vec!["keep.wav"]
    );
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(outside);
}

#[test]
fn source_scanning_targeted_refresh_removes_file_replaced_by_symlink() {
    let root = temp_source_root("wavecrate-browser-symlink-refresh-root");
    let outside = temp_source_root("wavecrate-browser-symlink-refresh-outside");
    let replaced = root.join("replaced.wav");
    let outside_file = outside.join("outside.wav");
    fs::write(&replaced, b"original").expect("write original sample");
    fs::write(&outside_file, b"outside").expect("write outside sample");
    let mut browser = FolderBrowserState::from_root(root.clone());
    let source_id = browser.selected_source_id().to_string();

    fs::remove_file(&replaced).expect("remove original sample");
    unix_fs::symlink(&outside_file, &replaced).expect("replace sample with outside link");
    assert!(browser.refresh_filesystem_paths(&source_id, &[PathBuf::from("replaced.wav")]));
    assert!(browser.selected_files().is_empty());

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(outside);
}

#[test]
fn source_scanning_direct_file_refresh_removes_file_replaced_by_symlink() {
    assert_file_refresh_removes_replaced_link("direct", |browser, path| {
        browser.refresh_file_path(path)
    });
}

#[test]
fn source_scanning_across_sources_refresh_removes_file_replaced_by_symlink() {
    assert_file_refresh_removes_replaced_link("across-sources", |browser, path| {
        browser.refresh_file_path_across_sources(path)
    });
}

#[test]
fn source_scanning_batched_file_refresh_removes_file_replaced_by_symlink() {
    assert_file_refresh_removes_replaced_link("batch", |browser, path| {
        browser.refresh_file_paths(&[path.to_path_buf()])
    });
}

fn assert_file_refresh_removes_replaced_link(
    name: &str,
    refresh: impl FnOnce(&mut FolderBrowserState, &std::path::Path) -> bool,
) {
    let root = temp_source_root(&format!("wavecrate-browser-symlink-{name}-root"));
    let outside = temp_source_root(&format!("wavecrate-browser-symlink-{name}-outside"));
    let replaced = root.join("replaced.wav");
    let outside_file = outside.join("outside.wav");
    fs::write(&replaced, b"original").expect("write original sample");
    fs::write(&outside_file, b"outside").expect("write outside sample");
    let mut browser = FolderBrowserState::from_root(root.clone());
    browser.select_file(path_id(&replaced));

    fs::remove_file(&replaced).expect("remove original sample");
    unix_fs::symlink(&outside_file, &replaced).expect("replace sample with outside link");
    assert!(refresh(&mut browser, &replaced));
    assert!(browser.selected_files().is_empty());
    assert!(browser.selected_file_paths().is_empty());

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(outside);
}
