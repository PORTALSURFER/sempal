/// Mirror the build-time version helpers for testing.
mod version_tools {
    include!("../build/version_tools.rs");
}

#[test]
fn bump_minor_increments_and_resets_patch() {
    let next = version_tools::bump_minor("1.2.3").unwrap();
    assert_eq!(next, "1.3.0");
}

#[test]
fn bump_minor_rejects_invalid_input() {
    let result = version_tools::bump_minor("not-a-version");
    assert!(result.is_err());
}
