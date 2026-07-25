//! Source-quality guardrails for the Wavecrate/Radiant GUI boundary.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

struct FacadeBudget {
    path: &'static str,
    max_lines: usize,
    max_exports: usize,
    owner: &'static str,
}

const WAVECRATE_FACADE_BUDGETS: &[FacadeBudget] = &[
    FacadeBudget {
        path: "src/native_app/sample_library/folder_browser.rs",
        max_lines: 180,
        max_exports: 32,
        owner: "OPT-541: folder-browser root stays a thin module/export facade",
    },
    FacadeBudget {
        path: "src/app_core/app_api.rs",
        max_lines: 150,
        max_exports: 28,
        owner: "OPT-541: app-api remains the owned legacy-crossing allowlist",
    },
    FacadeBudget {
        path: "src/app_core/actions/mod.rs",
        max_lines: 240,
        max_exports: 66,
        owner: "OPT-541: app-core action facade exports remain deliberate",
    },
    FacadeBudget {
        path: "src/native_app/test_support.rs",
        max_lines: 40,
        max_exports: 12,
        owner: "OPT-541: test support root stays a test-only re-export facade",
    },
];

#[test]
fn wavecrate_does_not_reintroduce_local_radiant_gui_prelude() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let prelude_path = Path::new(manifest_dir).join("src/ui_primitives/mod.rs");
    assert!(
        !prelude_path.exists(),
        "Wavecrate should not maintain a local Radiant GUI prelude; import radiant::prelude or explicit radiant::gui subsystems instead"
    );

    let source_root = Path::new(manifest_dir).join("src");
    let mut offenders = Vec::new();
    collect_matching_source_lines(
        &source_root,
        Path::new(manifest_dir),
        &mut offenders,
        |line| line.contains("crate::ui_primitives") || line.contains("ui_primitives::"),
    );
    assert!(
        offenders.is_empty(),
        "Wavecrate code should import Radiant directly instead of using a local ui_primitives facade:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn target_docs_call_out_large_gui_import_lists() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let source = fs::read_to_string(format!("{manifest_dir}/docs/TARGET.md"))
        .expect("docs/TARGET.md should be readable");

    for required in [
        "large import lists are architecture signals",
        "split the module by responsibility",
        "move reusable",
        "GUI behavior into Radiant",
        "avoid wildcard imports",
    ] {
        assert!(
            source.contains(required),
            "docs/TARGET.md should preserve the GUI import hygiene rule: missing `{required}`"
        );
    }
}

#[test]
fn source_processing_supervisor_uses_backend_neutral_events() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let source = fs::read_to_string(format!(
        "{manifest_dir}/src/native_app/source_processing/supervisor.rs"
    ))
    .expect("source-processing supervisor should be readable");

    assert!(
        !source.contains("GuiMessage"),
        "the source-processing supervisor must not depend on native GUI messages"
    );
    assert!(
        !source.contains("SourceProcessingProgress {"),
        "the source-processing supervisor must not construct native progress DTOs"
    );
    assert!(
        source.contains("SourceProcessingEventSink")
            && source.contains("SourceProcessingEvent::Progress"),
        "the source-processing supervisor must publish through the typed event boundary"
    );
}

#[test]
fn agent_instructions_call_out_large_gui_import_lists() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let source = fs::read_to_string(format!("{manifest_dir}/AGENTS.md"))
        .expect("AGENTS.md should be readable");

    for required in [
        "large import lists are architecture signals",
        "split the module by responsibility",
        "move reusable",
        "GUI behavior into Radiant",
        "facade may wire",
    ] {
        assert!(
            source.contains(required),
            "AGENTS.md should preserve the GUI import hygiene rule: missing `{required}`"
        );
    }
}

#[test]
fn sample_identity_fingerprints_require_wavecrate_debug_mode() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let source = fs::read_to_string(format!(
        "{manifest_dir}/src/native_app/sample_identity_diagnostics.rs"
    ))
    .expect("sample identity diagnostics should be readable");
    assert!(
        source.contains("fn sample_identity_info_enabled() -> bool"),
        "sample identity diagnostics should keep one central enablement gate"
    );
    assert!(
        source.contains("wavecrate::logging::debug_logging_enabled()")
            && source.contains("WAVECRATE_SAMPLE_IDENTITY_DIAGNOSTICS")
            && source.contains("sample_identity_diagnostics_enabled()")
            && source.contains(
                "tracing::enabled!(target: \"wavecrate::debug::sample_identity\", tracing::Level::INFO)",
            ),
        "sample identity diagnostics compute file/waveform fingerprints and must require explicit diagnostic opt-in, not plain info logging"
    );
}

#[test]
fn starmap_drag_hot_path_does_not_fingerprint_sample_identity() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let source = fs::read_to_string(format!(
        "{manifest_dir}/src/native_app/audio/sample_load_actions/entrypoints.rs"
    ))
    .expect("sample-load entrypoints should be readable");
    let body = source_between(
        &source,
        "pub(in crate::native_app) fn start_starmap_drag_audition_sample",
        "pub(in crate::native_app) fn promote_starmap_audition_sample",
    );

    assert!(
        !body.contains("log_sample_identity_checkpoint"),
        "starmap drag audition is a pointer hot path; use perf::starmap_drag telemetry instead of sample identity fingerprint diagnostics"
    );
}

#[test]
fn selection_navigation_hot_paths_do_not_fingerprint_sample_identity() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let source = fs::read_to_string(format!(
        "{manifest_dir}/src/native_app/audio/sample_load_actions/entrypoints.rs"
    ))
    .expect("sample-load entrypoints should be readable");

    for (name, start, end) in [
        (
            "select_sample",
            "pub(in crate::native_app) fn select_sample",
            "pub(in crate::native_app) fn select_sample_with_modifiers",
        ),
        (
            "select_sample_with_modifiers",
            "pub(in crate::native_app) fn select_sample_with_modifiers",
            "pub(in crate::native_app) fn start_starmap_drag_audition_sample",
        ),
        (
            "load_navigation_sample_validated",
            "pub(in crate::native_app) fn load_navigation_sample_validated",
            "fn queue_sample_load_path_validation",
        ),
        (
            "queue_sample_load_path_validation",
            "fn queue_sample_load_path_validation",
            "pub(in crate::native_app) fn finish_sample_load_path_validation",
        ),
    ] {
        let body = source_between(&source, start, end);
        assert!(
            !body.contains("log_sample_identity"),
            "{name} is a fast navigation path; sample identity diagnostics fingerprint files and must stay out of the UI-path handoff"
        );
    }
}

fn source_between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    source
        .split(start)
        .nth(1)
        .and_then(|tail| tail.split(end).next())
        .unwrap_or_else(|| panic!("expected source range {start:?}..{end:?}"))
}

#[test]
fn wavecrate_root_facades_stay_within_owned_size_budgets() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    for budget in WAVECRATE_FACADE_BUDGETS {
        let path = manifest_dir.join(budget.path);
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("{} should be readable: {err}", path.display()));
        let line_count = source.lines().count();
        let export_count = source.lines().filter(|line| is_export_line(line)).count();

        assert!(
            line_count <= budget.max_lines,
            "{} has {line_count} lines; budget is {}. {}",
            budget.path,
            budget.max_lines,
            budget.owner
        );
        assert!(
            export_count <= budget.max_exports,
            "{} has {export_count} export lines; budget is {}. {}",
            budget.path,
            budget.max_exports,
            budget.owner
        );
    }
}

#[test]
fn production_app_core_legacy_crossings_go_through_app_api() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let app_core_root = manifest_dir.join("src/app_core");
    let mut offenders = Vec::new();
    collect_app_core_legacy_crossings(&app_core_root, &manifest_dir, &mut offenders);

    assert!(
        offenders.is_empty(),
        "production app-core code must import legacy app modules through src/app_core/app_api.rs:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn app_api_state_dto_inventory_points_at_active_followup() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let source = fs::read_to_string(format!("{manifest_dir}/src/app_core/app_api.rs"))
        .expect("src/app_core/app_api.rs should be readable");

    assert!(
        !source.contains("OPT-676"),
        "app_api state DTO cleanup must not point at OPT-676; that issue now tracks completed browser tag-sidebar projection work"
    );
    assert!(
        source.contains("Browser/source/map/audio state DTOs") && source.contains("| `OPT-677` |"),
        "app_api migration inventory should map remaining legacy state DTO exports to OPT-677"
    );
    assert!(
        source.contains("while OPT-677 replaces legacy state usage"),
        "test-only legacy state DTO exports should mention the active OPT-677 follow-up"
    );
}

#[test]
fn production_native_app_modules_do_not_import_test_support_facade() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let native_app_root = manifest_dir.join("src/native_app");
    let mut offenders = Vec::new();
    collect_native_app_test_support_imports(&native_app_root, &manifest_dir, &mut offenders);

    assert!(
        offenders.is_empty(),
        "production native-app modules must not import the cfg(test) test_support facade:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn production_gui_modules_do_not_use_top_level_wildcard_imports() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let gui_root = manifest_dir.join("src/native_app/app_chrome");
    let mut offenders = Vec::new();
    collect_top_level_wildcard_imports(&gui_root, &mut offenders);

    assert!(
        offenders.is_empty(),
        "production GUI modules should use explicit imports instead of top-level wildcard imports:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn cross_crate_public_wildcard_reexports_are_explicitly_audited() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source_root = manifest_dir.join("src");
    let mut actual = BTreeSet::new();
    collect_cross_crate_public_wildcard_reexports(&source_root, &manifest_dir, &mut actual);

    let expected: BTreeSet<String> = BTreeSet::new();

    assert_eq!(
        actual, expected,
        "cross-crate public wildcard re-exports must be audited compatibility shims; \
         narrow the export or add an explicit ownership note before updating this list"
    );
}

#[test]
fn native_app_ui_update_paths_do_not_call_blocking_business_apis() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let native_app_root = manifest_dir.join("src/native_app");

    wavecrate_non_blocking_guardrail()
        .scan_roots([native_app_root])
        .expect("native-app UI/update paths must offload filesystem, database, thread, sleep, clipboard, and other blocking business work through Radiant BusinessRuntime or a platform service");
}

#[test]
fn native_app_blocking_helper_modules_stay_owned_by_business_workers() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let native_app_root = manifest_dir.join("src/native_app");
    let mut exclusive_transfer_consumers = BTreeSet::new();
    let mut atomic_write_consumers = BTreeSet::new();

    for_rust_source_file(&native_app_root, &mut |path| {
        if is_test_source(path) {
            return;
        }
        let source = read_source(path);
        let relative = path
            .strip_prefix(&manifest_dir)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        if source.contains("sample_library::exclusive_file_transfer::") {
            exclusive_transfer_consumers.insert(relative.clone());
        }
        if source.contains("mod atomic_write;") || source.contains("self::atomic_write::") {
            atomic_write_consumers.insert(relative);
        }
    });

    assert_eq!(
        exclusive_transfer_consumers,
        BTreeSet::from([
            String::from("src/native_app/sample_library/folder_browser/file_move_transaction.rs"),
            String::from("src/native_app/sample_library/native_file_drop_actions.rs"),
        ]),
        "exclusive transfer helpers must stay behind file-operation workers or the typed native-drop background handoff"
    );
    assert_eq!(
        atomic_write_consumers,
        BTreeSet::from([String::from("src/native_app/waveform_edits/worker.rs")]),
        "failure-atomic WAV replacement must remain private to the destructive-edit worker"
    );

    let compact = |source: &str| source.split_whitespace().collect::<String>();
    let destructive_queue = compact(&read_source(
        &native_app_root.join("waveform_edits/queue.rs"),
    ));
    assert!(
        destructive_queue.contains(
            ".business().blocking_io(WAVEFORM_DESTRUCTIVE_EDIT_TASK_NAME).latest(&mutself.background.waveform_destructive_edit_task).run(move|_|worker::execute_destructive_edit(worker_request),GuiMessage::WaveformDestructiveEditFinished,)"
        ),
        "destructive-edit execution must remain the closure passed to the blocking worker handoff"
    );

    let native_drop = compact(&read_source(
        &native_app_root.join("sample_library/native_file_drop_actions.rs"),
    ));
    assert!(
        native_drop.contains(
            ".business().background(\"gui-external-waveform-drop\").run(move|_|execute_external_waveform_file_drop(&source,&target_folder),"
        ),
        "exclusive external-drop transfer must remain the closure passed to its background handoff"
    );

    let folder_moves = compact(&read_source(
        &native_app_root.join("sample_library/drag_drop_actions/folder_moves.rs"),
    ));
    assert!(
        folder_moves.contains(
            ".business().background(\"gui-folder-browser-move\").stream_latest(move|_context,events|{execute_folder_move_request_with_progress(request,task_id,move|progress|{events.emit(progress)})},"
        ),
        "exclusive folder-browser transfers must remain inside the streamed background worker handoff"
    );
}

#[test]
fn native_app_blocking_worker_allowlist_uses_exact_file_and_tree_boundaries() {
    let root = std::env::temp_dir().join(format!(
        "wavecrate_worker_allowlist_boundary_fixture_{}",
        std::process::id()
    ));
    let native_app_root = root.join("src/native_app");
    let fixtures = [
        (
            "sample_library/exclusive_file_transfer.rs",
            "std::fs::read(\"allowed-root.wav\").ok();",
        ),
        (
            "sample_library/exclusive_file_transfer/platform.rs",
            "std::fs::read(\"allowed-child.wav\").ok();",
        ),
        (
            "sample_library/exclusive_file_transfer_shadow.rs",
            "std::fs::read(\"shadow-transfer.wav\").ok();",
        ),
        (
            "waveform_edits/worker.rs",
            "std::fs::read(\"allowed-worker.wav\").ok();",
        ),
        (
            "waveform_edits/worker/atomic_write.rs",
            "std::fs::read(\"allowed-worker-child.wav\").ok();",
        ),
        (
            "waveform_edits/worker_shadow.rs",
            "std::fs::read(\"shadow-worker.wav\").ok();",
        ),
    ];
    for (relative, body) in fixtures {
        let path = native_app_root.join(relative);
        fs::create_dir_all(path.parent().expect("fixture path has parent"))
            .expect("create worker allowlist fixture dir");
        fs::write(path, format!("fn fixture() {{ {body} }}\n"))
            .expect("write worker allowlist fixture");
    }

    let report = wavecrate_non_blocking_guardrail()
        .scan_roots([&native_app_root])
        .expect_err("similarly prefixed worker siblings must remain guarded");
    let violations = report
        .violations
        .iter()
        .map(|violation| violation.source_line.as_str())
        .collect::<Vec<_>>();
    assert_eq!(violations.len(), 2, "unexpected violations: {report:?}");
    assert!(
        violations
            .iter()
            .any(|line| line.contains("shadow-transfer"))
    );
    assert!(violations.iter().any(|line| line.contains("shadow-worker")));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn native_app_blocking_guardrail_skips_cfg_test_fixture_blocks_only() {
    let root = std::env::temp_dir().join(format!(
        "wavecrate_cfg_test_guardrail_fixture_{}",
        std::process::id()
    ));
    let source = root.join("ui_action.rs");
    fs::create_dir_all(&root).expect("create guardrail fixture dir");
    fs::write(
        &source,
        r#"fn update() {
    let value = 1;
}

#[cfg(test)]
mod tests {
    fn writes_fixture_file() {
        std::fs::write("sample.wav", []).ok();
    }
}

fn production_after_tests() {
    std::fs::read("sample.wav").ok();
}
"#,
    )
    .expect("write guardrail fixture");

    let report = wavecrate_non_blocking_guardrail()
        .scan_roots([&root])
        .expect_err("production read after cfg(test) module should still be reported");

    assert_eq!(report.violations.len(), 1);
    assert!(
        report.violations[0].source_line.contains("std::fs::read"),
        "production code after cfg(test) fixture should still be reported: {report:?}"
    );
    assert!(
        report
            .violations
            .iter()
            .all(|violation| !violation.source_line.contains("std::fs::write")),
        "cfg(test) fixture internals should be ignored: {report:?}"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn waveform_clipboard_staging_worker_does_not_touch_platform_clipboard() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source_path =
        manifest_dir.join("src/native_app/sample_library/drag_drop_actions/external.rs");
    let source = fs::read_to_string(&source_path)
        .unwrap_or_else(|err| panic!("{} should be readable: {err}", source_path.display()));
    let worker_start = source
        .find(".interactive(WAVEFORM_CLIPBOARD_HANDOFF_TASK_NAME)")
        .unwrap_or_else(|| {
            panic!(
                "{} should contain waveform clipboard handoff task",
                source_path.display()
            )
        });
    let worker_end = source[worker_start..]
        .find(";\n        true")
        .map(|offset| worker_start + offset)
        .unwrap_or(source.len());
    let worker_source = &source[worker_start..worker_end];

    for forbidden in [
        "external_clipboard::",
        "copy_file_paths(",
        "read_file_paths(",
    ] {
        assert!(
            !worker_source.contains(forbidden),
            "waveform clip staging may prepare the file on a worker, but platform clipboard handoff must stay on Radiant's typed platform service: found `{forbidden}`"
        );
    }
}

#[test]
fn external_drag_adapter_has_macos_appkit_file_url_session() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let module_path = manifest_dir.join("src/external_drag/mod.rs");
    let macos_path = manifest_dir.join("src/external_drag/platform_macos.rs");
    let module = fs::read_to_string(&module_path)
        .unwrap_or_else(|err| panic!("{} should be readable: {err}", module_path.display()));
    let macos = fs::read_to_string(&macos_path)
        .unwrap_or_else(|err| panic!("{} should be readable: {err}", macos_path.display()));

    assert!(
        module.contains("#[cfg(target_os = \"macos\")]")
            && module.contains("#[path = \"platform_macos.rs\"]")
            && module.contains("platform::start_file_drag(paths)"),
        "external drag module should route macOS file drags to the AppKit platform adapter"
    );
    for required_contract in [
        "beginDraggingSessionWithItems:event:source:",
        "fileURLWithPath:",
        "initWithPasteboardWriter:",
        "NSApplication has no key or main window",
        "NSWindow contentView returned nil",
        "External drag-out is only supported on Windows and macOS",
    ] {
        assert!(
            module.contains(required_contract) || macos.contains(required_contract),
            "macOS external drag support should keep contract `{required_contract}`"
        );
    }
}

#[test]
fn legacy_selection_export_completion_launches_external_drag_on_macos() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let completion_path =
        manifest_dir.join("src/app/controller/library/selection_export/completion.rs");
    let action_path =
        manifest_dir.join("src/app/controller/ui/drag_drop_controller/actions/external_drag.rs");
    let effects_path = manifest_dir
        .join("src/app/controller/ui/drag_drop_controller/drag_effects/external_drag.rs");
    let bridge_path = manifest_dir.join("src/app_core/ui_bridge/native_bridge.rs");
    let completion = fs::read_to_string(&completion_path)
        .unwrap_or_else(|err| panic!("{} should be readable: {err}", completion_path.display()));
    let actions = fs::read_to_string(&action_path)
        .unwrap_or_else(|err| panic!("{} should be readable: {err}", action_path.display()));
    let effects = fs::read_to_string(&effects_path)
        .unwrap_or_else(|err| panic!("{} should be readable: {err}", effects_path.display()));
    let bridge = fs::read_to_string(&bridge_path)
        .unwrap_or_else(|err| panic!("{} should be readable: {err}", bridge_path.display()));

    let platform_cfg = "#[cfg(any(target_os = \"windows\", target_os = \"macos\"))]";
    assert!(
        completion.contains(platform_cfg)
            && completion.contains("finish_external_selection_drag_export")
            && !completion.contains(
                "#[cfg(not(target_os = \"windows\"))]\n    fn finish_external_selection_drag_export(&mut self, _success"
            ),
        "selection export completion must launch external drags on macOS instead of no-oping"
    );
    assert!(
        actions.contains(platform_cfg)
            && actions.contains("pub(crate) fn maybe_launch_external_drag"),
        "external drag launch polling should be compiled for both Windows and macOS"
    );
    assert!(
        effects.contains("#[cfg(target_os = \"macos\")]")
            && effects.contains("crate::external_drag::start_file_drag((), paths)"),
        "drag controller should call the macOS external drag adapter"
    );
    assert!(
        bridge.contains(platform_cfg) && bridge.contains("maybe_launch_external_drag"),
        "native bridge should forward external drag polling on macOS"
    );
}

#[test]
fn native_app_playback_paths_do_not_start_audio_directly() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let native_app_root = manifest_dir.join("src/native_app");
    let mut offenders = Vec::new();
    collect_matching_source_lines(&native_app_root, &manifest_dir, &mut offenders, |line| {
        let code = line.trim();
        !is_comment_or_empty(code)
            && (code.contains("set_audio_samples_with_metadata(")
                || code.contains("set_audio_with_metadata(")
                || code.contains("set_interleaved_f32_file_with_metadata(")
                || code.contains(".play_range(")
                || code.contains(".play_looped_range_from("))
    });

    assert!(
        offenders.is_empty(),
        "native-app playback start paths must submit neutral requests to Reson's playback runtime instead of preparing or starting AudioPlayer directly:\n{}",
        offenders.join("\n")
    );
}

fn collect_app_core_legacy_crossings(dir: &Path, manifest_dir: &Path, offenders: &mut Vec<String>) {
    for_rust_source_file(dir, &mut |path| {
        if is_test_source(path) || path.ends_with("app_api.rs") {
            return;
        }
        collect_matching_lines(path, manifest_dir, offenders, app_core_legacy_crossing);
    });
}

fn collect_native_app_test_support_imports(
    dir: &Path,
    manifest_dir: &Path,
    offenders: &mut Vec<String>,
) {
    for_rust_source_file(dir, &mut |path| {
        if is_test_source(path) || path.ends_with("test_support.rs") {
            return;
        }
        collect_matching_lines(
            path,
            manifest_dir,
            offenders,
            native_app_test_support_import,
        );
    });
}

fn collect_top_level_wildcard_imports(dir: &Path, offenders: &mut Vec<String>) {
    for_rust_source_file(dir, &mut |path| {
        if is_test_source(path) {
            return;
        }
        let source = read_source(path);
        for (line_index, line) in source.lines().enumerate() {
            if line.starts_with("use super::*") {
                let relative = path
                    .strip_prefix(env!("CARGO_MANIFEST_DIR"))
                    .unwrap_or(path);
                offenders.push(format!("{}:{}", relative.display(), line_index + 1));
            }
        }
    });
}

fn collect_cross_crate_public_wildcard_reexports(
    dir: &Path,
    manifest_dir: &Path,
    actual: &mut BTreeSet<String>,
) {
    for_rust_source_file(dir, &mut |path| {
        let source = read_source(path);
        for line in source
            .lines()
            .filter_map(cross_crate_public_wildcard_target)
        {
            let relative = path.strip_prefix(manifest_dir).unwrap_or(path);
            actual.insert(format!("{}:{line}", relative.display()).replace('\\', "/"));
        }
    });
}

fn for_rust_source_file(dir: &Path, visit: &mut impl FnMut(&Path)) {
    for entry in fs::read_dir(dir).unwrap_or_else(|err| panic!("{dir:?} should be readable: {err}"))
    {
        let entry = entry.expect("source directory entry should be readable");
        let path = entry.path();
        if path.is_dir() {
            for_rust_source_file(&path, visit);
            continue;
        }
        if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
            continue;
        }
        visit(&path);
    }
}

fn collect_matching_lines(
    path: &Path,
    manifest_dir: &Path,
    offenders: &mut Vec<String>,
    matches_line: impl Fn(&str) -> bool,
) {
    let source = read_source(path);
    let mut previous_line_was_cfg_test = false;
    for (line_index, line) in source.lines().enumerate() {
        let cfg_test_line = is_cfg_test_line(line);
        if matches_line(line) && !previous_line_was_cfg_test {
            let relative = path.strip_prefix(manifest_dir).unwrap_or(path);
            offenders.push(format!(
                "{}:{}: {}",
                relative.display(),
                line_index + 1,
                line.trim()
            ));
        }
        previous_line_was_cfg_test = cfg_test_line;
    }
}

fn collect_matching_source_lines(
    dir: &Path,
    manifest_dir: &Path,
    offenders: &mut Vec<String>,
    matches_line: impl Copy + Fn(&str) -> bool,
) {
    for_rust_source_file(dir, &mut |path| {
        collect_matching_lines(path, manifest_dir, offenders, matches_line);
    });
}

fn read_source(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("{} should be readable: {err}", path.display()))
}

fn app_core_legacy_crossing(line: &str) -> bool {
    let code = line.trim();
    !is_comment_or_empty(code)
        && (code.contains("crate::app::controller")
            || code.contains("crate::app::state")
            || code.contains("crate::app::view_model"))
}

fn native_app_test_support_import(line: &str) -> bool {
    let code = line.trim();
    !is_comment_or_empty(code)
        && (code.contains("native_app::test_support")
            || code.contains("super::test_support")
            || code.contains("test_support::"))
}

fn is_export_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("pub use ")
        || trimmed.starts_with("pub(crate) use ")
        || trimmed.starts_with("pub(in ")
        || trimmed.starts_with("pub const ")
        || trimmed.starts_with("pub(crate) const ")
        || trimmed.starts_with("pub type ")
        || trimmed.starts_with("pub(crate) type ")
}

fn wavecrate_non_blocking_guardrail() -> WavecrateNonBlockingGuardrail {
    let mut guardrail = WavecrateNonBlockingGuardrail::app_update_paths()
        .forbid_token(
            "open_for_user_metadata_write",
            "metadata database write",
            "schedule metadata persistence through context.business()",
        )
        .forbid_token(
            "external_clipboard::",
            "direct clipboard access",
            "request clipboard work through a typed Radiant platform service",
        )
        .forbid_token(
            ".canonicalize(",
            "filesystem path resolution",
            "schedule filesystem work through context.business()",
        )
        .forbid_token(
            ".is_file(",
            "filesystem metadata check",
            "schedule filesystem work through context.business()",
        )
        .forbid_token(
            ".is_dir(",
            "filesystem metadata check",
            "schedule filesystem work through context.business()",
        )
        .forbid_token(
            ".exists(",
            "filesystem metadata check",
            "schedule filesystem work through context.business()",
        )
        .forbid_token(
            "fs::metadata(",
            "filesystem metadata check",
            "schedule filesystem work through context.business()",
        )
        .forbid_token(
            "fs::read(",
            "filesystem read",
            "schedule filesystem work through context.business()",
        )
        .forbid_token(
            "fs::read_dir(",
            "filesystem directory scan",
            "schedule filesystem work through context.business()",
        )
        .forbid_token(
            "fs::read_to_string(",
            "filesystem read",
            "schedule filesystem work through context.business()",
        )
        .forbid_token(
            "fs::write(",
            "filesystem write",
            "schedule filesystem work through context.business()",
        )
        .forbid_token(
            "fs::create_dir",
            "filesystem write",
            "schedule filesystem work through context.business()",
        )
        .forbid_token(
            "fs::remove_file(",
            "filesystem write",
            "schedule filesystem work through context.business()",
        )
        .forbid_token(
            "fs::remove_dir",
            "filesystem write",
            "schedule filesystem work through context.business()",
        )
        .forbid_token(
            "fs::rename(",
            "filesystem write",
            "schedule filesystem work through context.business()",
        )
        .forbid_token(
            "fs::copy(",
            "filesystem write",
            "schedule filesystem work through context.business()",
        )
        .forbid_token(
            "cached_waveform_file_exists(",
            "persisted waveform cache metadata probe",
            "schedule cache probing through context.business()",
        )
        .forbid_token(
            "cached_waveform_file_playback_ready_exists(",
            "persisted waveform cache metadata probe",
            "schedule cache probing through context.business()",
        );

    for (fragment, reason) in [
        ("/tests/", "native app behavior tests"),
        ("tests.rs", "native app behavior tests"),
        ("src/native_app/tests.rs", "native app behavior tests"),
        ("src/native_app/tests", "native app behavior tests"),
        (
            "src/native_app/app/state/source_scan_worker.rs",
            "source scan worker boundary",
        ),
        (
            "src/native_app/audio/sample_load_actions/cache/workers.rs",
            "waveform cache business worker",
        ),
        (
            "src/native_app/audio/sample_load_actions/deferred_drop.rs",
            "deferred file-drop worker boundary",
        ),
        (
            "src/native_app/audio/sample_load_actions/validation_worker.rs",
            "sample load path validation worker",
        ),
        (
            "src/native_app/audio/playback_history/worker.rs",
            "last-played persistence worker",
        ),
        (
            "src/native_app/audio/normalization_worker_pacing.rs",
            "normalization business worker pacing",
        ),
        (
            "src/native_app/file_actions/wav_normalize.rs",
            "audio file worker",
        ),
        (
            "src/native_app/metadata/persistence.rs",
            "metadata persistence worker",
        ),
        (
            "src/native_app/sample_library/file_actions/wav_normalize.rs",
            "audio file worker",
        ),
        (
            "src/native_app/sample_identity_diagnostics.rs",
            "explicit opt-in sample identity diagnostics boundary",
        ),
        (
            "src/native_app/sample_library/drag_drop_actions/external/clipboard_clip.rs",
            "waveform clipboard clip staging worker",
        ),
        (
            "src/native_app/sample_library/exclusive_file_transfer.rs",
            "exclusive file-transfer implementation module root",
        ),
        (
            "src/native_app/sample_library/exclusive_file_transfer/",
            "exclusive file-transfer implementation module children",
        ),
        (
            "src/native_app/sample_library/folder_scan_actions/filesystem_refresh_worker.rs",
            "source database filesystem-sync worker",
        ),
        (
            "src/native_app/sample_library/folder_scan_actions/rating_decay_worker.rs",
            "rating-decay metadata maintenance worker",
        ),
        (
            "src/native_app/sample_library/folder_browser/file_move_execution.rs",
            "file operation worker",
        ),
        (
            "src/native_app/sample_library/folder_browser/file_move_transaction.rs",
            "file operation transaction worker",
        ),
        (
            "src/native_app/sample_library/folder_browser/drag_drop_relocation.rs",
            "drag/drop relocation persistence worker",
        ),
        (
            "src/native_app/sample_library/folder_browser/collections/assignment.rs",
            "collection persistence worker",
        ),
        (
            "src/native_app/sample_library/folder_browser/delete_workflow.rs",
            "delete workflow worker",
        ),
        (
            "src/native_app/sample_library/folder_browser/filesystem_refresh.rs",
            "filesystem refresh worker",
        ),
        (
            "src/native_app/sample_library/folder_browser/rename_execution.rs",
            "rename worker",
        ),
        (
            "src/native_app/sample_library/folder_browser/scanning.rs",
            "source scanning worker",
        ),
        (
            "src/native_app/sample_library/folder_browser/scanning/",
            "source scanning worker helpers",
        ),
        (
            "src/native_app/sample_library/folder_browser/scanning/file_entry_metadata.rs",
            "source scanning metadata worker",
        ),
        (
            "src/native_app/sample_library/folder_create_actions/worker.rs",
            "folder creation worker",
        ),
        (
            "src/native_app/sample_library/context_menu_target/validation_worker.rs",
            "context-menu target validation worker",
        ),
        (
            "src/native_app/sample_library/committed_file_mutations/worker.rs",
            "committed file-mutation reconciliation worker",
        ),
        (
            "src/native_app/sample_library/committed_file_mutations/watcher_echo.rs",
            "committed mutation watcher identity worker helper",
        ),
        (
            "src/native_app/sample_library/folder_browser/source_scan_cache.rs",
            "source scan cache worker",
        ),
        (
            "src/native_app/sample_library/drag_drop_actions/external.rs",
            "typed external drag platform handoff",
        ),
        (
            "src/native_app/sample_library/native_file_drop_actions.rs",
            "typed native file-drop platform handoff",
        ),
        (
            "src/native_app/sample_library/native_file_open_actions/validation_worker.rs",
            "native file-open validation worker",
        ),
        (
            "src/native_app/sample_library/sample_collections/persistence.rs",
            "collection persistence worker",
        ),
        (
            "src/native_app/sample_library/sample_ratings.rs",
            "rating persistence scheduling boundary",
        ),
        (
            "src/native_app/sample_library/similarity_artifacts/worker",
            "similarity artifact publication worker",
        ),
        (
            "src/native_app/sample_library/similarity_scores.rs",
            "similarity score lookup worker",
        ),
        (
            "src/native_app/sample_library/source_watcher/classification.rs",
            "source watcher worker",
        ),
        (
            "src/native_app/sample_library/source_watcher/handle.rs",
            "source watcher worker",
        ),
        (
            "src/native_app/sample_library/source_watcher/journal.rs",
            "durable source watcher journal worker",
        ),
        (
            "src/native_app/sample_library/source_watcher/roots.rs",
            "source watcher worker",
        ),
        (
            "src/native_app/source_processing/supervisor.rs",
            "owned source processing supervisor worker boundary",
        ),
        (
            "src/native_app/source_processing/supervisor/",
            "owned source processing supervisor service workers",
        ),
        (
            "src/native_app/source_processing/worker",
            "owned source analysis worker process boundary",
        ),
        (
            "src/native_app/sample_library/trash_actions/movement.rs",
            "trash movement worker",
        ),
        (
            "src/native_app/waveform/audio_file/",
            "waveform cache and decode workers",
        ),
        (
            "src/native_app/waveform_edits/worker.rs",
            "waveform destructive edit implementation module root",
        ),
        (
            "src/native_app/waveform_edits/worker/",
            "waveform destructive edit implementation module children",
        ),
        (
            "src/native_app/waveform/similar_sections/source_loading.rs",
            "similar sections sample loading worker",
        ),
    ] {
        guardrail = guardrail.allow_path_fragment(fragment, reason);
    }

    guardrail
}

#[derive(Clone, Debug)]
struct WavecrateNonBlockingGuardrail {
    patterns: Vec<WavecrateBlockingPattern>,
    allowlisted_path_fragments: Vec<WavecrateAllowedPathFragment>,
}

impl WavecrateNonBlockingGuardrail {
    fn app_update_paths() -> Self {
        Self {
            patterns: default_non_blocking_patterns(),
            allowlisted_path_fragments: Vec::new(),
        }
    }

    fn forbid_token(
        mut self,
        token: &'static str,
        label: &'static str,
        guidance: &'static str,
    ) -> Self {
        self.patterns.push(WavecrateBlockingPattern {
            token,
            label,
            guidance,
        });
        self
    }

    fn allow_path_fragment(
        mut self,
        fragment: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        self.allowlisted_path_fragments
            .push(WavecrateAllowedPathFragment {
                fragment: normalize_path_fragment(&fragment.into()),
                reason: reason.into(),
            });
        self
    }

    fn scan_roots<I, P>(&self, roots: I) -> Result<(), WavecrateNonBlockingGuardrailReport>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let mut report = WavecrateNonBlockingGuardrailReport::default();
        for root in roots {
            self.scan_path(root.as_ref(), &mut report);
        }
        if report.is_empty() {
            Ok(())
        } else {
            Err(report)
        }
    }

    fn scan_path(&self, path: &Path, report: &mut WavecrateNonBlockingGuardrailReport) {
        if self.is_allowlisted(path) || !path.exists() {
            return;
        }
        if path.is_dir() {
            self.scan_dir(path, report);
            return;
        }
        if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
            self.scan_file(path, report);
        }
    }

    fn scan_dir(&self, dir: &Path, report: &mut WavecrateNonBlockingGuardrailReport) {
        match fs::read_dir(dir) {
            Ok(entries) => {
                for entry in entries {
                    match entry {
                        Ok(entry) => self.scan_path(&entry.path(), report),
                        Err(error) => report.read_errors.push(WavecrateGuardrailReadError {
                            path: dir.to_owned(),
                            error: error.to_string(),
                        }),
                    }
                }
            }
            Err(error) => report.read_errors.push(WavecrateGuardrailReadError {
                path: dir.to_owned(),
                error: error.to_string(),
            }),
        }
    }

    fn scan_file(&self, path: &Path, report: &mut WavecrateNonBlockingGuardrailReport) {
        let source = match fs::read_to_string(path) {
            Ok(source) => source,
            Err(error) => {
                report.read_errors.push(WavecrateGuardrailReadError {
                    path: path.to_owned(),
                    error: error.to_string(),
                });
                return;
            }
        };
        let mut cfg_test_pending = false;
        let mut skipped_cfg_test_depth = None;
        for (line_index, line) in source.lines().enumerate() {
            if let Some(depth) = skipped_cfg_test_depth {
                let next_depth = depth + brace_delta(line);
                skipped_cfg_test_depth = (next_depth > 0).then_some(next_depth);
                continue;
            }

            let trimmed = line.trim();
            if is_cfg_test_line(trimmed) {
                cfg_test_pending = true;
                continue;
            }
            if cfg_test_pending {
                if trimmed.is_empty()
                    || trimmed.starts_with("//")
                    || (trimmed.starts_with("#[") && !trimmed.starts_with("#[cfg("))
                {
                    continue;
                }
                let depth = brace_delta(line);
                if depth > 0 {
                    skipped_cfg_test_depth = Some(depth);
                }
                cfg_test_pending = false;
                continue;
            }

            for pattern in &self.patterns {
                if line.contains(pattern.token) {
                    report.violations.push(WavecrateNonBlockingViolation {
                        path: path.to_owned(),
                        line: line_index + 1,
                        token: pattern.token,
                        label: pattern.label,
                        guidance: pattern.guidance,
                        source_line: line.trim().to_owned(),
                    });
                    break;
                }
            }
        }
    }

    fn is_allowlisted(&self, path: &Path) -> bool {
        let normalized = normalize_path_fragment(&path.to_string_lossy());
        self.allowlisted_path_fragments
            .iter()
            .any(|allowlist| normalized.contains(&allowlist.fragment))
    }
}

#[derive(Clone, Debug)]
struct WavecrateBlockingPattern {
    token: &'static str,
    label: &'static str,
    guidance: &'static str,
}

#[derive(Clone, Debug)]
struct WavecrateAllowedPathFragment {
    fragment: String,
    #[allow(dead_code)]
    reason: String,
}

#[derive(Clone, Debug, Default)]
struct WavecrateNonBlockingGuardrailReport {
    violations: Vec<WavecrateNonBlockingViolation>,
    read_errors: Vec<WavecrateGuardrailReadError>,
}

impl WavecrateNonBlockingGuardrailReport {
    fn is_empty(&self) -> bool {
        self.violations.is_empty() && self.read_errors.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WavecrateNonBlockingViolation {
    path: PathBuf,
    line: usize,
    token: &'static str,
    label: &'static str,
    guidance: &'static str,
    source_line: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WavecrateGuardrailReadError {
    path: PathBuf,
    error: String,
}

fn default_non_blocking_patterns() -> Vec<WavecrateBlockingPattern> {
    vec![
        blocking_pattern("std::fs::", "filesystem API"),
        blocking_pattern("fs::", "filesystem API"),
        blocking_pattern(".exists()", "filesystem metadata check"),
        blocking_pattern(".metadata()", "filesystem metadata check"),
        blocking_pattern(".canonicalize()", "filesystem path resolution"),
        blocking_pattern("std::thread::sleep", "thread sleep"),
        blocking_pattern("thread::sleep", "thread sleep"),
        blocking_pattern("std::thread::spawn", "manual thread spawn"),
        blocking_pattern("thread::spawn", "manual thread spawn"),
        blocking_pattern(".join()", "blocking join"),
        blocking_pattern(".recv()", "blocking channel receive"),
        blocking_pattern("blocking_recv", "blocking channel receive"),
        blocking_pattern("SourceDatabase::open", "database open"),
        blocking_pattern("SourceDatabase::open_fast", "database open"),
        blocking_pattern("SourceDatabase::open_with_role", "database open"),
        blocking_pattern("FileDialog::new", "direct file dialog"),
        blocking_pattern("MessageDialog::new", "direct message dialog"),
        blocking_pattern("open::that", "direct shell open"),
        blocking_pattern("arboard::Clipboard", "direct clipboard access"),
        blocking_pattern("std::process::Command", "direct process launch"),
    ]
}

fn blocking_pattern(token: &'static str, label: &'static str) -> WavecrateBlockingPattern {
    WavecrateBlockingPattern {
        token,
        label,
        guidance: "route work through context.business() or a typed platform service",
    }
}

fn normalize_path_fragment(fragment: &str) -> String {
    fragment.replace('\\', "/")
}

fn brace_delta(line: &str) -> i32 {
    line.chars().fold(0, |depth, ch| match ch {
        '{' => depth + 1,
        '}' => depth - 1,
        _ => depth,
    })
}

fn is_comment_or_empty(line: &str) -> bool {
    line.is_empty() || line.starts_with("//")
}

fn is_cfg_test_line(line: &str) -> bool {
    matches!(line.trim(), "#[cfg(test)]" | "#[cfg(any(test, doctest))]")
}

fn cross_crate_public_wildcard_target(line: &str) -> Option<String> {
    let target = line
        .trim()
        .strip_prefix("pub use ")?
        .strip_suffix(';')?
        .trim();
    if !target.ends_with("::*") || !target.contains("::") {
        return None;
    }
    let root = target.split("::").next().unwrap_or_default();
    matches!(
        root,
        "radiant" | "reson" | "wavecrate_analysis" | "wavecrate_library" | "wavecrate_scan"
    )
    .then(|| target.to_owned())
}

fn is_test_source(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()) == Some("tests.rs")
        || path
            .components()
            .any(|component| component.as_os_str() == "tests")
        || path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with("_tests.rs"))
}
