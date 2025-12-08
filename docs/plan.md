## Goal
- Investigate waveform view edge cases where the rendered waveform becomes stale or missing when the underlying sample/source/collection state changes (e.g., deleting a source while its sample is selected) and align rendering with expected behaviour.

## Proposed solutions
- Audit waveform state transitions across controller and UI layers to find missing clear/reset paths after source, sample, or collection mutations.
- Define expected behaviours for selection, playback, and notices when data disappears or reloads across sample browser, collections, and playback interactions.
- Add defensive state syncing and tests so waveform images, playhead, and notices always match the current selection and available data.
- Create a reproduction/regression checklist covering high-risk operations (source removal, sample delete/rename, collection edits, missing files) to guide fixes and validation.

## Step-by-step plan
1. [-] Map the waveform lifecycle—selection, loading, playback, teardown—across `src/egui_app/controller/sources.rs`, `wavs.rs`, `sample_browser_actions.rs`, `collection_items_helpers.rs`, `playback.rs`, and `src/egui_app/ui/waveform_view.rs` to spot stale-state paths.
2. [-] Enumerate concrete edge cases and expected outcomes (source removal with selected sample, deleting/renaming loaded sample, clearing collections, missing/failed reads, playback/loop toggles) to target fixes and tests.
3. [-] Implement state-sync fixes so waveform images/selection/playhead/notice reset when backing data changes and texture handles invalidate when data disappears.
4. [-] Add focused controller tests (e.g., in `src/egui_app/controller/tests.rs`) and any small UI guards to prevent rendering stale textures, covering the identified scenarios.
5. [-] Run tests and a short manual pass through the key flows; update plan statuses and summarize findings.

## Code Style & Architecture Rules Reminder
### File and module structure
- Keep files under 400 lines; split when necessary.
- When functions require more than 5 arguments, group related values into a struct.
- Each module must have one clear responsibility; split when responsibilities mix.
- Do not use generic buckets like `misc.rs` or `util.rs`. Name modules by domain or purpose.
- Name folders by feature first, not layer first.

### Functions
- Keep functions under 30 lines; extract helpers as needed.
- Each function must have a single clear responsibility.
- Prefer many small structs over large ones.

### Documentation
- All public objects, functions, structs, traits, and modules must be documented.

### Testing
- All code should be well tested whenever feasible.
- “Feasible” should be interpreted broadly: tests are expected in almost all cases.
- Prefer small, focused unit tests that validate behaviour clearly.
- Do not allow untested logic unless explicitly approved by the user.
