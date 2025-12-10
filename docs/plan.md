## Goal
- Add the ability to move samples by drag-dropping them onto a folder in the sources panel.

## Proposed solutions
- Extend the drag/drop pipeline to carry folder targets alongside collections/triage and route drop handling accordingly without disrupting current behaviours.
- Update the folder browser UI to surface drop affordances, track hover state while dragging samples, and reject cross-source or invalid folder drops.
- Implement a controller path to move the underlying file into the target folder (rename + database/cache/collection sync) with clear status feedback and regression tests.

## Step-by-step plan
1. [x] Wire drag state and drop resolution to include folder targets next to collections/triage so existing drag flows keep working.
2. [x] Add folder browser hover/drop handling (highlighting, pointer tracking) that feeds the drag state and guards against invalid targets.
3. [x] Implement controller logic to move a sample payload into the hovered folder (rename on disk, DB update, cache/collection/export refresh) and connect it to the drop handler.
4. [x] Add/update tests covering folder move flows (success, duplicate/invalid target handling, collection sync) and refine user-facing status messages.
5. [~] Smoke-test drag/drop interactions across sample browser, collections, and folder browser to confirm no regressions (pending UI verification after fixes).

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
