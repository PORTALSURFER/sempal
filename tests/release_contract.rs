//! Release contract checks for active targets and release asset labels.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use toml::Value;

const RELEASE_CONTRACT: &str = include_str!("../release_contract.toml");
const WORKSPACE_MANIFEST: &str = include_str!("../Cargo.toml");
const WORKSPACE_LOCKFILE: &str = include_str!("../Cargo.lock");
const RELEASE_PACKAGE_MANIFESTS: &[(&str, &str)] = &[
    (
        "wavecrate-analysis",
        include_str!("../crates/wavecrate-analysis/Cargo.toml"),
    ),
    (
        "wavecrate-library",
        include_str!("../crates/wavecrate-library/Cargo.toml"),
    ),
    (
        "wavecrate-scan",
        include_str!("../crates/wavecrate-scan/Cargo.toml"),
    ),
    (
        "wavecrate-analysis-admin",
        include_str!("../tools/analysis-admin/Cargo.toml"),
    ),
    (
        "wavecrate-bench-cli",
        include_str!("../tools/bench-cli/Cargo.toml"),
    ),
];
const CODEOWNERS: &str = include_str!("../.github/CODEOWNERS");
const NIGHTLY_WORKFLOW: &str = include_str!("../.github/workflows/release-build.yml");
const RELEASE_TRAIN_PREP_WORKFLOW: &str =
    include_str!("../.github/workflows/release-train-prepare.yml");
const RC_WORKFLOW: &str = include_str!("../.github/workflows/release-rc.yml");
const STABLE_WORKFLOW: &str = include_str!("../.github/workflows/release-stable.yml.disabled");
const RADIANT_SMOKE_WORKFLOW: &str = include_str!("../.github/workflows/radiant-sibling-smoke.yml");
const RELEASE_TRAIN_PREP_SCRIPT: &str =
    include_str!("../scripts/internal/release/prepare_release_train.py");
const ASSEMBLE_RELEASE_FILES_SCRIPT: &str =
    include_str!("../scripts/internal/release/assemble_release_files.sh");
const BUILD_RELEASE_ARTIFACT_SCRIPT: &str =
    include_str!("../scripts/internal/release/build_release_artifact.sh");
const PROVISION_RADIANT_SCRIPT: &str =
    include_str!("../scripts/internal/release/provision_radiant_sibling.sh");
const SETUP_UBUNTU_RELEASE_DEPS_SCRIPT: &str =
    include_str!("../scripts/internal/release/setup_ubuntu_release_deps.sh");
const RELEASE_ZIP_SCRIPT: &str = include_str!("../scripts/internal/release/build_release_zip.sh");
const RELEASE_LOG_SCRIPT: &str =
    include_str!("../scripts/internal/release/generate_release_log.sh");
const PUBLISH_PORTALSURFER_SCRIPT: &str =
    include_str!("../scripts/internal/release/publish_portalsurfer_release.sh");
const SIGN_RELEASE_CHECKSUMS_SCRIPT: &str =
    include_str!("../scripts/internal/release/sign_release_checksums.sh");
const RUN_RELEASE_VALIDATION_SCRIPT: &str =
    include_str!("../scripts/internal/release/run_release_validation.sh");
const WRITE_RELEASE_SUMMARY_SCRIPT: &str =
    include_str!("../scripts/internal/release/write_release_step_summary.sh");
const VALIDATE_PROMOTED_RC_SCRIPT: &str =
    include_str!("../scripts/internal/release/validate_promoted_rc_release.py");
const VERIFY_PORTALSURFER_UPLOAD_CATALOG_SCRIPT: &str =
    include_str!("../scripts/internal/release/verify_portalsurfer_upload_catalog.py");
const VERIFY_PUBLISHED_RELEASE_SCRIPT: &str =
    include_str!("../scripts/internal/release/verify_published_release.py");
const ENV_VAR_DOCS: &str = include_str!("../docs/ENV_VARS.md");
const SCRIPTS_DOCS: &str = include_str!("../scripts/README.md");
const TEST_DOCS: &str = include_str!("../docs/TEST.md");
const TARGET_DOCS: &str = include_str!("../docs/TARGET.md");
const GETTING_STARTED_DOCS: &str = include_str!("../docs/book/src/getting-started.md");
const MANUAL_USAGE_DOCS: &str = include_str!("../manual/usage.md");
const UPDATER_ASSET_NAMES: &str = include_str!("../src/updater/asset_names.rs");
const CHECKSUMS_PUBLIC_KEY: &str = "8Z7dQJBRMbxCFkFMeBYa1FMSWOUm6nePFgoK5c43jT4=";

#[test]
fn workspace_release_identity_is_pre_one_and_consistent() {
    let workspace = WORKSPACE_MANIFEST
        .parse::<Value>()
        .expect("workspace manifest parses");
    let current_version = workspace["package"]["version"]
        .as_str()
        .expect("workspace package version");
    let parsed = semver::Version::parse(current_version).expect("workspace version is semver");

    assert_eq!(
        parsed.major, 0,
        "Wavecrate must remain pre-1.0 until an explicit 1.0 decision"
    );

    let release_package_names = RELEASE_PACKAGE_MANIFESTS
        .iter()
        .map(|(name, manifest)| {
            let manifest = manifest
                .parse::<Value>()
                .unwrap_or_else(|err| panic!("{name} manifest parses: {err}"));
            assert_eq!(
                manifest["package"]["version"].as_str(),
                Some(current_version),
                "{name} must share the Wavecrate release version"
            );
            *name
        })
        .chain(std::iter::once("wavecrate"))
        .collect::<BTreeSet<_>>();

    let lockfile = WORKSPACE_LOCKFILE
        .parse::<Value>()
        .expect("workspace lockfile parses");
    for package in lockfile["package"].as_array().expect("lockfile packages") {
        let Some(name) = package["name"].as_str() else {
            continue;
        };
        if release_package_names.contains(name) {
            assert_eq!(
                package["version"].as_str(),
                Some(current_version),
                "Cargo.lock entry for {name} must share the Wavecrate release version"
            );
        }
    }

    assert!(
        TARGET_DOCS.contains("Wavecrate uses pre-1.0 semantic versions"),
        "docs/TARGET.md must preserve the pre-1.0 release policy"
    );
    assert!(
        RC_WORKFLOW.contains("example 0.")
            && RC_WORKFLOW.contains("example release/0.")
            && STABLE_WORKFLOW.contains("example 0.")
            && STABLE_WORKFLOW.contains("example release/0."),
        "manual release workflows must demonstrate the pre-1.0 version and branch shape"
    );
}

#[test]
fn stable_workflow_is_inert_while_its_implementation_is_preserved() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for active_name in ["release-stable.yml", "release-stable.yaml"] {
        assert!(
            !repo_root
                .join(".github/workflows")
                .join(active_name)
                .exists(),
            "stable publication must not be exposed as active workflow {active_name}"
        );
    }
    assert!(
        STABLE_WORKFLOW.contains("name: Wavecrate stable release")
            && STABLE_WORKFLOW.contains("Publish GitHub stable release"),
        "the stable workflow implementation must remain available for later re-enablement"
    );
}

#[test]
fn platform_labels_are_active_release_labels_only() {
    let contract = parse_contract();
    let targets = contract_targets(&contract);
    let platform_labels = platform_labels(&contract);
    let active_labels = active_labels_for_targets(&targets);

    assert_eq!(
        platform_labels, active_labels,
        "release_contract.toml [platform_labels] must describe only active target labels"
    );
    assert!(
        !platform_labels.contains("linux"),
        "Linux is not an active release target and must not appear in [platform_labels]"
    );
}

#[test]
fn release_contract_targets_match_nightly_workflow_matrix() {
    let contract = parse_contract();
    let targets = contract_targets(&contract);
    let workflow_targets = workflow_release_targets(NIGHTLY_WORKFLOW);

    assert_eq!(
        workflow_targets,
        active_target_matrix(&targets),
        "release_contract.toml targets must match the nightly release workflow matrix"
    );
}

#[test]
fn nightly_workflow_runs_validation_before_build_and_publish() {
    assert!(
        NIGHTLY_WORKFLOW.contains("test:\n    name: Run nightly validation tests"),
        "nightly workflow must define a dedicated validation job"
    );
    assert!(
        NIGHTLY_WORKFLOW.contains("ref: ${{ needs.resolve-main.outputs.target_sha }}"),
        "nightly validation must run against the exact resolved main SHA"
    );
    assert!(
        NIGHTLY_WORKFLOW.contains("fetch-depth: 0"),
        "nightly validation must have enough history for workspace tests that inspect git state"
    );
    assert!(
        NIGHTLY_WORKFLOW.contains("scripts/internal/release/provision_radiant_sibling.sh"),
        "nightly validation must provision the pinned Radiant sibling"
    );
    assert!(
        NIGHTLY_WORKFLOW.contains("scripts/internal/release/emit_rust_toolchain_channel.py"),
        "nightly validation must use the pinned Rust toolchain"
    );
    assert!(
        NIGHTLY_WORKFLOW.contains("scripts/internal/release/run_release_validation.sh"),
        "nightly validation must run the shared release validation lane"
    );
    assert!(
        NIGHTLY_WORKFLOW.contains("needs: [resolve-main, test]"),
        "nightly package builds must wait for validation"
    );
    assert!(
        NIGHTLY_WORKFLOW.contains("needs: [resolve-main, test, release-log, build]"),
        "PortalSurfer nightly publication must wait for validation"
    );
    assert!(
        NIGHTLY_WORKFLOW
            .contains("needs: [resolve-main, test, release-log, build, publish-frontend]"),
        "GitHub nightly promotion must wait for validation and PortalSurfer publication"
    );

    let test_position = NIGHTLY_WORKFLOW
        .find("name: Run nightly validation tests")
        .expect("nightly validation job");
    let build_position = NIGHTLY_WORKFLOW
        .find("nightly\n    runs-on: ${{ matrix.os }}")
        .expect("nightly build job");
    let publish_frontend_position = NIGHTLY_WORKFLOW
        .find("name: Upload PortalSurfer nightly downloads")
        .expect("PortalSurfer nightly publish job");
    let publish_github_position = NIGHTLY_WORKFLOW
        .find("name: Promote GitHub nightly")
        .expect("GitHub nightly publish job");

    assert!(
        test_position < build_position
            && test_position < publish_frontend_position
            && test_position < publish_github_position,
        "nightly validation job should be declared before build and publish jobs"
    );
}

#[test]
fn release_validation_installs_ubuntu_audio_deps_before_cargo_tests() {
    for (name, workflow) in [
        ("nightly workflow", NIGHTLY_WORKFLOW),
        ("RC workflow", RC_WORKFLOW),
        ("stable workflow", STABLE_WORKFLOW),
    ] {
        let test_job = workflow_job_block(workflow, "test");
        let deps_position = test_job
            .find("scripts/internal/release/setup_ubuntu_release_deps.sh")
            .unwrap_or_else(|| panic!("{name} test job must install Ubuntu release dependencies"));
        let validation_position = test_job
            .find("scripts/internal/release/run_release_validation.sh")
            .unwrap_or_else(|| panic!("{name} test job must run shared release validation"));
        assert!(
            deps_position < validation_position,
            "{name} must install Ubuntu release dependencies before Cargo builds release validation"
        );
    }

    assert!(
        SETUP_UBUNTU_RELEASE_DEPS_SCRIPT.contains("pkg-config")
            && SETUP_UBUNTU_RELEASE_DEPS_SCRIPT.contains("libasound2-dev")
            && SETUP_UBUNTU_RELEASE_DEPS_SCRIPT.contains("pkg-config --exists alsa"),
        "Ubuntu release dependency helper must install and verify ALSA build dependencies"
    );
}

#[test]
fn release_validation_lane_builds_workspace_and_runs_focused_checks() {
    assert!(
        RUN_RELEASE_VALIDATION_SCRIPT
            .contains("cargo test --workspace --locked --no-run")
            && RUN_RELEASE_VALIDATION_SCRIPT.contains("cargo test --manifest-path \"$RADIANT_DIR/Cargo.toml\""),
        "release validation must compile Wavecrate workspace targets and standalone Radiant targets"
    );
    assert!(
        RUN_RELEASE_VALIDATION_SCRIPT.contains("cargo test --test release_contract")
            && RUN_RELEASE_VALIDATION_SCRIPT.contains("cargo test --test release_workflow_helpers")
            && RUN_RELEASE_VALIDATION_SCRIPT.contains("cargo test -p wavecrate-scan --lib"),
        "release validation must run focused release and scanner tests"
    );
}

#[test]
fn release_workflows_define_timeouts_and_step_summaries() {
    for (workflow_name, workflow, jobs) in [
        (
            "nightly workflow",
            NIGHTLY_WORKFLOW,
            &[
                ("resolve-main", "10"),
                ("release-log", "20"),
                ("test", "75"),
                ("build", "120"),
                ("publish-github", "30"),
                ("publish-frontend", "45"),
            ][..],
        ),
        (
            "RC workflow",
            RC_WORKFLOW,
            &[
                ("resolve", "15"),
                ("test", "75"),
                ("build", "120"),
                ("publish", "45"),
            ][..],
        ),
        (
            "stable workflow",
            STABLE_WORKFLOW,
            &[
                ("resolve", "30"),
                ("test", "75"),
                ("build", "120"),
                ("publish", "45"),
            ][..],
        ),
        (
            "release train prep workflow",
            RELEASE_TRAIN_PREP_WORKFLOW,
            &[("prepare", "60")][..],
        ),
    ] {
        for (job, minutes) in jobs {
            assert_job_timeout(workflow_name, workflow, job, minutes);
        }
        assert!(
            workflow.contains("if: ${{ always() }}"),
            "{workflow_name} must write release summaries from always() steps"
        );
        assert!(
            workflow.contains("scripts/internal/release/write_release_step_summary.sh \\"),
            "{workflow_name} must use the shared release summary helper"
        );
    }
}

#[test]
fn portalsurfer_publish_helper_bounds_http_and_summarizes_state() {
    assert!(
        PUBLISH_PORTALSURFER_SCRIPT
            .contains("PORTALSURFER_RELEASE_CONNECT_TIMEOUT_SECONDS=\"${PORTALSURFER_RELEASE_CONNECT_TIMEOUT_SECONDS:-10}\""),
        "PortalSurfer helper must default to a bounded connect timeout"
    );
    assert!(
        PUBLISH_PORTALSURFER_SCRIPT
            .contains("PORTALSURFER_RELEASE_MAX_TIME_SECONDS=\"${PORTALSURFER_RELEASE_MAX_TIME_SECONDS:-300}\""),
        "PortalSurfer helper must default to a bounded per-request transfer timeout"
    );
    assert!(
        PUBLISH_PORTALSURFER_SCRIPT
            .contains("--connect-timeout \"$PORTALSURFER_RELEASE_CONNECT_TIMEOUT_SECONDS\"")
            && PUBLISH_PORTALSURFER_SCRIPT
                .contains("--max-time \"$PORTALSURFER_RELEASE_MAX_TIME_SECONDS\""),
        "PortalSurfer curl calls must include connection and total transfer bounds"
    );
    assert!(
        PUBLISH_PORTALSURFER_SCRIPT
            .matches("curl \"${curl_args[@]}\"")
            .count()
            >= 5,
        "PortalSurfer upload, commit, and verification fetches must use the bounded curl policy"
    );
    assert!(
        PUBLISH_PORTALSURFER_SCRIPT.contains("write_portalsurfer_summary \"$status_code\""),
        "PortalSurfer helper must summarize success or the failed upload phase"
    );
    assert!(
        WRITE_RELEASE_SUMMARY_SCRIPT.contains("GITHUB_STEP_SUMMARY")
            && WRITE_RELEASE_SUMMARY_SCRIPT.contains("SHA-256")
            && WRITE_RELEASE_SUMMARY_SCRIPT.contains("PortalSurfer catalog"),
        "release summary helper must write structured public release metadata"
    );
    assert!(
        !WRITE_RELEASE_SUMMARY_SCRIPT.contains("UPLOAD_TOKEN")
            && !WRITE_RELEASE_SUMMARY_SCRIPT.contains("SIGNING_KEY")
            && !WRITE_RELEASE_SUMMARY_SCRIPT.contains("APPLE_"),
        "release summary helper must not read or print release secrets"
    );
}

#[test]
fn release_contract_targets_match_manual_release_workflow_matrices() {
    let contract = parse_contract();
    let targets = contract_targets(&contract);
    let active = active_target_matrix(&targets);

    assert_eq!(
        workflow_release_targets(RC_WORKFLOW),
        active,
        "RC workflow matrix must match release_contract.toml targets"
    );
    assert_eq!(
        workflow_release_targets(STABLE_WORKFLOW),
        active,
        "stable workflow matrix must match release_contract.toml targets"
    );
}

#[test]
fn release_contract_declares_required_channels() {
    let contract = parse_contract();
    let channels: BTreeSet<_> = contract
        .get("channels")
        .and_then(Value::as_array)
        .expect("channels")
        .iter()
        .map(|channel| channel.as_str().expect("channel string"))
        .collect();

    assert_eq!(channels, BTreeSet::from(["nightly", "rc", "stable"]));
}

#[test]
fn release_contract_template_emits_supported_nightly_asset_names() {
    let contract = parse_contract();
    let targets = contract_targets(&contract);
    let template = contract
        .get("templates")
        .and_then(Value::as_table)
        .and_then(|templates| templates.get("nightly_asset"))
        .and_then(Value::as_str)
        .expect("nightly_asset template");

    let asset_names: BTreeSet<String> = active_target_matrix(&targets)
        .into_iter()
        .map(|target| {
            template
                .replace("{APP_NAME}", app_name(&contract))
                .replace("{platform}", &target.platform)
                .replace("{arch}", &target.arch)
        })
        .collect();

    assert_eq!(
        asset_names,
        BTreeSet::from([
            "wavecrate-nightly-macos-aarch64.zip".to_string(),
            "wavecrate-nightly-macos-x86_64.zip".to_string(),
            "wavecrate-nightly-windows-x86_64.zip".to_string(),
        ])
    );
    assert!(
        asset_names.iter().all(|name| !name.contains("linux")),
        "nightly assets generated from the release contract must not include Linux"
    );
}

#[test]
fn release_surfaces_describe_supported_artifacts_without_setup_contracts() {
    assert!(
        TEST_DOCS.contains("### Windows ZIP Distribution Lifecycle"),
        "docs/TEST.md must describe the supported manual Windows release lifecycle"
    );
    assert!(
        GETTING_STARTED_DOCS.contains("On Windows, Wavecrate is a manual ZIP install"),
        "user docs must explain the supported Windows ZIP install path"
    );
    assert!(
        TARGET_DOCS.contains("Windows release installs are manual by design"),
        "target contract must keep the manual Windows install policy explicit"
    );
    assert!(
        MANUAL_USAGE_DOCS.contains("Downloaded bundles may include ML assets"),
        "manual usage docs must avoid setup-managed asset-copy promises"
    );

    for (surface, contents) in release_distribution_surfaces() {
        for forbidden in unsupported_setup_contract_terms() {
            assert!(
                !contents.contains(&forbidden),
                "{surface} must not advertise unsupported OS-managed release behavior: {forbidden}"
            );
        }
    }
}

#[test]
fn release_lifecycle_docs_separate_rc_stabilization_from_stable_promotion() {
    for (surface, contents) in [
        ("docs/ENV_VARS.md", ENV_VAR_DOCS),
        ("docs/TEST.md", TEST_DOCS),
        ("docs/TARGET.md", TARGET_DOCS),
        ("scripts/README.md", SCRIPTS_DOCS),
    ] {
        assert!(
            contents.contains("stabilization"),
            "{surface} must explain that an RC starts stabilization"
        );
        assert!(
            contents.contains("explicit") && contents.contains("stable"),
            "{surface} must keep stable publication behind an explicit decision"
        );
    }

    assert!(
        TEST_DOCS.contains("Continue normal PRs into `main`")
            && TEST_DOCS.contains("published as an RC"),
        "docs/TEST.md must describe continuous stabilization work and re-RC promotion"
    );
    assert!(
        ENV_VAR_DOCS.contains("--version X.Y.Z")
            && ENV_VAR_DOCS.contains("--source-ref main")
            && ENV_VAR_DOCS.contains("--push"),
        "docs/ENV_VARS.md must show how to advance a train at the same version"
    );
}

#[test]
fn workspace_no_longer_declares_removed_setup_crate() {
    for (surface, contents) in [
        ("Cargo.toml", WORKSPACE_MANIFEST),
        ("Cargo.lock", WORKSPACE_LOCKFILE),
        (".github/CODEOWNERS", CODEOWNERS),
    ] {
        for forbidden in removed_setup_crate_terms() {
            assert!(
                !contents.contains(&forbidden),
                "{surface} must not keep the removed setup-crate contract: {forbidden}"
            );
        }
    }
}

#[test]
fn release_packager_uses_contract_nightly_asset_name_without_build_number() {
    let contract = parse_contract();
    let nightly_template = contract
        .get("templates")
        .and_then(Value::as_table)
        .and_then(|templates| templates.get("nightly_asset"))
        .and_then(Value::as_str)
        .expect("nightly_asset template");
    let expected_assignment = format!(
        r#"ZIP_NAME="{}""#,
        nightly_template
            .replace("{APP_NAME}", "${APP_NAME}")
            .replace("{platform}", "${PLATFORM}")
            .replace("{arch}", "${ARCH}")
    );
    let expected_updater_format = nightly_template
        .replace("{APP_NAME}", "{APP_NAME}")
        .replace("{platform}", "{platform}")
        .replace("{arch}", "{arch}");

    assert!(
        RELEASE_ZIP_SCRIPT.contains(&expected_assignment),
        "nightly packager ZIP_NAME must match release_contract.toml: {expected_assignment}"
    );
    assert!(
        UPDATER_ASSET_NAMES.contains(&expected_updater_format),
        "GitHub updater nightly asset lookup must match release_contract.toml: {expected_updater_format}"
    );
    assert!(
        !RELEASE_ZIP_SCRIPT.contains("nightly${BUILD_LABEL}"),
        "rolling GitHub nightly assets must not include build-number labels"
    );
    assert!(
        RELEASE_ZIP_SCRIPT.contains(r#"printf "%s  %s\n" "$SHA" "$ZIP_NAME""#),
        "checksums-entry.txt must list the exact zip filename selected by the packager"
    );
    assert!(
        RELEASE_ZIP_SCRIPT.contains("Compress-Archive -LiteralPath"),
        "PowerShell zip fallback must archive the app root directory as a literal path"
    );
    assert!(
        !RELEASE_ZIP_SCRIPT.contains(r#"$POWERSHELL_WORK_DIR\\$APP_NAME\\*"#),
        "PowerShell zip fallback must not flatten the archive root with a wildcard"
    );
}

fn release_distribution_surfaces() -> Vec<(&'static str, &'static str)> {
    vec![
        ("release_contract.toml", RELEASE_CONTRACT),
        ("nightly release workflow", NIGHTLY_WORKFLOW),
        ("RC release workflow", RC_WORKFLOW),
        ("stable release workflow", STABLE_WORKFLOW),
        ("release artifact builder", BUILD_RELEASE_ARTIFACT_SCRIPT),
        ("release ZIP builder", RELEASE_ZIP_SCRIPT),
        ("release assembly helper", ASSEMBLE_RELEASE_FILES_SCRIPT),
        ("post-publish verifier", VERIFY_PUBLISHED_RELEASE_SCRIPT),
        ("docs/TEST.md", TEST_DOCS),
        ("docs/book/src/getting-started.md", GETTING_STARTED_DOCS),
        ("manual/usage.md", MANUAL_USAGE_DOCS),
    ]
}

fn unsupported_setup_contract_terms() -> Vec<String> {
    let old_binary_suffix = ["instal", "ler"].concat();
    vec![
        ["wavecrate", old_binary_suffix.as_str()].join("-"),
        ["Start", "Menu"].join(" "),
        ["uninstall", "registry"].join(" "),
        ["Add/Remove", "Programs"].join(" "),
        ["Programs", "and", "Features"].join(" "),
    ]
}

fn removed_setup_crate_terms() -> Vec<String> {
    let old_binary_suffix = ["instal", "ler"].concat();
    vec![
        ["apps", old_binary_suffix.as_str()].join("/"),
        ["wavecrate", old_binary_suffix.as_str()].join("-"),
    ]
}

#[test]
fn nightly_workflow_publishes_packager_outputs_as_github_assets() {
    assert!(
        NIGHTLY_WORKFLOW.contains("scripts/internal/release/build_release_artifact.sh \\"),
        "nightly workflow must use the shared release artifact builder"
    );
    assert!(
        NIGHTLY_WORKFLOW.contains("--channel nightly \\"),
        "nightly workflow must call the packager in nightly mode"
    );
    assert!(
        NIGHTLY_WORKFLOW.contains("scripts/internal/release/assemble_release_files.sh"),
        "GitHub nightly release must publish the zip filenames emitted by the packager"
    );
    assert!(
        NIGHTLY_WORKFLOW.contains("--checksum-name checksums-nightly.txt"),
        "GitHub nightly checksums must be assembled from packager checksum entries"
    );
    assert!(
        NIGHTLY_WORKFLOW.contains("gh release upload nightly")
            && NIGHTLY_WORKFLOW.contains("--clobber \"${release_files[@]}\""),
        "GitHub nightly release must refresh the assembled dist/release assets"
    );
    assert!(
        RELEASE_ZIP_SCRIPT.contains("ZIP_NAME=\"${APP_NAME}-nightly-${PLATFORM}-${ARCH}.zip\""),
        "rolling GitHub nightly workflow must not introduce build-numbered asset names"
    );
    assert!(
        NIGHTLY_WORKFLOW
            .contains("portal_build_id=\"wavecrate-nightly-b${build_number}-${short_sha}\""),
        "PortalSurfer nightly build ids must be immutable and URL-path safe"
    );
    assert!(
        !NIGHTLY_WORKFLOW.contains("portal_build_id=\"wavecrate-${nightly_version}\""),
        "PortalSurfer nightly build ids must not contain SemVer build metadata '+' characters"
    );
}

#[test]
fn nightly_workflow_promotes_public_identity_after_public_surfaces_are_ready() {
    assert!(
        NIGHTLY_WORKFLOW.contains("cancel-in-progress: false"),
        "nightly runs must not cancel an in-flight public promotion"
    );
    assert!(
        !NIGHTLY_WORKFLOW.contains("cancel-in-progress: true"),
        "nightly runs must not use cancellation-prone public promotion"
    );
    assert!(
        NIGHTLY_WORKFLOW
            .contains("needs: [resolve-main, test, release-log, build, publish-frontend]"),
        "GitHub nightly promotion must wait for validation plus the PortalSurfer upload and verifier job"
    );
    assert!(
        NIGHTLY_WORKFLOW.contains("needs: [resolve-main, test, release-log, build]"),
        "PortalSurfer upload must wait for validation and be able to complete before GitHub nightly identity promotion"
    );
    assert!(
        !NIGHTLY_WORKFLOW.contains("needs: [resolve-main, publish-github]"),
        "PortalSurfer upload must not wait on rolling GitHub identity promotion"
    );

    let refresh_assets = NIGHTLY_WORKFLOW
        .find("Refresh rolling nightly release assets")
        .expect("rolling nightly asset refresh step");
    let promote_tags = NIGHTLY_WORKFLOW
        .find("Promote nightly tags")
        .expect("nightly tag promotion step");
    let update_metadata = NIGHTLY_WORKFLOW
        .find("Update rolling nightly release metadata")
        .expect("nightly release metadata update step");
    let verify_github = NIGHTLY_WORKFLOW
        .find("Verify published GitHub nightly release")
        .expect("GitHub nightly verifier step");

    assert!(
        refresh_assets < promote_tags,
        "rolling GitHub assets must be refreshed before nightly tags move"
    );
    assert!(
        promote_tags < update_metadata && update_metadata < verify_github,
        "rolling GitHub metadata and verification must happen after tag promotion"
    );
}

#[test]
fn release_contract_templates_emit_supported_rc_and_stable_asset_names() {
    let contract = parse_contract();
    let targets = contract_targets(&contract);
    let templates = contract
        .get("templates")
        .and_then(Value::as_table)
        .expect("templates");
    let rc_template = templates
        .get("rc_asset")
        .and_then(Value::as_str)
        .expect("rc_asset template");
    let stable_template = templates
        .get("stable_asset")
        .and_then(Value::as_str)
        .expect("stable_asset template");

    let rc_names: BTreeSet<String> = active_target_matrix(&targets)
        .iter()
        .map(|target| apply_asset_template(rc_template, app_name(&contract), target, "0.19.1", "2"))
        .collect();
    let stable_names: BTreeSet<String> = active_target_matrix(&targets)
        .iter()
        .map(|target| {
            apply_asset_template(stable_template, app_name(&contract), target, "0.19.1", "2")
        })
        .collect();

    assert_eq!(
        rc_names,
        BTreeSet::from([
            "wavecrate-0.19.1-rc.2-macos-aarch64.zip".to_string(),
            "wavecrate-0.19.1-rc.2-macos-x86_64.zip".to_string(),
            "wavecrate-0.19.1-rc.2-windows-x86_64.zip".to_string(),
        ])
    );
    assert_eq!(
        stable_names,
        BTreeSet::from([
            "wavecrate-0.19.1-macos-aarch64.zip".to_string(),
            "wavecrate-0.19.1-macos-x86_64.zip".to_string(),
            "wavecrate-0.19.1-windows-x86_64.zip".to_string(),
        ])
    );
}

#[test]
fn stable_workflow_requires_matching_rc_before_publish() {
    assert!(
        STABLE_WORKFLOW.contains("requires at least one RC tag"),
        "stable workflow must fail when no RC exists"
    );
    assert!(
        STABLE_WORKFLOW.contains("Latest RC ${rc_tag} points at"),
        "stable workflow must require the latest RC tag to point at the stable target commit"
    );
}

#[test]
fn stable_workflow_validates_promoted_rc_release_before_publish() {
    assert!(
        STABLE_WORKFLOW.contains("scripts/internal/release/validate_promoted_rc_release.py \\"),
        "stable workflow must validate the promoted RC GitHub release"
    );
    assert!(
        STABLE_WORKFLOW.contains("--rc-tag \"${{ steps.resolve.outputs.rc_tag }}\""),
        "stable workflow must validate the exact latest RC tag selected by resolve"
    );
    assert!(
        STABLE_WORKFLOW.contains("--repo \"${{ github.repository }}\""),
        "stable workflow must validate the RC release in the current GitHub repository"
    );
    assert!(
        STABLE_WORKFLOW.contains("--checksum-public-key"),
        "stable workflow must verify the promoted RC checksum signature"
    );
    let validate_position = STABLE_WORKFLOW
        .find("Validate promoted RC GitHub release artifacts")
        .expect("stable workflow validation step");
    let test_job_position = STABLE_WORKFLOW
        .find("name: Run stable validation tests")
        .expect("stable workflow test job");
    assert!(
        validate_position < test_job_position,
        "stable workflow must validate the promoted RC before stable test/build jobs start"
    );
}

#[test]
fn rc_and_stable_workflows_use_structured_release_log_generator() {
    for (name, workflow) in [
        ("RC workflow", RC_WORKFLOW),
        ("stable workflow", STABLE_WORKFLOW),
    ] {
        assert!(
            workflow.contains("fetch-depth: 0"),
            "{name} must fetch full history so release log ranges can be generated"
        );
        assert!(
            workflow.contains("tool: git-cliff"),
            "{name} must install git-cliff for generated release logs"
        );
        assert!(
            workflow.contains("scripts/internal/release/generate_release_log.sh \\"),
            "{name} must invoke the shared structured release-log generator"
        );
        assert!(
            workflow.contains("scripts/internal/release/prepare_github_release_body.py \\"),
            "{name} must bound the GitHub release body before public publication"
        );
        assert!(
            workflow.contains("--input dist/release/release-log.md"),
            "{name} must prepare the GitHub body from the canonical release log"
        );
        assert!(
            workflow.contains("--output dist/github-release-body.md"),
            "{name} must keep the bounded GitHub body outside the release asset directory"
        );
        assert!(
            workflow.contains("body_path: dist/github-release-body.md"),
            "{name} must publish the bounded GitHub release body"
        );
    }
    assert!(
        !RC_WORKFLOW.contains("Release candidate for final validation."),
        "RC workflow must not fall back to a manual-only minimal release body"
    );
    assert!(
        !STABLE_WORKFLOW.contains("Stable release promoted from ${RC_TAG}."),
        "stable workflow must not fall back to a manual-only minimal release body"
    );
}

#[test]
fn rc_and_stable_workflows_publish_to_portalsurfer_catalog() {
    for (name, workflow, channel) in [
        ("RC workflow", RC_WORKFLOW, "rc"),
        ("stable workflow", STABLE_WORKFLOW, "stable"),
    ] {
        assert!(
            workflow.contains("scripts/internal/release/publish_portalsurfer_release.sh \\"),
            "{name} must publish release artifacts through the shared PortalSurfer helper"
        );
        assert!(
            workflow.contains(&format!("--channel {channel} \\")),
            "{name} must pass the explicit PortalSurfer channel"
        );
        assert!(
            workflow.contains("PORTALSURFER_RELEASE_UPLOAD_TOKEN"),
            "{name} must use the PortalSurfer upload token"
        );
        assert!(
            workflow.contains("--artifact-dir dist/release"),
            "{name} must publish the same assembled release files used for GitHub"
        );
        assert!(
            workflow.contains("--release-log dist/release/release-log.md"),
            "{name} must upload the generated release-bound markdown log"
        );
        assert!(
            workflow.contains("--full-changelog-out dist/release/changelog.md"),
            "{name} must update the full PortalSurfer changelog"
        );
    }
    assert!(
        RC_WORKFLOW.contains("portal_build_id=\"wavecrate-${release_version}\""),
        "RC PortalSurfer build ids must include the RC version"
    );
    assert!(
        STABLE_WORKFLOW.contains("portal_build_id=\"wavecrate-${VERSION}\""),
        "stable PortalSurfer build ids must include the stable version"
    );
}

#[test]
fn portalsurfer_publish_helper_uses_shared_catalog_verifier() {
    assert!(
        PUBLISH_PORTALSURFER_SCRIPT.contains("$upload_base/$BUILD_ID/staging/files/$encoded_name"),
        "PortalSurfer publish helper must stage release files before public catalog commit"
    );
    assert!(
        PUBLISH_PORTALSURFER_SCRIPT.contains("$upload_base/$BUILD_ID/commit"),
        "PortalSurfer publish helper must commit staged release files, release log, and full changelog together"
    );
    assert!(
        !PUBLISH_PORTALSURFER_SCRIPT.contains("$upload_base/$BUILD_ID/files/$encoded_name"),
        "PortalSurfer publish helper must not directly mutate public release files"
    );
    assert!(
        !PUBLISH_PORTALSURFER_SCRIPT.contains("$upload_base/$BUILD_ID/changelog"),
        "PortalSurfer publish helper must not directly mutate the public per-release changelog"
    );
    assert!(
        !PUBLISH_PORTALSURFER_SCRIPT.contains("$upload_base/changelog"),
        "PortalSurfer publish helper must not directly mutate the public full changelog"
    );
    assert!(
        PUBLISH_PORTALSURFER_SCRIPT
            .contains("scripts/internal/release/verify_portalsurfer_upload_catalog.py"),
        "PortalSurfer publish helper must verify the catalog through the shared upload verifier"
    );
    assert!(
        PUBLISH_PORTALSURFER_SCRIPT.contains("--current-build-number \"$BUILD_NUMBER\"")
            && PUBLISH_PORTALSURFER_SCRIPT.contains("--current-version \"$RELEASE_VERSION\"")
            && PUBLISH_PORTALSURFER_SCRIPT.contains("--current-released-at \"$RELEASED_AT\""),
        "PortalSurfer publish helper must assemble the full changelog from staged current release metadata before commit"
    );
    assert!(
        VERIFY_PORTALSURFER_UPLOAD_CATALOG_SCRIPT.contains("parse_released_at"),
        "PortalSurfer upload catalog verifier must normalize release timestamp precision"
    );
}

#[test]
fn release_workflows_verify_published_artifacts_after_publication() {
    for (name, workflow, channel) in [
        ("nightly workflow", NIGHTLY_WORKFLOW, "nightly"),
        ("RC workflow", RC_WORKFLOW, "rc"),
        ("stable workflow", STABLE_WORKFLOW, "stable"),
    ] {
        assert!(
            workflow.contains("scripts/internal/release/verify_published_release.py \\"),
            "{name} must invoke the shared post-publish verifier"
        );
        assert!(
            workflow.contains(&format!("--channel {channel} \\")),
            "{name} must verify the published assets with the matching release channel"
        );
        assert!(
            workflow.contains(&format!("--checksum-public-key {CHECKSUMS_PUBLIC_KEY}")),
            "{name} must verify checksum signatures with the pinned public key"
        );
        assert!(
            workflow.contains("--surface portalsurfer"),
            "{name} must verify PortalSurfer downloads after PortalSurfer publication"
        );
        assert!(
            workflow
                .matches("PORTALSURFER_RELEASE_UPLOAD_TOKEN:")
                .count()
                >= 2,
            "{name} must authenticate both PortalSurfer publication and post-publish verification"
        );
    }
    assert!(
        NIGHTLY_WORKFLOW.contains("Verify published GitHub nightly release"),
        "nightly workflow must verify the rolling GitHub nightly release"
    );
    assert!(
        RC_WORKFLOW.contains("Verify published GitHub RC release"),
        "RC workflow must verify the GitHub RC release"
    );
    assert!(
        STABLE_WORKFLOW.contains("Verify published GitHub stable release"),
        "stable workflow must verify the GitHub stable release"
    );
    assert!(
        VERIFY_PUBLISHED_RELEASE_SCRIPT.contains("update-manifest.json"),
        "post-publish verifier must inspect package manifests"
    );
    assert!(
        VERIFY_PUBLISHED_RELEASE_SCRIPT.contains("archive_layout")
            && VERIFY_PUBLISHED_RELEASE_SCRIPT.contains("platform_files")
            && VERIFY_PUBLISHED_RELEASE_SCRIPT.contains("expected_files_for"),
        "post-publish verifier must enforce archive layout from release_contract.toml"
    );
    assert!(
        VERIFY_PUBLISHED_RELEASE_SCRIPT.contains("PortalSurfer catalog sha256 mismatch"),
        "post-publish verifier must compare public catalog hashes to downloaded bytes"
    );
    assert!(
        VERIFY_PUBLISHED_RELEASE_SCRIPT.contains("pkeyutl"),
        "post-publish verifier must verify checksum signatures"
    );
    assert!(
        VERIFY_PUBLISHED_RELEASE_SCRIPT.contains("/gate?donation_amount=0.00")
            && VERIFY_PUBLISHED_RELEASE_SCRIPT.contains("download_token"),
        "PortalSurfer post-publish downloads must verify through the public download gate"
    );
}

#[test]
fn release_workflows_verify_signing_key_before_public_github_publish() {
    for (name, workflow, publish_step) in [
        (
            "nightly workflow",
            NIGHTLY_WORKFLOW,
            "Refresh rolling nightly release assets",
        ),
        ("RC workflow", RC_WORKFLOW, "Publish RC release"),
        ("stable workflow", STABLE_WORKFLOW, "Publish stable release"),
    ] {
        let sign_position = workflow
            .find("scripts/internal/release/sign_release_checksums.sh \\")
            .unwrap_or_else(|| panic!("{name} must sign release checksums"));
        let publish_position = workflow
            .find(publish_step)
            .unwrap_or_else(|| panic!("{name} must publish a public GitHub release"));
        assert!(
            sign_position < publish_position,
            "{name} must sign and verify checksums before public GitHub release publication"
        );

        let pre_publish = &workflow[sign_position..publish_position];
        assert!(
            pre_publish.contains(&format!("--verify-public-key {CHECKSUMS_PUBLIC_KEY}")),
            "{name} must verify the checksum signing key against the pinned public key before public GitHub release publication"
        );
    }
}

#[test]
fn structured_release_log_generator_declares_required_sections() {
    for section in [
        "## Release Metadata",
        "## Artifacts",
        "## Checksums",
        "## Manual Notes",
        "## Generated Changes",
    ] {
        assert!(
            RELEASE_LOG_SCRIPT.contains(section),
            "release log generator must declare {section}"
        );
    }
    assert!(
        RELEASE_LOG_SCRIPT.contains("git cliff"),
        "release log generator must try git-cliff before using the fallback"
    );
    assert!(
        RELEASE_LOG_SCRIPT.contains("git log --no-merges --pretty=format:'- %s'"),
        "release log generator must keep a deterministic commit-list fallback"
    );
}

#[test]
fn release_train_prep_workflow_is_manual_and_explicit_about_branch_pushes() {
    assert!(
        RELEASE_TRAIN_PREP_WORKFLOW.contains("workflow_dispatch:"),
        "release train prep must be a manual workflow"
    );
    assert!(
        RELEASE_TRAIN_PREP_WORKFLOW.contains("push_branch:"),
        "release train prep must require an explicit push_branch input"
    );
    assert!(
        RELEASE_TRAIN_PREP_WORKFLOW.contains("default: false"),
        "release train prep must default to dry-run/no-push"
    );
    assert!(
        RELEASE_TRAIN_PREP_WORKFLOW.contains("scripts/internal/release/prepare_release_train.py"),
        "release train prep workflow must invoke the shared prep script"
    );
    assert!(
        RELEASE_TRAIN_PREP_WORKFLOW.contains("--dry-run"),
        "release train prep workflow must support validation without mutation"
    );
    assert!(
        RELEASE_TRAIN_PREP_WORKFLOW.contains("--push"),
        "release train prep workflow must only push behind an explicit input"
    );
}

#[test]
fn release_train_prep_script_enforces_version_and_package_scope() {
    assert!(
        RELEASE_TRAIN_PREP_SCRIPT.contains("VERSION_RE = re.compile"),
        "prep script must validate MAJOR.MINOR.PATCH versions"
    );
    assert!(
        RELEASE_TRAIN_PREP_SCRIPT.contains("RELEASE_PACKAGE_RE = re.compile"),
        "prep script must derive release package scope from package names"
    );
    assert!(
        RELEASE_TRAIN_PREP_SCRIPT.contains("not VERSION_RE.fullmatch(package.version)"),
        "prep script must reject stale prerelease package versions"
    );
    assert!(
        RELEASE_TRAIN_PREP_SCRIPT.contains("cargo(\"test\", \"--test\", \"release_contract\", \"--test\", \"manual_release_matching\")"),
        "prep script must run focused release contract validation"
    );
    assert!(
        RELEASE_TRAIN_PREP_SCRIPT.contains("validate_lockfile_versions(version)"),
        "prep script must verify Cargo.lock release package versions"
    );
}

#[test]
fn release_workflows_use_shared_setup_build_and_signing_helpers() {
    for (name, workflow) in [
        ("nightly workflow", NIGHTLY_WORKFLOW),
        ("RC workflow", RC_WORKFLOW),
        ("stable workflow", STABLE_WORKFLOW),
    ] {
        assert!(
            workflow.contains("scripts/internal/release/provision_radiant_sibling.sh"),
            "{name} must use the shared Radiant provisioning helper"
        );
        assert!(
            workflow.contains("scripts/internal/release/emit_rust_toolchain_channel.py"),
            "{name} must use the shared Rust toolchain channel helper"
        );
        assert!(
            workflow.contains("scripts/internal/release/setup_ubuntu_release_deps.sh"),
            "{name} must use the shared Ubuntu release dependency helper"
        );
        assert!(
            workflow.contains("scripts/internal/release/run_release_validation.sh"),
            "{name} must use the shared release validation helper"
        );
        assert!(
            workflow.contains("scripts/internal/release/setup_windows_asio_sdk.ps1"),
            "{name} must use the shared Windows ASIO setup helper"
        );
        assert!(
            workflow.contains("scripts/internal/release/build_release_artifact.sh"),
            "{name} must use the shared release artifact build helper"
        );
        assert!(
            workflow.contains("scripts/internal/release/assemble_release_files.sh"),
            "{name} must use the shared release assembly helper"
        );
        assert!(
            workflow.contains("scripts/internal/release/sign_release_checksums.sh"),
            "{name} must use the shared checksum signing helper"
        );
    }
    assert!(
        RELEASE_TRAIN_PREP_WORKFLOW
            .contains("scripts/internal/release/provision_radiant_sibling.sh"),
        "release train prep must use the shared Radiant provisioning helper"
    );
    assert!(
        RELEASE_TRAIN_PREP_WORKFLOW
            .contains("scripts/internal/release/emit_rust_toolchain_channel.py"),
        "release train prep must use the shared Rust toolchain channel helper"
    );
    assert!(
        NIGHTLY_WORKFLOW.contains("scripts/internal/release/prune_github_release_assets.sh"),
        "nightly workflow must use the shared rolling-release asset pruning helper"
    );
}

#[test]
fn standalone_radiant_smoke_workflow_provisions_before_cargo() {
    let provision = RADIANT_SMOKE_WORKFLOW
        .find("scripts/internal/release/provision_radiant_sibling.sh")
        .expect("Radiant smoke must provision the sibling");
    let metadata = RADIANT_SMOKE_WORKFLOW
        .find("cargo metadata --locked")
        .expect("Radiant smoke must validate Cargo metadata");
    assert!(
        provision < metadata,
        "provisioning must precede Cargo validation"
    );
    assert!(RADIANT_SMOKE_WORKFLOW.contains("workflow_dispatch:"));
}

#[test]
fn shared_release_helpers_keep_policy_visible_and_strict() {
    assert!(
        PROVISION_RADIANT_SCRIPT.contains("RADIANT_REPOSITORY_DEPLOY_KEY")
            && PROVISION_RADIANT_SCRIPT.contains("RADIANT_SUBMODULE_DEPLOY_KEY"),
        "Radiant provisioning helper must map the deploy-key secret explicitly"
    );
    assert!(
        BUILD_RELEASE_ARTIFACT_SCRIPT.contains("--channel <nightly|rc|stable>"),
        "artifact build helper must keep release channel explicit"
    );
    assert!(
        BUILD_RELEASE_ARTIFACT_SCRIPT.contains("build_release_zip.sh"),
        "artifact build helper must delegate to the canonical zip packager"
    );
    assert!(
        ASSEMBLE_RELEASE_FILES_SCRIPT.contains("No checksums entry files found"),
        "release assembly helper must fail when checksum entries are missing"
    );
    assert!(
        SIGN_RELEASE_CHECKSUMS_SCRIPT.contains("Missing CHECKSUMS_SIGNING_KEY"),
        "checksum signing helper must fail clearly when the signing key is missing"
    );
    assert!(
        SIGN_RELEASE_CHECKSUMS_SCRIPT.contains("--verify-public-key"),
        "checksum signing helper must preserve public-key verification support"
    );
    assert!(
        PUBLISH_PORTALSURFER_SCRIPT.contains("--channel <nightly|rc|stable>"),
        "PortalSurfer publish helper must keep channel explicit"
    );
    assert!(
        PUBLISH_PORTALSURFER_SCRIPT.contains("X-Wavecrate-Release-Channel"),
        "PortalSurfer publish helper must send channel metadata"
    );
    assert!(
        PUBLISH_PORTALSURFER_SCRIPT.contains("Staging PortalSurfer release file"),
        "PortalSurfer publish helper must report staged upload progress"
    );
    assert!(
        PUBLISH_PORTALSURFER_SCRIPT.contains("Committing PortalSurfer release"),
        "PortalSurfer publish helper must report the public commit step"
    );
    assert!(
        PUBLISH_PORTALSURFER_SCRIPT.contains("assemble_portal_changelog.py"),
        "PortalSurfer publish helper must update the full changelog"
    );
    assert!(
        PUBLISH_PORTALSURFER_SCRIPT
            .contains("Fetched changelog body does not match generated release log"),
        "PortalSurfer publish helper must verify per-release changelog round trips"
    );
    assert!(
        PUBLISH_PORTALSURFER_SCRIPT
            .contains("Fetched full changelog body does not match generated changelog"),
        "PortalSurfer publish helper must verify full changelog round trips"
    );
    assert!(
        VALIDATE_PROMOTED_RC_SCRIPT.contains("gh\","),
        "promoted RC validator must use GitHub CLI for live release validation"
    );
    assert!(
        VALIDATE_PROMOTED_RC_SCRIPT.contains("release_contract.toml"),
        "promoted RC validator must derive expected assets from the release contract"
    );
    assert!(
        VALIDATE_PROMOTED_RC_SCRIPT.contains("RC release is missing required assets"),
        "promoted RC validator must fail clearly when release assets are incomplete"
    );
    assert!(
        VALIDATE_PROMOTED_RC_SCRIPT.contains("Checksum mismatch"),
        "promoted RC validator must compare checksum entries to downloaded zip hashes"
    );
}

fn parse_contract() -> Value {
    RELEASE_CONTRACT
        .parse::<Value>()
        .expect("release_contract.toml parses")
}

fn app_name(contract: &Value) -> &str {
    contract
        .get("app_name")
        .and_then(Value::as_str)
        .expect("app_name")
}

fn contract_targets(contract: &Value) -> Vec<String> {
    contract
        .get("targets")
        .and_then(Value::as_array)
        .expect("targets")
        .iter()
        .map(|target| target.as_str().expect("target triple").to_string())
        .collect()
}

fn platform_labels(contract: &Value) -> BTreeSet<String> {
    contract
        .get("platform_labels")
        .and_then(Value::as_table)
        .expect("platform_labels")
        .values()
        .map(|label| label.as_str().expect("label string").to_string())
        .collect()
}

fn active_labels_for_targets(targets: &[String]) -> BTreeSet<String> {
    active_target_matrix(targets)
        .into_iter()
        .flat_map(|target| [target.platform, target.arch])
        .collect()
}

fn active_target_matrix(targets: &[String]) -> BTreeSet<ReleaseTarget> {
    targets
        .iter()
        .map(|target| ReleaseTarget {
            target: target.clone(),
            platform: platform_for_target(target),
            arch: arch_for_target(target),
        })
        .collect()
}

fn platform_for_target(target: &str) -> String {
    if target.contains("-pc-windows-") {
        return "windows".to_string();
    }
    if target.ends_with("-apple-darwin") {
        return "macos".to_string();
    }
    if target.contains("-unknown-linux-") {
        return "linux".to_string();
    }
    panic!("unsupported release target triple: {target}");
}

fn arch_for_target(target: &str) -> String {
    target
        .split_once('-')
        .map(|(arch, _)| arch.to_string())
        .expect("target arch")
}

fn apply_asset_template(
    template: &str,
    app_name: &str,
    target: &ReleaseTarget,
    version: &str,
    rc_number: &str,
) -> String {
    template
        .replace("{APP_NAME}", app_name)
        .replace("{version}", version)
        .replace("{rc_number}", rc_number)
        .replace("{platform}", &target.platform)
        .replace("{arch}", &target.arch)
}

fn workflow_release_targets(workflow: &str) -> BTreeSet<ReleaseTarget> {
    let mut targets = BTreeSet::new();
    let mut entry = BTreeMap::new();
    let mut in_matrix_include = false;

    for line in workflow.lines() {
        let trimmed = line.trim();
        if trimmed == "include:" {
            in_matrix_include = true;
            continue;
        }
        if !in_matrix_include {
            continue;
        }
        if trimmed == "steps:" {
            break;
        }
        if trimmed.starts_with("- os:") {
            insert_workflow_entry(&mut targets, &mut entry);
            continue;
        }
        if let Some((key, value)) = trimmed.split_once(':') {
            let value = value.trim();
            if matches!(key, "target" | "platform" | "arch") && !value.is_empty() {
                entry.insert(key.to_string(), value.to_string());
            }
        }
    }
    insert_workflow_entry(&mut targets, &mut entry);

    targets
}

fn assert_job_timeout(workflow_name: &str, workflow: &str, job: &str, minutes: &str) {
    let block = workflow_job_block(workflow, job);
    assert!(
        block.contains(&format!("timeout-minutes: {minutes}")),
        "{workflow_name} job {job} must declare timeout-minutes: {minutes}"
    );
}

fn workflow_job_block(workflow: &str, job: &str) -> String {
    let marker = format!("  {job}:");
    let mut found = false;
    let mut block = Vec::new();
    for line in workflow.lines() {
        if line == marker {
            found = true;
            block.push(line);
            continue;
        }
        if found && line.starts_with("  ") && !line.starts_with("    ") && line.ends_with(':') {
            break;
        }
        if found {
            block.push(line);
        }
    }
    assert!(found, "workflow job {job} must exist");
    block.join("\n")
}

fn insert_workflow_entry(
    targets: &mut BTreeSet<ReleaseTarget>,
    entry: &mut BTreeMap<String, String>,
) {
    let Some(target) = entry.remove("target") else {
        entry.clear();
        return;
    };
    let platform = entry.remove("platform").expect("workflow platform");
    let arch = entry.remove("arch").expect("workflow arch");
    targets.insert(ReleaseTarget {
        target,
        platform,
        arch,
    });
    entry.clear();
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ReleaseTarget {
    target: String,
    platform: String,
    arch: String,
}
