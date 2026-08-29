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

fn scan_forbidden_io(path: &str, source: &str) -> Vec<String> {
    let mut violations = Vec::new();

    for (line_number, code) in production_code_lines(source) {
        let code = code.trim();
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
