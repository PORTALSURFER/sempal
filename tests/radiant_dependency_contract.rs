//! Contract tests for the standalone Radiant sibling dependency.

use std::fs;
use std::path::Path;

use toml::Value;

#[test]
fn canonical_radiant_dependency_uses_the_standalone_sibling_contract() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = fs::read_to_string(root.join("Cargo.toml")).expect("Cargo.toml");
    let metadata =
        fs::read_to_string(root.join("radiant-dependency.toml")).expect("radiant-dependency.toml");
    let env_docs = fs::read_to_string(root.join("docs/ENV_VARS.md")).expect("docs/ENV_VARS.md");
    let manifest_value: Value = manifest.parse().expect("valid Cargo.toml");
    let metadata_value: Value = metadata.parse().expect("valid radiant-dependency.toml");

    assert!(manifest.contains("radiant = { path = \"../radiant\" }"));
    assert!(metadata.contains("repository = \"https://github.com/PORTALSURFER/radiant.git\""));
    assert!(metadata.contains("path = \"../radiant\""));
    let workspace_revision = manifest_value
        .get("workspace")
        .and_then(|workspace| workspace.get("metadata"))
        .and_then(|metadata| metadata.get("radiant"))
        .and_then(|radiant| radiant.get("revision"))
        .and_then(Value::as_str)
        .expect("workspace.metadata.radiant.revision");
    let dependency_revision = metadata_value
        .get("revision")
        .and_then(Value::as_str)
        .expect("radiant-dependency.toml revision");
    assert_eq!(dependency_revision, workspace_revision);
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
