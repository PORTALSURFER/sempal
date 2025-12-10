## Goal
- Add an `r` hotkey to the sample browser that triggers the same rename workflow currently available in the folder browser.

## Proposed solutions
- Mirror the folder browser rename flow: introduce a sample-browser-scoped hotkey that starts an inline rename prompt on the focused row, prefilled with the existing filename, and executes the existing sample rename controller path.
- Ensure focus handling, cancel/confirm shortcuts, and UI refresh mirror the folder browser experience so caches, collections, and waveforms stay in sync after rename.

## Step-by-step plan
1. [-] Add a Sample Browser rename hotkey entry (gesture `r`) and command that kicks off rename when a sample row is focused.
2. [-] Add controller/UI state to request a rename prompt for the focused sample, mirroring the folder browser approach (prefill current name, manage focus flags).
3. [-] Render the inline rename editor for the targeted sample browser row, wiring confirm/cancel paths to the existing rename handler and restoring focus/selection states.
4. [-] Extend tests and hotkey/help surfaces to cover the new rename shortcut and ensure browser state, caches, and collections remain consistent after rename.

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
