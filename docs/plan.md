## Goal
- Align waveform keyboard navigation so shift+space playback sets the next navigation anchor to that start position, navigation moves the green cursor rather than the live playhead, and shift+space uses the cursor when no prior play start exists.

## Proposed solutions
- Introduce a persistent waveform navigation cursor separate from the playhead, updated by keyboard navigation and mouse interactions, and rendered alongside the existing hover indicator.
- Anchor shift+space playback to the last navigation cursor when no explicit play start marker exists while keeping current playback/loop behaviour intact.
- Decouple navigation gestures from moving the active playhead; only seek when explicitly playing, otherwise move the cursor and keep the playhead untouched.
- Add targeted tests covering cursor movement, shift+space fallback, and viewport clamping to avoid regressions.

## Step-by-step plan
1. [x] Review waveform navigation and playback flow (`ui.rs` input handling, `waveform_navigation.rs`, `playback.rs`, rendering) to confirm current anchors, playhead updates, and cursor drawing.
2. [x] Add persistent waveform cursor state and rendering so keyboard navigation moves this cursor (not the playhead) while staying in sync with mouse hover/click affordances.
3. [x] Update navigation logic to operate on the cursor and record shift+space starts as the next navigation origin; default shift+space to the cursor when no previous play position exists without shifting the live playhead.
4. [x] Extend/adjust tests to cover cursor-driven navigation, shift+space fallback behaviour, and viewport clamping; verify no regressions in play/pause, selection, and zoom flows.
5. [-] Do a quick manual pass for waveform keyboard navigation, cursor rendering, and shift+space playback anchoring.

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
