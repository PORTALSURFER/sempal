## Goal
- Keep waveform selection resizing active until mouse release even if the pointer leaves the waveform frame while dragging a new selection.

## Proposed solutions
- Relax the in-rect checks during selection drags so updates keep flowing while the pointer is outside, clamping normalized positions to the waveform bounds.
- Start selection creation only on in-bounds initiation, but keep subsequent drag updates/finishing hooked to egui drag responses regardless of pointer location.
- Add coverage (unit or UI-facing) to confirm selection updates clamp at edges and do not drop when the pointer exits the frame mid-drag.

## Step-by-step plan
1. [x] Review current waveform drag handling (shift-drag selection and edge handles) to pinpoint where out-of-bounds pointer positions halt updates.
2. [x] Update interaction logic to continue selection updates while dragging outside the frame, clamping positions to [0, 1] and ensuring finishing still occurs on release anywhere.
3. [x] Add or adjust tests (or manual verification notes) to cover dragging beyond bounds and confirm selections stay active until release.
4. [x] Run relevant test suite or targeted checks to ensure no regressions in selection and playback interactions.

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
