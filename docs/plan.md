Goal
- Perform a housekeeping pass to improve maintainability, trim oversized modules, document public surfaces, strengthen tests, and catch latent bugs or perf issues without changing existing behaviour.

Proposed solutions
- Split oversized modules (e.g., `src/ui.rs`, `src/app/sources.rs`, `src/app/playback.rs`, `src/sample_sources/db.rs`) into smaller, focused components while preserving current UI/logic.
- Add documentation to all public structs/functions across audio, sample_sources, selection, waveform, and app layers to clarify responsibilities and usage.
- Expand targeted unit/integration tests for scanning, DB interactions, selection/playback flows, and UI callbacks to guard regressions.
- Harden error handling and responsiveness in scanning/playback pipelines (e.g., thread communication, timer handling, wav loading) and address any correctness/perf gaps found during review.
- Prune dead code/config, align formatting, and keep files under 400 lines by extracting helpers where needed.

Step-by-step plan
1. [-] Review the current architecture for hotspots (noting large files like `src/ui.rs` (627 lines), `src/app/sources.rs`, `src/app/playback.rs`, `src/sample_sources/db.rs`) and record specific refactor targets and risks.
2. [-] Restructure the Slint UI (`src/ui.rs`) into smaller components/templates to reduce file length and improve readability without altering behaviours or bindings.
3. [-] Simplify app coordination modules (`src/app/sources.rs`, `src/app/playback.rs`, `src/app/tags.rs`) by extracting helpers, clarifying state transitions, and adding inline docs where logic is non-obvious.
4. [-] Harden sample source persistence and scanning (`src/sample_sources/db.rs`, `src/sample_sources/scanner.rs`, `src/app/scan.rs`): add docs, improve error handling, and add tests covering DB lifecycle, scan stats, and edge cases.
5. [-] Strengthen playback/selection/waveform correctness (`src/audio.rs`, `src/selection.rs`, `src/waveform.rs`): add unit tests for selection edges and playhead math, document public APIs, and address any timing/perf issues uncovered.
6. [-] Add or update docs across remaining public surfaces and tidy ancillary modules (`src/file_browser.rs`, `src/app/callbacks.rs`, etc.), pruning dead code and ensuring modules stay under 400 lines.
7. [-] Finalize housekeeping: run/check formatting locally as feasible, update any repository docs/todo notes with changes, and outline follow-up actions for the user to validate (builds/tests).
