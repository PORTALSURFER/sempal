## Goal
- Add folder browser shortcuts so users can delete a folder with confirmation (`d`), rename a folder (`r`), create a new folder (`n`), and fuzzy search folders (`f`).

## Proposed solutions
- Extend folder browser controller APIs to handle delete/rename/create operations with confirmation, validation, and UI refresh.
- Add fuzzy search support that filters visible folders while keeping selection/focus in sync.
- Wire new actions into the existing folder hotkey handling and overlay, ensuring conflicts are avoided.
- Update UI feedback (warnings/status messages) and tests to cover the new workflows.

## Step-by-step plan
1. [-] Review current folder browser UI state, controller logic, and hotkey mappings to understand focus handling and available actions.
2. [-] Design and implement controller methods for delete (with warning), rename, and create operations that update folder models and refresh views safely.
3. [-] Implement fuzzy search for folders, including input handling, matching strategy, and integration with selection/focus behaviours.
4. [-] Connect `d`, `r`, `n`, and `f` shortcuts to the folder browser UI, add user prompts/status messaging, and ensure overlay/help text documents the actions.
5. [-] Add or update tests for folder operations and fuzzy search, and refresh any relevant docs or changelog entries.

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
