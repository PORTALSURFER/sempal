## Goal
- When a sample is removed from the active sample browser filter (e.g., retagging an untagged sample), automatically shift focus to the next available item in the filtered list instead of leaving focus on an item that is no longer visible.

## Proposed solutions
- Capture the currently focused visible row when tagging under a filter and, after retagging/rebuild, move focus to the next visible row (falling back to previous or first) if the original item no longer passes the filter.
- Adjust the sample browser rebuild/selection logic so filtered-out items are detected and the focus/selection indices are reassigned without breaking multi-selection or autoscroll behaviour.
- Add targeted tests around tagging inside filtered views (especially Untagged) to validate the new focus handoff and guard against regressions in search/random navigation paths.

## Step-by-step plan
1. [-] Trace how tagging updates browser state under filters (selection indices, autoscroll, rebuild paths) to pinpoint where to hook the focus reassignment.
2. [-] Implement focus handoff when a tagging action removes the focused sample from the active filter, choosing the next eligible visible row (with sensible fallbacks) while keeping multi-selection and status updates intact.
3. [-] Add/extend controller-level tests covering tagging within filtered views (e.g., Untagged → Keep/Trash) to confirm focus jumps to the next visible sample and does not regress other navigation modes.
4. [-] Verify behaviour via existing test suite or targeted checks and adjust any UI status/selection edge cases discovered during validation.

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
