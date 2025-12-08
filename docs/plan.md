## Goal
- Add an undo/redo system that captures user actions with a rolling history of the last 10 actions, and wire keyboard shortcuts (`u`/`Ctrl+Z` for undo, `U`/`Ctrl+Y` for redo) into the existing egui controller and hotkey overlay.

## Proposed solutions
- Introduce an action history manager (command pattern) within the controller to record reversible operations and maintain a bounded stack (10 items) with redo support.
- Define clear action types aligned with current controller responsibilities (sample selection edits, collection mutations, waveform/loop toggles, tagging/normalization) and ensure recording happens at the boundary where state mutates.
- Surface undo/redo through new hotkey actions and status/hotkey UI hints, ensuring focus-aware behaviour matches current hotkey handling.
- Cover the history manager with focused tests that validate stack limits, ordering, and state restoration across undo/redo sequences.

## Step-by-step plan
1. [-] Audit controller entry points and hotkey bindings to list undoable actions and decide granularity (selection toggles, collection edits, tag/flag changes, waveform toggles).
2. [-] Design and implement an action history component (stack + redo stack) limited to 10 entries, defining action structs with apply/revert hooks that work with `EguiController` state.
3. [-] Integrate history recording into existing controller methods for the chosen actions, ensuring state snapshots/deltas are captured at mutation boundaries without altering current behaviour when history is empty/full.
4. [-] Add undo/redo triggers (`u`/`Ctrl+Z` and `U`/`Ctrl+Y`) to the hotkey system and UI overlay, updating status messaging to confirm actions taken.
5. [-] Add unit/integration tests for the history manager and a representative set of actions to confirm correct undo/redo ordering, focus-aware hotkeys, and history truncation at 10 entries.

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
