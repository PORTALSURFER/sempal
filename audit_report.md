# Audit Report: Sempal Workspace

## Summary
Audit performed on `x:\sempal`. The codebase is generally well-structured but suffers from significant scaling issues in data handling (loading full datasets into memory) and a critical defect in the Windows update mechanism.

## Findings



### 2. Broken Windows Updates (Correctness/Maintainability)
**Severity**: Critical (Windows Only)
**Why it matters**: The updater explicitly skips copying the new executable if it matches the running filename. On Windows, you cannot *overwrite* a running executable, but you *can* rename it. The current logic simply leaves the old executable in place, resulting in a "partial update" (assets are new, code is old) which can lead to crashes or undefined behavior.
**Evidence**: `src/updater/apply.rs`: Lines 219-221 (`if running_name.as_deref() == dest.file_name() { continue; }`).
**Recommended change**: 
1.  Detect if `dest` matches the running executable.
2.  If so, `fs::rename(&dest, dest.with_extension("exe.old"))` to move the running binary out of the way.
3.  Copy the new binary to `dest`.
**Risk/Tradeoffs**: Minimal. Standard practice on Windows.
**Quick win?**: Yes.
**Verification**: Run an update on Windows and verify the `.exe` timestamp changes and the old one exists as `.old`.

### 3. Unbounded File Reading in Sanitizer (Performance/DoS)
**Severity**: High
**Why it matters**: `read_sanitized_wav_bytes` reads the **entire file** into memory to sanitize the header. This is utilized during normalization. If a user imports a massive logical recording (e.g., 1GB WAV), the app will attempt to allocate 1GB buffer, potentially causing an OOM crash.
**Evidence**: 
- `src/wav_sanitize.rs`: Line 47 (`std::fs::read(path)`).
- `src/egui_app/controller/collection_items_helpers/io.rs`: Line 7.
**Recommended change**: 
1.  Change `sanitize_wav_bytes` to accept a `Read` or `Seek` trait.
2.  Read only the first 4KB to inspect/fix the header.
3.  If no fix is needed, return the file handle. If fix is needed, construct a "Chained Reader" that performs the fix on the fly or copies the fixed header + streams the rest of the file.
**Risk/Tradeoffs**: Increased complexity in I/O logic.
**Quick win?**: No.
**Verification**: Open a 2GB WAV file and monitor RAM usage. It should not spike by 2GB.

### 4. Inefficient Trash Collection (Performance)
**Severity**: Medium
**Why it matters**: To move trashed files, the application loads the **entire** file list from the database and filters it in memory using Rust code. This is an O(N) memory operation for a task that should be O(T) (number of trashed items).
**Evidence**: `src/egui_app/controller/trash_move.rs`: Lines 78-88 (`db.list_files()` -> `filter()`).
**Recommended change**: 
1.  Add `list_files_by_tag(tag: SampleTag)` to `SourceDatabase` in `src/sample_sources/db/read.rs`.
2.  Use `SELECT ... WHERE tag = ?` to fetch only the files that need to be moved.
**Risk/Tradeoffs**: None.
**Quick win?**: Yes.
**Verification**: Mark 5 files as trash in a 50k library and measure time/memory to open the "Empty Trash" dialog.

### 5. Controller Module Bloat (Maintainability)
**Severity**: Medium (Maintainability)
**Why it matters**: `src/egui_app/controller` is a flat directory with 45 source files and 20 subdirectories. `controller.rs` declares 54 modules. This "god object" structure makes it difficult to understand dependencies or locate logic, slowing down development and increasing merge conflict risks.
**Evidence**: `src/egui_app/controller.rs` and file structure.
**Recommended change**: Refactor into semantic sub-modules:
- `controller/library/` (browsing, sources, collections, scanning)
- `controller/playback/` (audio, loop, transport)
- `controller/ui/` (focus, drag_drop, status)
**Risk/Tradeoffs**: High churn (moving many files), git history might be slightly harder to follow (use `git mv`).
**Quick win?**: No.
**Verification**: Code compiles and `tree` command shows a deeper, cleaner hierarchy.
