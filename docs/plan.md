## Goal
- Ensure that after deleting a sample or folder in the UI, focus moves to the next available item to keep keyboard navigation smooth.

## Proposed solutions
- Determine the current focused row and next candidate before deletion, then reapply focus after the data/state rebuild.
- Add helper logic to handle focus fallback (next item, otherwise previous, otherwise clear) that works with filtered/visible lists.
- Cover both sample browser and folder browser flows, reusing existing selection/focus utilities where possible.
- Add regression tests around deletion + focus to lock behaviour in.

## Step-by-step plan
1. [x] Map current focus and selection handling for sample and folder deletion paths to find the correct hook points.
2. [x] Implement post-delete focus advancement for sample browser deletions, respecting filters and bounds.
3. [x] Implement post-delete focus advancement for folder deletions, selecting the next available row or sensible fallback.
4. [x] Add/extend controller tests to confirm focus jumps as expected after deletions.
5. [x] Run relevant checks/tests to ensure regressions are caught.

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
