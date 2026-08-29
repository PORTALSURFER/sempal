//! Static guardrails for the 0.19.1 I/O ownership and projection contract.

use std::fs;
use std::path::{Path, PathBuf};

const IO_ARCHITECTURE_TARGET: &str = include_str!("../docs/IO_ARCHITECTURE_TARGET.md");
const IO_ALIGNMENT_ESTIMATE: &str = include_str!("../docs/IO_ALIGNMENT_ESTIMATE.md");

const OWNER_FORBIDDEN_SIDE_EFFECT_PAIRS: &[(&str, &str)] = &[
    ("**I/O coordinator**", "Direct filesystem or SQL work."),
    (
        "**Durable app-local journal**",
        "Source manifest truth or arbitrary user metadata.",
    ),
    (
        "**File operation owner**",
        "SQLite transactions or browser projection.",
    ),
    (
        "**Per-physical-source DB writer owner**",
        "Filesystem traversal, copy, hashing, cache payload writes, or another source.",
    ),
    ("**Global-library owner**", "physical file mutation"),
    ("**Harvest owner**", "Rendering or copying bytes."),
    (
        "**Projection publisher**",
        "Filesystem/SQLite reads during UI application.",
    ),
    (
        "**Artifact store**",
        "Durable user metadata or source membership.",
    ),
];

#[test]
fn source_revision_contract_is_one_monotonic_cursor() {
    for required in [
        concat!(
            "For the ",
            "\x600.19.1\x60",
            " target, each physical source has one monotonic committed ",
            "\x60SourceRevision\x60",
            ".",
        ),
        "It is the sole authoritative publication cursor for source membership, path, and structural\ndirectory truth.",
        "A directory generation is only a staging/readiness aid fenced to the committed\n\x60SourceRevision\x60;",
        "There is no composite source-publication cursor.",
        "advances the single source revision only when authoritative source truth changed,",
    ] {
        assert!(
            IO_ARCHITECTURE_TARGET.contains(required),
            "IO_ARCHITECTURE_TARGET.md must preserve the resolved single-cursor contract; missing required wording: {required}"
        );
    }
    assert!(
        IO_ALIGNMENT_ESTIMATE.contains("The OPT-1298 contract gate"),
        "IO_ALIGNMENT_ESTIMATE.md must record the bounded OPT-1298 contract gate"
    );
    assert!(
        !IO_ARCHITECTURE_TARGET.contains("Should source revisions be one global manifest sequence"),
        "the old open source-revision decision must be removed once the 0.19.1 contract is resolved"
    );
}

#[test]
fn io_target_names_owners_and_forbidden_side_effects() {
    for (owner, forbidden) in OWNER_FORBIDDEN_SIDE_EFFECT_PAIRS {
        let row = IO_ARCHITECTURE_TARGET
            .lines()
            .find(|line| line.starts_with('|') && line.contains(owner))
            .unwrap_or_else(|| {
                panic!("IO_ARCHITECTURE_TARGET.md must contain an owner-table row for {owner}")
            });
        assert!(
            row.contains(forbidden),
            "owner row for {owner} must contain paired forbidden-side-effect text {forbidden}; row was: {row}"
        );
    }
}

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
            for (line_number, code) in production_code_lines(&read_file(path)) {
                let code = code.trim();
                for forbidden in FORBIDDEN_IO_TOKENS {
                    if code.contains(forbidden.token) {
                        violations.push(format!(
                            "{relative}:{line_number}: forbidden {} in {} -- {}",
                            forbidden.token, code, forbidden.action
                        ));
                    }
                }
            }
        });
    }

    assert!(
        violations.is_empty(),
        "app_chrome and workflows must keep direct filesystem/SQLite work behind a background or typed owner boundary:\n{}",
        violations.join("\n")
    );
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
        token: "open_for_source_write(",
        action: "open source databases only in an attributable database owner or worker",
    },
    ForbiddenIoToken {
        token: "open_for_background_job(",
        action: "open source databases only in an attributable database owner or worker",
    },
    ForbiddenIoToken {
        token: "open_for_scan(",
        action: "open source databases only in an attributable database owner or worker",
    },
    ForbiddenIoToken {
        token: "open_for_maintenance(",
        action: "open source databases only in an attributable database owner or worker",
    },
    ForbiddenIoToken {
        token: "open_with_role(",
        action: "open source databases only in an attributable database owner or worker",
    },
    ForbiddenIoToken {
        token: "open_connection_with_role(",
        action: "open source databases only in an attributable database owner or worker",
    },
];

fn is_test_source(path: &Path) -> bool {
    if path
        .components()
        .any(|component| component.as_os_str().to_string_lossy() == "tests")
    {
        return true;
    }
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| stem == "tests" || stem.ends_with("_tests"))
}

fn for_rust_source_file(root: &Path, visit: &mut impl FnMut(&Path)) {
    let mut entries = fs::read_dir(root)
        .unwrap_or_else(|error| panic!("{} should be readable: {error}", root.display()))
        .map(|entry| entry.expect("source directory entry should be readable"))
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .unwrap_or_else(|error| panic!("{} should have a file type: {error}", path.display()));
        if file_type.is_dir() {
            for_rust_source_file(&path, visit);
        } else if file_type.is_file()
            && path.extension().and_then(|extension| extension.to_str()) == Some("rs")
        {
            visit(&path);
        } else if file_type.is_symlink() {
            panic!(
                "{} is a source-tree symlink; production source walking must fail closed",
                path.display()
            );
        }
    }
}

fn relative_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or_else(|error| {
            panic!(
                "{} should be under {}: {error}",
                path.display(),
                root.display()
            )
        })
        .to_string_lossy()
        .replace('\\', "/")
}

fn read_file(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("{} should be readable: {error}", path.display()))
}

/// Keep line numbers while excluding only test-only items. Production code after an inline test
/// block remains visible to the scan, so fixtures cannot hide later direct I/O.
fn production_code_lines(source: &str) -> Vec<(usize, String)> {
    let mut lexer = RustLexState::default();
    let mut pending_test_item = false;
    let mut skipped_braces = 0_usize;
    let mut lines = Vec::new();

    for (line_index, raw_line) in source.lines().enumerate() {
        let line_number = line_index + 1;
        let code = lexer.code_line(raw_line);
        let trimmed = code.trim();
        let (opens, closes) = brace_counts(&code);

        if skipped_braces > 0 {
            skipped_braces = advance_brace_depth(skipped_braces, opens, closes);
            continue;
        }
        if pending_test_item {
            if opens > 0 {
                skipped_braces = advance_brace_depth(0, opens, closes);
                pending_test_item = false;
            } else if trimmed.contains(';') {
                pending_test_item = false;
            }
            continue;
        }
        if trimmed == "#[cfg(test)]" || trimmed.starts_with("#[cfg(all(test") {
            pending_test_item = true;
            continue;
        }
        lines.push((line_number, code));
    }

    lines
}

#[test]
fn code_after_char_and_byte_char_literals_remains_visible() {
    for (literal, label) in [("'{'", "char"), ("b'{'", "byte-char")] {
        let source = format!(
            "#[cfg(test)]\nmod fixture {{\n    let _literal = {literal};\n}}\nstd::fs::read(\"production\");\n"
        );
        let lines = production_code_lines(&source);
        assert!(
            lines.iter().any(|(_, line)| line.contains("std::fs::read")),
            "code after a {label} literal must remain visible to the I/O scan"
        );
    }
}

#[cfg(unix)]
#[test]
fn source_walker_rejects_symlinked_rust_entries() {
    use std::os::unix::fs::symlink;
    use std::time::{SystemTime, UNIX_EPOCH};

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let temp_root = manifest_dir.join(".tmp");
    fs::create_dir_all(&temp_root).expect("create repository-local disposable root");
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after the Unix epoch")
        .as_nanos();
    let source_root = temp_root.join(format!(
        "io_architecture_contract_symlink_{}_{}",
        std::process::id(),
        unique
    ));
    assert!(
        !source_root.exists(),
        "unique repository-local symlink fixture path unexpectedly exists: {}",
        source_root.display()
    );
    let cleanup = DisposableSourceRoot {
        path: source_root.clone(),
    };
    fs::create_dir(&source_root).expect("create repository-local symlink fixture");
    let real_source = source_root.join("real.rs");
    let linked_source = source_root.join("linked.rs");
    fs::write(&real_source, "fn fixture() {}\n").expect("write real Rust fixture");
    symlink(&real_source, &linked_source).expect("create symlinked Rust fixture");

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        for_rust_source_file(&source_root, &mut |_| {});
    }));

    assert!(
        result.is_err(),
        "source walker must reject a symlinked .rs entry instead of silently skipping it"
    );
    drop(cleanup);
}

fn brace_counts(code: &str) -> (usize, usize) {
    (
        code.chars().filter(|character| *character == '{').count(),
        code.chars().filter(|character| *character == '}').count(),
    )
}

fn advance_brace_depth(depth: usize, opens: usize, closes: usize) -> usize {
    depth.saturating_add(opens).saturating_sub(closes)
}

#[derive(Default)]
struct RustLexState {
    block_comment_depth: usize,
    quoted: Option<QuotedState>,
}

#[derive(Clone, Copy)]
enum QuotedState {
    String,
    RawString(usize),
}

impl RustLexState {
    fn code_line(&mut self, line: &str) -> String {
        let bytes = line.as_bytes();
        let mut code = String::with_capacity(line.len());
        let mut index = 0;
        let mut escaped = false;

        while index < bytes.len() {
            if self.block_comment_depth > 0 {
                if bytes[index..].starts_with(b"/*") {
                    self.block_comment_depth += 1;
                    index += 2;
                } else if bytes[index..].starts_with(b"*/") {
                    self.block_comment_depth -= 1;
                    index += 2;
                } else {
                    index += 1;
                }
                continue;
            }
            if let Some(quoted) = self.quoted {
                match quoted {
                    QuotedState::String => {
                        if escaped {
                            escaped = false;
                        } else if bytes[index] == b'\\' {
                            escaped = true;
                        } else if bytes[index] == b'"' {
                            self.quoted = None;
                        }
                        index += 1;
                    }
                    QuotedState::RawString(hash_count) => {
                        if bytes[index] == b'"' {
                            let mut end = index + 1;
                            while end < bytes.len() && bytes[end] == b'#' {
                                end += 1;
                            }
                            if end - index - 1 == hash_count {
                                self.quoted = None;
                                index = end;
                            } else {
                                index += 1;
                            }
                        } else {
                            index += 1;
                        }
                    }
                }
                continue;
            }

            if bytes[index..].starts_with(b"//") {
                break;
            }
            if bytes[index..].starts_with(b"/*") {
                self.block_comment_depth = 1;
                index += 2;
                continue;
            }
            if let Some((quoted, consumed)) = raw_string_start(bytes, index) {
                self.quoted = Some(quoted);
                index += consumed;
                continue;
            }
            if bytes[index] == b'b'
                && bytes.get(index + 1) == Some(&b'\'')
                && char_literal_len(bytes, index + 1).is_some()
            {
                index += char_literal_len(bytes, index + 1).unwrap_or(0) + 1;
                continue;
            }
            if bytes[index] == b'\'' {
                if let Some(consumed) = char_literal_len(bytes, index) {
                    index += consumed;
                    continue;
                }
            }
            if bytes[index] == b'"' {
                self.quoted = Some(QuotedState::String);
                escaped = false;
                index += 1;
            } else {
                code.push(bytes[index] as char);
                index += 1;
            }
        }

        code
    }
}

fn raw_string_start(bytes: &[u8], index: usize) -> Option<(QuotedState, usize)> {
    let prefix_len = usize::from(bytes.get(index) == Some(&b'b'));
    if bytes.get(index + prefix_len) != Some(&b'r') {
        return None;
    }

    let mut cursor = index + prefix_len + 1;
    let mut hash_count = 0;
    while bytes.get(cursor) == Some(&b'#') {
        hash_count += 1;
        cursor += 1;
    }
    (bytes.get(cursor) == Some(&b'"'))
        .then_some((QuotedState::RawString(hash_count), cursor + 1 - index))
}

fn char_literal_len(bytes: &[u8], quote_index: usize) -> Option<usize> {
    if bytes.get(quote_index) != Some(&b'\'') {
        return None;
    }

    let mut cursor = quote_index + 1;
    match bytes.get(cursor) {
        Some(b'\\') => {
            cursor += 1;
            match bytes.get(cursor) {
                Some(b'u') if bytes.get(cursor + 1) == Some(&b'{') => {
                    cursor += 2;
                    while let Some(byte) = bytes.get(cursor) {
                        cursor += 1;
                        if *byte == b'}' {
                            break;
                        }
                    }
                }
                Some(b'x') => cursor += 3,
                Some(_) => cursor += 1,
                None => return None,
            }
        }
        Some(_) => {
            let character = std::str::from_utf8(&bytes[cursor..]).ok()?.chars().next()?;
            cursor += character.len_utf8();
        }
        None => return None,
    }

    (bytes.get(cursor) == Some(&b'\'')).then_some(cursor + 1 - quote_index)
}

struct DisposableSourceRoot {
    path: PathBuf,
}

impl Drop for DisposableSourceRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
