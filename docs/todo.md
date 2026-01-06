- add full stereo recording ability, with ASIO support for windows
- in the waveform UX add a record button to start recording, also add a play and stop button, for regular playback

- TODO 2: Decompose enqueue samples pipeline for clarity and reuse.
  - File: src/egui_app/controller/analysis_jobs/enqueue/enqueue_samples.rs
  - Why: large file handles scan, invalidation, DB writes, and logging in one
    place.
  - Suggested: extract enqueue/scan.rs (filesystem + sample list), enqueue/
    invalidate.rs (hash/version checks), and enqueue/persist.rs (DB writes); keep
    a thin orchestration layer.
  - Tests: move/enhance related tests to enqueue/tests.rs and add coverage for
    “skip failed samples on hard sync” vs “force requeue”.
- TODO 5: Split oversized controller tests into focused modules + shared helpers.
  - Files: src/egui_app/controller/tests/waveform.rs, src/egui_app/controller/
    tests/browser_actions.rs, src/egui_app/controller/tests/collections.rs, src/
    egui_app/controller/tests/focus_random.rs
  - Why: 450–530 lines per file, repeated setup/fixtures.
  - Suggested: introduce tests/helpers.rs for controller setup, sample fixtures,
    and selection helpers; split tests by behavior (navigation, selection,
    tagging, hotkeys).
  - Tests: add targeted tests for hotkey focus gating per context (keeps
    regressions visible).
- TODO 6: Consolidate sample staging logic in analysis job enqueueing and add
  coverage for invalidation paths. src/egui_app/controller/analysis_jobs/enqueue/
  enqueue_samples.rs repeats “scan entries → check missing → build metadata” across
  backfill/missing-features; extract a shared “stage samples” helper and add tests
  for “analysis version stale” and “content hash change” invalidation so
  regressions are caught early.
