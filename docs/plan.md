## Goal
- Perform a housekeeping pass to shrink oversized files, improve maintainability, add missing documentation, resolve existing bugs, and tighten performance without changing current features.

## Proposed solutions
- Decompose the oversized egui controller and renderer into focused modules (playback, selection, collections, persistence/scan) while keeping the UI behaviour intact.
- Add documentation and targeted tests around public APIs, sample-source persistence, and UI state transitions to guard regressions.
- Address known UX/bug issues (loop toggle behaviour, triage autoscroll when collections are focused, autoplay on drop) and verify waveform/audio performance paths.
- Strengthen I/O and caching flows (wav loading/rendering, scanning, config persistence) to keep interactions responsive on larger libraries.

## Step-by-step plan
1. [x] Audit the codebase (file lengths, undocumented public items, TODO/bug list) and run checks/tests to surface current regressions or hotspots.
2. [x] Refactor `src/egui_app/controller.rs` into smaller feature-focused units (playback/selection, collections/persistence, scanning/loading) while preserving the existing controller API and UI state.
3. [x] Break down `src/egui_app/ui.rs` into composable view helpers for the top bar, sources, collections, waveform, and triage panels to reduce file length and clarify responsibilities.
4. [x] Add doc comments for all public types/functions across modules (egui_app state/controller, sample_sources, audio/waveform), keeping files/functions within the size guidelines.
5. [x] Fix the identified UX/bug issues (stop looping after a loop toggle, prevent trash autoscroll when a collection is focused, avoid autoplay on drop) and add regression coverage.
6. [x] Optimize performance-critical paths (wav loading/render caching, scan batching, audio playback progress/fades) using targeted measurements or benchmarks where feasible.
7. [x] Expand automated coverage for critical flows (sample DB scan/tagging, selection/triage navigation, config persistence) and rerun the full test suite to validate changes.

## Code Style & Architecture Rules Reminder
- Keep files under 400 lines; split when necessary.
- When functions require more than 5 arguments, group related values into a struct.
- Each module must have one clear responsibility; split when responsibilities mix.
- Do not use generic buckets like `misc.rs` or `util.rs`. Name modules by domain or purpose.
- Name folders by feature first, not layer first.
- Keep functions under 30 lines; extract helpers as needed.
- Each function must have a single clear responsibility.
- Prefer many small structs over large ones.
- All public objects, functions, structs, traits, and modules must be documented.
- All code should be well tested whenever feasible.
- “Feasible” should be interpreted broadly: tests are expected in almost all cases.
- Prefer small, focused unit tests that validate behaviour clearly.
- Do not allow untested logic unless explicitly approved by the user.
