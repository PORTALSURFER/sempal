## Goal
- Add random sample playback shortcuts with a 20-item history so users can jump to a random visible sample (Shift + R) and step backward through recent picks (Ctrl/Cmd + Shift + R) without disrupting existing navigation.

## Proposed solutions
- Keep random selection scoped to the current visible browser list, reuse existing focus/playback flows, and record each pick (source + path) into a bounded history that trims older entries.
- Add a backward navigation action that re-focuses and plays prior random picks, handling missing sources/files gracefully and resetting the cursor when new randoms are added.
- Surface both shortcuts in the hotkey registry/UI and cover behaviour with deterministic tests for history bounds, cursor movement, and edge cases.

## Step-by-step plan
1. [x] Review current sample browser selection and playback pathways (`controller/playback.rs`, `controller/wavs.rs`, hotkey handling in `controller/hotkeys.rs` and `ui.rs`) to find the right hook for selecting and focusing a random visible sample.
2. [x] Implement a controller method that picks a random visible browser row (skipping empty states), updates focus/selection/autoscroll as existing navigation does, and kicks off playback through the established audio path.
3. [x] Wire Shift + R into the hotkey registry with an appropriate scope/label, and connect it to the new random-play helper so the overlay and key handling trigger the behaviour.
4. [x] Update user-facing hotkey documentation and add targeted tests covering random selection edge cases and hotkey dispatch; keep randomness deterministic in tests.
5. [-] Manually sanity-check the new shortcut across focus contexts to confirm it doesn’t interfere with existing navigation or playback defaults.
6. [x] Add random playback history (capped at 20 entries) and a Ctrl/Cmd + Shift + R hotkey to step backward through prior random picks, aligning playback and focus even across source changes.

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
