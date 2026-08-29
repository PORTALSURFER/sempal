use std::fs;
use std::path::Path;

use tempfile::{Builder, TempDir};

pub(crate) fn is_test_source(path: &Path) -> bool {
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

pub(crate) fn for_rust_source_file(root: &Path, visit: &mut impl FnMut(&Path)) {
    let root_metadata = fs::symlink_metadata(root)
        .unwrap_or_else(|error| panic!("{} should have metadata: {error}", root.display()));
    let root_type = root_metadata.file_type();
    if root_type.is_symlink() {
        panic!(
            "{} is a source-tree symlink; production source walking must fail closed",
            root.display()
        );
    }
    if root_type.is_file() {
        if root.extension().and_then(|extension| extension.to_str()) == Some("rs") {
            visit(root);
            return;
        }
        panic!(
            "{} is an unsupported non-Rust source leaf; expected a directory or .rs file",
            root.display()
        );
    }
    if !root_type.is_dir() {
        panic!(
            "{} is an unsupported source root; expected a directory or .rs file",
            root.display()
        );
    }
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

#[test]
fn source_walker_visits_a_regular_rust_leaf_once() {
    let leaf = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/native_app/app_chrome.rs");
    let mut visited = Vec::new();

    for_rust_source_file(&leaf, &mut |path| visited.push(path.to_path_buf()));

    assert_eq!(
        visited,
        vec![leaf],
        "a regular .rs leaf must invoke the callback exactly once with its exact path"
    );
}

pub(crate) fn relative_path(path: &Path, root: &Path) -> String {
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

pub(crate) fn read_file(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("{} should be readable: {error}", path.display()))
}

/// Keep line numbers while excluding only test-only items. Production code after an inline test
/// block remains visible to the scan, so fixtures cannot hide later direct I/O.
pub(crate) fn production_code_lines(source: &str) -> Vec<(usize, String)> {
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

    let source_root: TempDir = Builder::new()
        .prefix("io_architecture_contract_symlink")
        .tempdir()
        .expect("create disposable symlink fixture");
    let real_source = source_root.path().join("real.rs");
    let linked_source = source_root.path().join("linked.rs");
    fs::write(&real_source, "fn fixture() {}\n").expect("write real Rust fixture");
    symlink(&real_source, &linked_source).expect("create symlinked Rust fixture");

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        for_rust_source_file(source_root.path(), &mut |_| {});
    }));

    assert!(
        result.is_err(),
        "source walker must reject a symlinked .rs entry instead of silently skipping it"
    );
}

#[cfg(unix)]
#[test]
fn source_walker_rejects_symlinked_root_before_callback() {
    use std::os::unix::fs::symlink;

    let real_root = Builder::new()
        .prefix("io_architecture_contract_real_root")
        .tempdir()
        .expect("create real source root");
    let link_parent = Builder::new()
        .prefix("io_architecture_contract_link_parent")
        .tempdir()
        .expect("create symlink parent");
    fs::write(real_root.path().join("real.rs"), "fn fixture() {}\n")
        .expect("write real Rust fixture");
    let mut callback_reached = false;
    for_rust_source_file(real_root.path(), &mut |_| callback_reached = true);
    assert!(
        callback_reached,
        "source walker must reach a real TempDir root's Rust entry"
    );
    let linked_root = link_parent.path().join("linked-root");
    symlink(real_root.path(), &linked_root).expect("create symlinked source root");
    callback_reached = false;

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        for_rust_source_file(&linked_root, &mut |_| callback_reached = true);
    }));

    assert!(
        result.is_err(),
        "source walker must reject a symlinked root before reading its entries"
    );
    assert!(
        !callback_reached,
        "source walker must not reach the callback for a symlinked root"
    );
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
