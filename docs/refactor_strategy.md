---
layout: default
title: Refactor Strategy
permalink: /refactor-strategy
description: Small-PR strategy for splitting oversized modules safely without behavior changes.
---

# Refactor Strategy (Small PR Friendly)

## Goals
- Keep behavior stable (mechanical refactors only unless explicitly called out).
- Split oversized modules into smaller, single-responsibility units.
- Preserve public APIs and call sites to minimize churn.
- Land changes in small PRs that are easy to review and revert.

## Working rules
- Start from a green baseline; run the smallest relevant `cargo test` target before and after each PR.
- Prefer extraction + delegation: move cohesive code into `mod` files, then keep thin façade methods on the original type (e.g. `EguiController`).
- Avoid “drive-by” renames and formatting-only churn; move code first, then improve names in follow-up PRs if needed.
- Keep modules focused; if a new file grows, split again rather than creating catch-all helpers.
- Use `pub(super)` and `pub(crate)` to keep visibility tight; re-export sparingly.
- If a function crosses 30 lines, extract helpers; if an API needs >5 args, group into a struct.
- Add/adjust unit tests when the extraction changes seams; don’t allow new untested logic.

## Step-by-step workflow (repeat per PR)
1. Identify one cohesive “slice” (typically ~150–300 LOC) to extract.
2. Create a new module file under the same directory (e.g. `src/egui_app/controller/wavs/<slice>.rs`).
3. Move code with minimal edits; keep signatures and logic unchanged.
4. Wire it up via `mod` + `use` + delegation methods.
5. Run targeted tests (`cargo test <module_or_test_name>`), then expand if needed.
6. Commit with a single purpose; include a short note in the PR description about what moved and what did not change.

## Current targets (in order)
- `src/egui_app/controller/wavs.rs`: split browser list/filter/search, row actions, selection/triage ops, loading/autoplay.
- `src/egui_app/controller/source_folders.rs`: split folder tree model/search, filesystem ops, selection/navigation, sync orchestration.
- `src/egui_app/ui/waveform_view.rs`: split rendering, interactions, context menus/destructive actions, selection-handle drag logic.
- `src/egui_app/controller/playback.rs`: split transport/playback, random navigation/history, tagging/undo helpers.

