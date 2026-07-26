//! Contract tests for the standalone Radiant sibling dependency.

use std::fs;
use std::path::Path;

#[test]
fn canonical_radiant_dependency_uses_the_standalone_sibling_contract() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = fs::read_to_string(root.join("Cargo.toml")).expect("Cargo.toml");
    let metadata =
        fs::read_to_string(root.join("radiant-dependency.toml")).expect("radiant-dependency.toml");
    let env_docs = fs::read_to_string(root.join("docs/ENV_VARS.md")).expect("docs/ENV_VARS.md");

    assert!(manifest.contains("radiant = { path = \"../radiant\" }"));
    assert!(metadata.contains("repository = \"https://github.com/PORTALSURFER/radiant.git\""));
    assert!(metadata.contains("revision = \"14c2e70818d6c7fb78ff3f19b06b017e99bdcfa3\""));
    assert!(metadata.contains("path = \"../radiant\""));
    assert!(!root.join(".gitmodules").exists());
    assert!(!root.join("vendor/radiant").exists());
    assert!(!env_docs.contains("WAVECRATE_RADIANT_DIR"));

    let bash = fs::read_to_string(root.join("scripts/internal/radiant/sibling.sh"))
        .expect("Bash sibling helper");
    let powershell = fs::read_to_string(root.join("scripts/internal/radiant/sibling.ps1"))
        .expect("PowerShell sibling helper");
    for helper in [bash, powershell] {
        assert!(helper.contains("RADIANT_REPOSITORY_DEPLOY_KEY"));
        assert!(helper.contains("REVISION") || helper.contains("$Revision"));
        assert!(helper.contains("Cargo.toml"));
        assert!(helper.contains("WAVECRATE_RADIANT_DIR is unsupported"));
        assert!(helper.contains("does not match Cargo's configured sibling"));
    }
}
