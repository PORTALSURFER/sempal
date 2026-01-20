use super::ops::{normalize_folder_name, rename_target};
use std::path::Path;

#[test]
fn normalize_folder_name_rejects_empty_and_invalid() {
    assert!(normalize_folder_name("").is_err());
    assert!(normalize_folder_name("   ").is_err());
    assert!(normalize_folder_name(".").is_err());
    assert!(normalize_folder_name("..").is_err());
    assert!(normalize_folder_name("a/b").is_err());
    assert!(normalize_folder_name("a\\b").is_err());
}

#[test]
fn normalize_folder_name_trims_and_returns() {
    let name = normalize_folder_name("  Drums ").expect("valid name");
    assert_eq!(name, "Drums");
}

#[test]
fn rename_target_keeps_parent() {
    let target = Path::new("kits/old");
    let renamed = rename_target(target, "new").expect("rename ok");
    assert_eq!(renamed, Path::new("kits/new"));
}

#[test]
fn rename_target_can_be_noop() {
    let target = Path::new("kits/old");
    let renamed = rename_target(target, "old").expect("rename ok");
    assert_eq!(renamed, target);
}
