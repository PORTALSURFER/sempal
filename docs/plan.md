## Goal
- Make the waveform viewer respond to `Home`/`End` by jumping to the start or end of the sample when the waveform has keyboard focus.

## Proposed solutions
- Extend existing waveform keyboard handling in `egui_app` to recognize `Home`/`End` and route them through the controller.
- Add controller-level helpers so the jump logic (playhead position, viewport adjustment, playback visibility) stays centralized and testable.
- Ensure behaviour respects current focus checks and leaves other contexts unchanged.

## Step-by-step plan
1. [-] Review current waveform keyboard handling paths (`InputSnapshot`, focus checks, controller navigation helpers) to choose the right hook for `Home`/`End`.
2. [-] Add controller methods to jump the waveform playhead/view to the start or end of the sample, keeping playhead visibility consistent.
3. [-] Wire `Home`/`End` handling when waveform focus is active and verify no other contexts consume these keys.
4. [-] Add/update controller tests covering the new jump behaviour and adjust any related docs or hotkey references if needed.

## Code Style & Architecture Rules Reminder
- File and module structure
  - Keep files under 400 lines; split when necessary.
  - When functions require more than 5 arguments, group related values into a struct.
  - Each module must have one clear responsibility; split when responsibilities mix.
  - Do not use generic buckets like `misc.rs` or `util.rs`. Name modules by domain or purpose.
  - Name folders by feature first, not layer first.
- Functions
  - Keep functions under 30 lines; extract helpers as needed.
  - Each function must have a single clear responsibility.
  - Prefer many small structs over large ones.
- Documentation
  - All public objects, functions, structs, traits, and modules must be documented.
- Testing
  - All code should be well tested whenever feasible.
  - “Feasible” should be interpreted broadly: tests are expected in almost all cases.
  - Prefer small, focused unit tests that validate behaviour clearly.
  - Do not allow untested logic unless explicitly approved by the user.
