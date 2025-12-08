## Goal
- Add a waveform selection context menu with crop, trim/delete, directional fade-to-null, and mute-with-fade tools so users can edit the selected audio span directly.

## Proposed solutions
- Attach a context menu to the waveform selection region that only appears when a selection exists and surfaces crop, trim, fade-out (`/` or `\` to show direction), and mute options with clear labels/tooltips.
- Implement selection-based audio editing helpers (crop to selection, trim/remove selection gap, fade the selection to silence in a chosen direction, mute selection with a 5 ms edge fade) that reuse existing wav decode/write utilities and refresh waveform images, caches, and exports.
- Provide status feedback and guard rails (e.g., disable when no loaded sample/selection) to avoid destructive actions without context.
- Add focused tests covering audio processing math and controller flows so new edits do not regress playback/export behaviour.

## Step-by-step plan
1. [x] Review waveform selection rendering and interaction wiring (`src/egui_app/ui/waveform_view.rs`, `src/egui_app/controller/playback.rs`, `src/egui_app/controller/selection_export.rs`) to confirm entry points for context menus and selection bounds.
2. [x] Add controller-level selection edit APIs that reuse existing decoding helpers to crop to selection, trim/remove the selection span, apply directional fade-out to silence (`/` or `\`), and mute the selection with 5 ms edge fades while updating metadata caches, exports, and loaded waveform state.
3. [x] Wire the waveform selection context menu UI to surface the new actions with clear labels/icons, enable/disable rules, and status messaging consistent with existing menus.
4. [x] Add unit tests for the audio edit helpers (crop/trim/fade/mute) and controller responses (metadata refresh, waveform update), keeping functions/modules within size limits.
5. [-] Perform a quick manual or scripted smoke test to ensure selection playback/export still work and new edits behave as expected.

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
