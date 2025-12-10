## Goal
- Make pressing `f` focus the sample browser's fuzzy search input so users can type queries without leaving the list.

## Proposed solutions
- Add a sample-browser-scoped `f` hotkey that requests focus for the fuzzy search box while preserving existing navigation and playback behaviour.
- Introduce a search focus request flag in the sample browser UI state (mirroring the folder search pattern) and have the filter UI honor it with `request_focus`.

## Step-by-step plan
1. [x] Review existing sample browser search, focus, and hotkey plumbing (`controller/hotkeys.rs`, `controller/wavs.rs`, `state.rs`, `ui/sample_browser_panel.rs`) to map current behaviour and side effects.
2. [x] Add controller/state support for a sample browser search focus request, ensuring focus context stays consistent and no autoplay is triggered.
3. [x] Wire the `f` hotkey for the sample browser scope to trigger the search focus request without disturbing other shortcuts or selections.
4. [x] Update the sample browser filter UI to honor the focus request flag and keep list rebuilding/filtering unchanged.
5. [x] Add or adjust tests covering the new hotkey/search focus behaviour and run the relevant test suite.

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
