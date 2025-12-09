## Goal
- Remove the initial “sticky” feel when resizing waveform selection edges so edge drags respond instantly and smoothly for precise tweaks.

## Proposed solutions
- Start selection edge drags on pointer-down inside the edge handles (skip egui’s drag threshold) and update continuously while the button is held.
- Alternatively, lower/bypass drag thresholds only for selection edges via custom interaction handling while keeping other drags unchanged.
- Confirm selection creation/seek behaviour and drag-and-drop handle interactions stay unchanged after the edge drag refinement.

## Step-by-step plan
1. [x] Trace the current selection edge drag flow (UI responses in `waveform_view`, controller hooks, `SelectionState`) to confirm where the drag threshold delays updates.
2. [x] Implement immediate edge dragging on pointer-down for selection brackets, ensuring normalized updates run every frame without waiting for drag thresholds.
3. [x] Guard other interactions (selection creation, seek clicks, drag payload handle) so they remain unchanged and cursors/hover states still feel correct, and keep edge alignment stable when drags start.
4. [-] Add/adjust tests or targeted UI logic checks for zero-threshold edge drags and document a brief manual QA pass for selection resize smoothness.
5. [x] Run relevant tests (e.g., `cargo test` for selection/controller modules) and perform a quick manual resize check in the waveform view.

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
