## Goal
- Ensure sample renaming keeps the `.wav` extension out of the editable name and consistently preserves the original extension when applying renames.

## Proposed solutions
- Map the current sample rename flows (browser, collections, inline editors) to see where the editable name is seeded and how file paths/extensions are constructed when committing a rename.
- Change rename inputs to prefill only the basename (excluding `.wav`/extensions) and adjust commit logic to reattach the original extension automatically, guarding against accidental extension changes.
- Add focused tests that verify renames preserve the extension, handle filenames containing additional dots in the stem, and keep collection/export metadata aligned.

## Step-by-step plan
1. [-] Audit sample rename UI/controller paths to understand how default rename text and destination paths are built, including any extension handling.
2. [-] Update rename inputs and application logic to strip the `.wav` extension from user-facing text while reappending the original extension during file and metadata updates.
3. [-] Extend or add tests covering rename-with-extension behaviour (including names with extra dots) across browser and collection contexts.

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
