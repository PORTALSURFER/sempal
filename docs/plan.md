## Goal
- Drop folder item focus whenever the folder browser no longer has user focus, while keeping existing selections intact.

## Proposed solutions
- Track focus context transitions and clear the folder browser’s focused row when focus leaves `SourceFolders`, preserving selection state.
- Gate folder row focus highlighting/rendering on active folder focus so UI matches keyboard focus ownership.
- Add regression coverage to ensure focus is cleared on focus loss without disturbing selections or expansion state.

## Step-by-step plan
1. [x] Review current folder focus management in controller and UI (e.g., `source_folders.rs`, `focus.rs`, `sources_panel.rs`) to map how focus is set and retained.
2. [x] Design the focus-loss handling point (e.g., centralized context setter or UI loop) that clears folder item focus when `FocusContext` leaves `SourceFolders` without altering selections.
3. [x] Implement the focus drop logic and adjust rendering to avoid showing a focused row when the browser lacks focus.
4. [x] Add/extend unit tests around folder focus and selection to cover focus loss scenarios.
5. [-] Manually verify folder navigation, selection, and drag/drop still behave as before when focus changes.

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
