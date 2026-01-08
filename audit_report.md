# Audit Report: Sempal Workspace

## Summary
Audit performed on `x:\sempal`. The codebase is generally well-structured but suffers from significant scaling issues in data handling (loading full datasets into memory) and a critical defect in the Windows update mechanism.

## Findings


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
