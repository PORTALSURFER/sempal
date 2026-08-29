use std::path::PathBuf;

use super::source_scan_support::{
    for_rust_source_file, is_test_source, production_code_lines, read_file, relative_path,
};

#[test]
fn app_chrome_and_workflows_do_not_perform_direct_io() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut violations = Vec::new();

    for relative_root in ["src/native_app/app_chrome", "src/native_app/workflows"] {
        let source_root = root.join(relative_root);
        for_rust_source_file(&source_root, &mut |path| {
            if is_test_source(path) {
                return;
            }
            let relative = relative_path(path, &root);
            violations.extend(scan_forbidden_io(&relative, &read_file(path)));
        });
    }

    assert!(
        violations.is_empty(),
        "app_chrome and workflows must keep direct filesystem/SQLite work behind a background or typed owner boundary:\n{}",
        violations.join("\n")
    );
}

#[test]
fn source_open_db_is_rejected_by_the_forbidden_io_scan() {
    let violations = scan_forbidden_io(
        "fixture.rs",
        "fn projection(source: &Source) {\n    source.open_db();\n}\n",
    );

    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("open_db(") && violation.contains("fixture.rs:2")),
        "the shared forbidden-I/O scan must reject source.open_db() with path/line guidance: {violations:?}"
    );
}

#[test]
fn source_test_fixture_open_facade_is_rejected_by_the_forbidden_io_scan() {
    let violations = scan_forbidden_io(
        "fixture.rs",
        "fn fixture(source: &Source) {\n    source.open_for_test_fixture_source_write();\n}\n",
    );

    assert!(
        violations.iter().any(|violation| {
            violation.contains("open_for_test_fixture_source_write(")
                && violation.contains("fixture.rs:2")
        }),
        "the shared forbidden-I/O scan must reject the test-fixture source DB facade with path/line guidance: {violations:?}"
    );
}

#[test]
fn grouped_std_filesystem_alias_is_rejected() {
    let violations = scan_forbidden_io("fixture.rs", "use std::{fs as disk};\n");

    assert!(
        violations.iter().any(|violation| {
            violation.contains("fixture.rs:1")
                && violation.contains("grouped std import")
                && violation.contains("identifier-bounded `fs`")
        }),
        "grouped std filesystem aliases must be rejected with path/line guidance: {violations:?}"
    );
}

#[test]
fn multiline_grouped_std_filesystem_alias_is_rejected() {
    let violations = scan_forbidden_io("fixture.rs", "use std::{\n    fs as disk,\n};\n");

    assert!(
        violations.iter().any(|violation| {
            violation.contains("fixture.rs:1")
                && violation.contains("grouped std import")
                && violation.contains("identifier-bounded `fs`")
        }),
        "multiline grouped std filesystem aliases must be rejected with the use-item start line: {violations:?}"
    );
}

#[test]
fn grouped_std_non_filesystem_import_is_allowed() {
    let violations = scan_forbidden_io("fixture.rs", "use std::{path::Path, time::Duration};\n");

    assert!(
        violations.is_empty(),
        "grouped std imports without filesystem access should remain allowed: {violations:?}"
    );
}

fn scan_forbidden_io(path: &str, source: &str) -> Vec<String> {
    let mut violations = Vec::new();
    let mut pending_use_item = None;

    for (line_number, code) in production_code_lines(source) {
        let code = code.trim();
        scan_grouped_std_import(
            path,
            line_number,
            code,
            &mut pending_use_item,
            &mut violations,
        );
        for forbidden in FORBIDDEN_IO_TOKENS {
            if code.contains(forbidden.token) {
                violations.push(format!(
                    "{path}:{line_number}: forbidden {} in {} -- {}",
                    forbidden.token, code, forbidden.action
                ));
            }
        }
    }

    violations
}

fn scan_grouped_std_import(
    path: &str,
    line_number: usize,
    code: &str,
    pending_use_item: &mut Option<(usize, String)>,
    violations: &mut Vec<String>,
) {
    let mut remainder = code.to_owned();

    while !remainder.trim().is_empty() {
        if let Some((start_line, mut use_item)) = pending_use_item.take() {
            use_item.push_str(remainder.trim());
            if let Some(semicolon) = use_item.find(';') {
                report_grouped_std_import(path, start_line, &use_item[..=semicolon], violations);
                remainder = use_item[semicolon + 1..].to_owned();
            } else {
                *pending_use_item = Some((start_line, use_item));
                break;
            }
        } else {
            let trimmed = remainder.trim();
            if !starts_use_item(trimmed) {
                break;
            }
            if let Some(semicolon) = trimmed.find(';') {
                report_grouped_std_import(path, line_number, &trimmed[..=semicolon], violations);
                remainder = trimmed[semicolon + 1..].to_owned();
            } else {
                *pending_use_item = Some((line_number, trimmed.to_owned()));
                break;
            }
        }
    }
}

fn report_grouped_std_import(
    path: &str,
    line_number: usize,
    use_item: &str,
    violations: &mut Vec<String>,
) {
    if grouped_std_import_contains_fs(use_item) {
        violations.push(format!(
            "{path}:{line_number}: forbidden grouped std import containing identifier-bounded `fs` in {use_item} -- route filesystem imports through an attributable worker or file-operation owner"
        ));
    }
}

fn starts_use_item(item: &str) -> bool {
    use_tree(item).is_some()
}

fn grouped_std_import_contains_fs(item: &str) -> bool {
    let Some(tree) = use_tree(item) else {
        return false;
    };
    let Some(group) = tree.strip_prefix("std::") else {
        return false;
    };
    group.starts_with('{') && contains_identifier_bounded_fs(group)
}

fn use_tree(item: &str) -> Option<&str> {
    let mut item = item.trim_start();
    if let Some(after_pub) = item.strip_prefix("pub") {
        let after_pub = after_pub.trim_start();
        if let Some(after_visibility) = after_pub.strip_prefix('(') {
            let close = after_visibility.find(") ")?;
            item = &after_visibility[close + 2..];
        } else {
            item = after_pub;
        }
    }
    item.strip_prefix("use ")
}

fn contains_identifier_bounded_fs(import_tree: &str) -> bool {
    for (index, _) in import_tree.match_indices("fs") {
        let before = import_tree[..index].chars().next_back();
        let after = import_tree[index + "fs".len()..].chars().next();
        let before_is_boundary =
            before.map_or(true, |character| !is_rust_identifier_continue(character));
        let after_is_boundary =
            after.map_or(true, |character| !is_rust_identifier_continue(character));
        if before_is_boundary && after_is_boundary {
            return true;
        }
    }
    false
}

fn is_rust_identifier_continue(character: char) -> bool {
    character == '_' || character.is_alphanumeric()
}

struct ForbiddenIoToken {
    token: &'static str,
    action: &'static str,
}

const FORBIDDEN_IO_TOKENS: &[ForbiddenIoToken] = &[
    ForbiddenIoToken {
        token: "std::fs",
        action: "route filesystem work through an attributable worker or file-operation owner",
    },
    ForbiddenIoToken {
        token: "fs::",
        action: "route filesystem work through an attributable worker or file-operation owner",
    },
    ForbiddenIoToken {
        token: "File::open(",
        action: "route filesystem reads through an attributable worker or platform service",
    },
    ForbiddenIoToken {
        token: "File::create(",
        action: "route filesystem writes through the file-operation owner",
    },
    ForbiddenIoToken {
        token: "OpenOptions::new(",
        action: "route filesystem writes through the file-operation owner",
    },
    ForbiddenIoToken {
        token: "rusqlite",
        action: "keep SQLite access behind the source/global database owner",
    },
    ForbiddenIoToken {
        token: "Connection::open(",
        action: "open databases only in an attributable database owner or worker",
    },
    ForbiddenIoToken {
        token: "Connection::open_with_flags(",
        action: "open databases only in an attributable database owner or worker",
    },
    ForbiddenIoToken {
        token: "SourceDatabase::",
        action: "request source-database work through a background or typed owner boundary",
    },
    ForbiddenIoToken {
        token: "open_db(",
        action: "open source databases only in an attributable database owner or worker",
    },
    ForbiddenIoToken {
        token: "open_for_source_write(",
        action: "open source databases only in an attributable database owner or worker",
    },
    ForbiddenIoToken {
        token: "open_for_source_write_with_database_root(",
        action: "open source databases only in an attributable database owner or worker",
    },
    ForbiddenIoToken {
        token: "open_for_background_job(",
        action: "open source databases only in an attributable database owner or worker",
    },
    ForbiddenIoToken {
        token: "open_for_background_job_with_database_root(",
        action: "open source databases only in an attributable database owner or worker",
    },
    ForbiddenIoToken {
        token: "open_for_scan(",
        action: "open source databases only in an attributable database owner or worker",
    },
    ForbiddenIoToken {
        token: "open_for_scan_with_database_root(",
        action: "open source databases only in an attributable database owner or worker",
    },
    ForbiddenIoToken {
        token: "open_for_ui_read(",
        action: "open source databases only in an attributable database owner or worker",
    },
    ForbiddenIoToken {
        token: "open_for_ui_read_with_database_root(",
        action: "open source databases only in an attributable database owner or worker",
    },
    ForbiddenIoToken {
        token: "open_for_maintenance(",
        action: "open source databases only in an attributable database owner or worker",
    },
    ForbiddenIoToken {
        token: "open_for_user_metadata_write(",
        action: "open source databases only in an attributable database owner or worker",
    },
    ForbiddenIoToken {
        token: "open_for_user_metadata_write_with_database_root(",
        action: "open source databases only in an attributable database owner or worker",
    },
    ForbiddenIoToken {
        token: "open_for_playback_history_write(",
        action: "open source databases only in an attributable database owner or worker",
    },
    ForbiddenIoToken {
        token: "open_for_playback_history_write_with_database_root(",
        action: "open source databases only in an attributable database owner or worker",
    },
    ForbiddenIoToken {
        token: "open_for_test_fixture_source_write(",
        action: "open source databases only in an attributable database owner or worker",
    },
    ForbiddenIoToken {
        token: "open_with_role(",
        action: "open source databases only in an attributable database owner or worker",
    },
    ForbiddenIoToken {
        token: "open_with_role_and_database_root(",
        action: "open source databases only in an attributable database owner or worker",
    },
    ForbiddenIoToken {
        token: "open_connection_with_role(",
        action: "open source databases only in an attributable database owner or worker",
    },
    ForbiddenIoToken {
        token: "open_connection_for_background_job(",
        action: "open source databases only in an attributable database owner or worker",
    },
    ForbiddenIoToken {
        token: "open_connection_with_role_and_database_root(",
        action: "open source databases only in an attributable database owner or worker",
    },
    ForbiddenIoToken {
        token: "open_unavailable_source_metadata_connection(",
        action: "open source databases only in an attributable database owner or worker",
    },
];
