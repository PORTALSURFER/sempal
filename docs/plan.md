## Goal
- Migrate the UI to egui, keep existing functionality (sources, triage, waveform/selection, playback, collections with drag/drop), and now improve wav list performance so 50k+ items stay responsive.

## Proposed solutions
- Keep the current three-panel layout in egui while making list rendering scale-friendly (unique IDs, stable state).
- Profile and measure the current wav loading/render path to identify hotspots (database fetch, view-model build, UI rendering).
- Introduce incremental/virtualized wav list rendering (e.g., chunked `ScrollArea` drawing, windowed slices) to avoid painting all rows at once.
- Defer expensive per-row work (string formatting, allocation) and reuse cached view models keyed by source IDs.
- Keep scanning manual and non-blocking; continue background loading with caching invalidated only on explicit scans.

## Step-by-step plan
1. [x] Inventory UI features and interactions to map them to egui widgets and events.
2. [x] Design egui layout (top bar, sources panel, waveform + triage columns, collections pane) with consistent theming.
3. [x] Define egui-side state models mirroring existing logic (sources/wavs/collections rows, drag state, selection state) and wire persistence.
4. [x] Implement egui rendering for core panels: sources list, triage lists, status bar, top bar controls.
5. [x] Implement waveform view in egui (texture rendering + overlays) and wire playback/selection interactions.
6. [x] Implement collections panel with add/select, member list, drag/drop tagging, and hover/drop feedback.
7. [x] Replace application entry/runtime to launch egui, remove Slint dependencies, and ensure audio/worker threads communicate with egui state.
8. [x] Profile current wav list loading/rendering (measure DB fetch, view-model building, and UI frame time for 50k items); record bottlenecks.
9. [x] Add virtualized/chunked rendering for wav lists (windowed slices, row height caching) so only visible rows draw each frame.
10. [x] Cache wav view models per source and reuse between frames; avoid recomputing strings/paths every frame, and throttle rebuilds on scroll.
11. [x] Validate performance gains with large sources (50k+ files) and adjust thresholds; add unit tests for view-model paging logic and manual QA checklist for large lists.
12. [x] Remove remaining Slint assets/code paths (if any) after performance work is stable.

## Code Style & Architecture Rules Reminder
- Keep files under 400 lines; split when necessary. Name folders by feature first; avoid generic buckets.
- Keep functions under 30 lines with single responsibilities; group 5+ args into a struct; prefer many small structs.
- Each module has one clear responsibility; avoid misc/util buckets.
- Document all public objects, functions, structs, traits, and modules.
- All code should be well tested whenever feasible; prefer small, focused unit tests; do not leave untested logic without explicit approval.
