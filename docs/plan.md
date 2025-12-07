## Goal
- Add multi-selection to the sample browser so ctrl+click accumulates selections, shift+click/shift+arrow keys extend the selection range, pressing `x` toggles selection without losing focus, and focused vs selected items are visually distinct while preserving current navigation and autoplay behaviour.

## Proposed solutions
- Introduce a focused row separate from a multi-selected set, keeping autoplay/loaded behaviour tied to focus and maintaining existing tag/loaded bookkeeping.
- Track selection anchors and ranges across triage columns/filters so shift interactions extend correctly within the visible ordering while ctrl/`x` toggle membership.
- Update mouse and keyboard handling to respect the new model (plain click focuses, ctrl+click toggles, shift+click or shift+arrows grow ranges, `x` toggles the focused row) without breaking existing navigation or menus.
- Refresh sample browser rendering to show distinct focus vs selection styling alongside current loaded cues and drag/hover states.
- Adjust controller actions (tagging, autoplay, exports, drag) to use the correct focus/selection targets and add tests to lock the behaviours down.

## Step-by-step plan
1. [x] Review current sample browser focus/selection, navigation, and autoplay flows to define the desired focus vs selection semantics and identify dependencies.
2. [x] Extend UI/controller state to track a focused row, multi-selection set, and shift anchor across filtered triage columns while keeping loaded/autoscroll behaviour intact.
3. [x] Implement mouse interactions: plain click focuses and clears selection, ctrl+click toggles membership, and shift+click extends from the anchor to the clicked row within the visible list.
4. [x] Implement keyboard interactions: shift+up/down grow the selection range around the focused row, and `x` toggles selection on the focused row while leaving focus in place and preserving autoplay/navigation behaviour.
5. [x] Update browser rendering to differentiate focused vs selected vs loaded rows, ensuring drag targets, context menus, and tagging still work with the new state.
6. [x] Add/extend controller and UI tests to cover ctrl/shift click paths, shift+arrow growth, `x` toggling, and autoplay/focus consistency.

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
