## Goal
- Ensure loop mode applies to mouse-triggered playback, add a global `L` hotkey to toggle looping, and surface a looping indicator bar that reflects the active loop span on the waveform.

## Proposed solutions
- Align mouse-based playback paths with the existing loop flag so selection plays looped when enabled, matching spacebar behaviour.
- Extend the hotkey registry with a global `L` binding that flips loop mode, updates UI state, and appears in the hotkey overlay.
- Render a top-of-waveform loop bar that tracks the active loop span (selection or full range), with visibility/opacity keyed off loop mode.
- Validate loop UX across input methods and selections to avoid regressions in playback or rendering.

## Step-by-step plan
1. [x] Trace pointer-based playback and loop state handling to identify where clicks bypass looping and confirm selection span inputs.
2. [x] Update seek/playback triggers to honor loop mode for mouse interactions while preserving selection boundaries and playhead updates.
3. [x] Add a global `L` hotkey that toggles looping, integrates with the hotkey overlay, and keeps controller/UI state consistent.
4. [x] Implement a looping indicator bar atop the waveform that follows the current loop span (selection or full range) and hides/fades when looping is off.
5. [~] Test loop behaviour via spacebar, mouse clicks, and the new hotkey; verify the loop bar responds to selection changes and loop toggles.

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
