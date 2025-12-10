## Goal
- Add a Shift + R hotkey that jumps to a random sample in the current browser view and immediately plays it, to speed up exploratory auditioning.

## Proposed solutions
- Introduce a controller helper that selects a random entry from the visible browser indices (respecting filters/search) and reuses existing selection and playback flows to load/play the chosen sample.
- Register a new hotkey action in `controller::hotkeys` with a clear label and Shift + R binding, scoped so it works while browsing/focused samples without disrupting text input.
- Surface the shortcut in the UI (hotkey overlay/usage docs) and ensure status feedback covers empty lists or missing sources.
- Add deterministic tests around the random-selection helper and hotkey dispatch to keep selection, focus, and playback behaviour stable.

## Step-by-step plan
1. [x] Review current sample browser selection and playback pathways (`controller/playback.rs`, `controller/wavs.rs`, hotkey handling in `controller/hotkeys.rs` and `ui.rs`) to find the right hook for selecting and focusing a random visible sample.
2. [x] Implement a controller method that picks a random visible browser row (skipping empty states), updates focus/selection/autoscroll as existing navigation does, and kicks off playback through the established audio path.
3. [x] Wire Shift + R into the hotkey registry with an appropriate scope/label, and connect it to the new random-play helper so the overlay and key handling trigger the behaviour.
4. [x] Update user-facing hotkey documentation and add targeted tests covering random selection edge cases and hotkey dispatch; keep randomness deterministic in tests.
5. [-] Manually sanity-check the new shortcut across focus contexts to confirm it doesn’t interfere with existing navigation or playback defaults.

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
