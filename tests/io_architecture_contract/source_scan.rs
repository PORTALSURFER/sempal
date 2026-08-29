use std::path::PathBuf;

use super::source_scan_support::{
    for_rust_source_file, is_test_source, production_code_lines, read_file, relative_path,
};

#[test]
fn app_chrome_and_workflows_do_not_perform_direct_io() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut violations = Vec::new();

    for relative_root in [
        "src/native_app/app_chrome",
        "src/native_app/app_chrome.rs",
        "src/native_app/workflows",
        "src/native_app/workflows.rs",
    ] {
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
fn source_database_opening_facades_are_rejected_by_the_forbidden_io_scan() {
    for (call, canonical_token) in [
        ("source.open_db ();", "open_db("),
        (
            "Db::open_for_source_write (root);",
            "open_for_source_write(",
        ),
        (
            "source.open_for_test_fixture_source_write();",
            "open_for_test_fixture_source_write(",
        ),
    ] {
        let source = format!("fn projection() {{\n    {call}\n}}\n");
        let violations = scan_forbidden_io("fixture.rs", &source);
        let expected_prefix = format!("fixture.rs:2: forbidden {canonical_token} in ");

        assert!(
            violations
                .iter()
                .any(|violation| violation.starts_with(&expected_prefix)),
            "the shared forbidden-I/O scan must reject {call:?} with canonical token and path/line guidance: {violations:?}"
        );
    }
}

#[test]
fn source_database_type_names_are_identifier_bounded() {
    let source = concat!(
        "use crate::SourceDatabase as Db; type SourceDb = SourceDatabase; struct Holder { database: &SourceDatabase }\n",
        "fn query(database: &SourceDatabase) -> SourceDatabase {\n",
        "    database.get_metadata();\n}\n",
        "fn allowed(role: SourceDatabaseConnectionRole, request: SourceDatabaseRequest) {}\n",
    );
    let violations = scan_forbidden_io("fixture.rs", source);

    for line in [1, 2] {
        assert!(
            violations.iter().any(|violation| {
                violation.starts_with(&format!("fixture.rs:{line}: forbidden SourceDatabase in "))
            }),
            "SourceDatabase must be rejected as an identifier-bounded token on line {line}: {violations:?}"
        );
    }
    assert!(
        violations.iter().all(|violation| {
            !violation.contains("get_metadata(")
                && !violation.contains("SourceDatabaseConnectionRole")
                && !violation.contains("SourceDatabaseRequest")
        }),
        "the SourceDatabase guard must not broadly ban get_metadata or longer type names: {violations:?}"
    );
}

#[test]
fn grouped_std_filesystem_alias_is_rejected() {
    let violations = scan_forbidden_io(
        "fixture.rs",
        "pub(crate)  use std::{fs as disk};\ndisk::read (path);\n",
    );

    assert!(
        violations.iter().any(|violation| {
            violation.contains("fixture.rs:1")
                && violation.contains("grouped std import")
                && violation.contains("identifier-bounded `fs`")
        }),
        "grouped std filesystem aliases must be rejected with path/line guidance: {violations:?}"
    );
    assert!(
        violations.iter().any(|violation| {
            violation.contains("fixture.rs:2") && violation.contains("::read(")
        }),
        "aliased filesystem reads must retain their original line while reporting the canonical token: {violations:?}"
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
        let matching_code = compact_for_matching(code);
        for forbidden in FORBIDDEN_IO_TOKENS {
            let matches = if is_identifier_only(forbidden.token) {
                contains_identifier_bounded(code, forbidden.token)
            } else {
                matching_code.contains(&compact_for_matching(forbidden.token))
            };
            if matches {
                violations.push(format!(
                    "{path}:{line_number}: forbidden {} in {} -- {}",
                    forbidden.token, code, forbidden.action
                ));
            }
        }
    }

    violations
}

fn compact_for_matching(source: &str) -> String {
    source
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
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
            let fragment = remainder.trim();
            if !use_item.is_empty() && !fragment.is_empty() {
                use_item.push(' ');
            }
            use_item.push_str(fragment);
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
    let normalized_tree = compact_for_matching(tree);
    normalized_tree.starts_with("std::{") && contains_identifier_bounded(tree, "fs")
}

fn use_tree(item: &str) -> Option<&str> {
    let mut item = item.trim_start();
    if let Some(after_pub) = item.strip_prefix("pub") {
        let after_pub = after_pub.trim_start();
        if let Some(after_visibility) = after_pub.strip_prefix('(') {
            let close = after_visibility.find(')')?;
            item = &after_visibility[close + 1..];
        } else {
            item = after_pub;
        }
    }
    let item = item.trim_start();
    let after_use = item.strip_prefix("use")?;
    if after_use
        .chars()
        .next()
        .is_some_and(|character| !character.is_whitespace())
    {
        return None;
    }
    Some(after_use.trim_start())
}

fn is_identifier_only(token: &str) -> bool {
    !token.is_empty() && token.chars().all(is_rust_identifier_continue)
}

fn contains_identifier_bounded(source: &str, token: &str) -> bool {
    for (index, _) in source.match_indices(token) {
        let before = source[..index].chars().next_back();
        let after = source[index + token.len()..].chars().next();
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

const fn forbidden_io_token(token: &'static str, action: &'static str) -> ForbiddenIoToken {
    ForbiddenIoToken { token, action }
}

const FILESYSTEM_ACTION: &str =
    "route filesystem work through an attributable worker or file-operation owner";
const NAMESPACED_READ_ACTION: &str =
    "route namespaced filesystem or database reads through an attributable owner";
const FILE_READ_ACTION: &str =
    "route filesystem reads through an attributable worker or platform service";
const FILE_WRITE_ACTION: &str = "route filesystem writes through the file-operation owner";
const SQLITE_ACTION: &str = "keep SQLite access behind the source/global database owner";
const DATABASE_OPEN_ACTION: &str =
    "open databases only in an attributable database owner or worker";
const SOURCE_DATABASE_ACTION: &str =
    "request source-database work through a background or typed owner boundary";
const SOURCE_DATABASE_FACADE_ACTION: &str =
    "open source databases only in an attributable database owner or worker";

const FORBIDDEN_IO_TOKENS: &[ForbiddenIoToken] = &[
    forbidden_io_token("std::fs", FILESYSTEM_ACTION),
    forbidden_io_token("fs::", FILESYSTEM_ACTION),
    forbidden_io_token("::read(", NAMESPACED_READ_ACTION),
    forbidden_io_token("File::open(", FILE_READ_ACTION),
    forbidden_io_token("File::create(", FILE_WRITE_ACTION),
    forbidden_io_token("OpenOptions::new(", FILE_WRITE_ACTION),
    forbidden_io_token("rusqlite", SQLITE_ACTION),
    forbidden_io_token("Connection::open(", DATABASE_OPEN_ACTION),
    forbidden_io_token("Connection::open_with_flags(", DATABASE_OPEN_ACTION),
    forbidden_io_token("SourceDatabase", SOURCE_DATABASE_ACTION),
    forbidden_io_token("open_db(", SOURCE_DATABASE_FACADE_ACTION),
    forbidden_io_token("open_for_source_write(", SOURCE_DATABASE_FACADE_ACTION),
    forbidden_io_token(
        "open_for_source_write_with_database_root(",
        SOURCE_DATABASE_FACADE_ACTION,
    ),
    forbidden_io_token("open_for_background_job(", SOURCE_DATABASE_FACADE_ACTION),
    forbidden_io_token(
        "open_for_background_job_with_database_root(",
        SOURCE_DATABASE_FACADE_ACTION,
    ),
    forbidden_io_token("open_for_scan(", SOURCE_DATABASE_FACADE_ACTION),
    forbidden_io_token(
        "open_for_scan_with_database_root(",
        SOURCE_DATABASE_FACADE_ACTION,
    ),
    forbidden_io_token("open_for_ui_read(", SOURCE_DATABASE_FACADE_ACTION),
    forbidden_io_token(
        "open_for_ui_read_with_database_root(",
        SOURCE_DATABASE_FACADE_ACTION,
    ),
    forbidden_io_token("open_for_maintenance(", SOURCE_DATABASE_FACADE_ACTION),
    forbidden_io_token(
        "open_for_user_metadata_write(",
        SOURCE_DATABASE_FACADE_ACTION,
    ),
    forbidden_io_token(
        "open_for_user_metadata_write_with_database_root(",
        SOURCE_DATABASE_FACADE_ACTION,
    ),
    forbidden_io_token(
        "open_for_playback_history_write(",
        SOURCE_DATABASE_FACADE_ACTION,
    ),
    forbidden_io_token(
        "open_for_playback_history_write_with_database_root(",
        SOURCE_DATABASE_FACADE_ACTION,
    ),
    forbidden_io_token(
        "open_for_test_fixture_source_write(",
        SOURCE_DATABASE_FACADE_ACTION,
    ),
    forbidden_io_token("open_with_role(", SOURCE_DATABASE_FACADE_ACTION),
    forbidden_io_token(
        "open_with_role_and_database_root(",
        SOURCE_DATABASE_FACADE_ACTION,
    ),
    forbidden_io_token("open_connection_with_role(", SOURCE_DATABASE_FACADE_ACTION),
    forbidden_io_token(
        "open_connection_for_background_job(",
        SOURCE_DATABASE_FACADE_ACTION,
    ),
    forbidden_io_token(
        "open_connection_with_role_and_database_root(",
        SOURCE_DATABASE_FACADE_ACTION,
    ),
    forbidden_io_token(
        "open_unavailable_source_metadata_connection(",
        SOURCE_DATABASE_FACADE_ACTION,
    ),
];
