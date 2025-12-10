## Goal
- Make the waveform cursor/hover line visually distinct from the playhead, leave a dotted marker showing the last clicked play start position in its own colour, and add a Shift+Space hotkey to replay from that marker.

## Proposed solutions
- Adjust the waveform hover cursor stroke to use a separate palette colour from the playhead, ensuring both remain visible on dark backgrounds.
- Track the last play start position when the user clicks/seeks in the waveform, storing it in UI state so it can be rendered independently of the active playhead.
- Render a dotted vertical marker at the stored position with its own colour and stroke pattern, respecting current zoom/view bounds and clearing it when a file or selection changes context as needed.
- Bind Shift+Space to reuse the stored marker (or playhead fallback) and restart playback from that point, mirroring a click seek.

## Step-by-step plan
1. [x] Review waveform rendering and playhead/seek handling to locate where hover lines, playhead strokes, and seek clicks are processed.
2. [x] Choose distinct palette colours for the hover cursor and last-start marker, and update the hover line drawing to use the new cursor colour without affecting playhead styling.
3. [x] Add waveform UI/controller state to capture the last clicked play start, update it on seek/play interactions and relevant resets, and render a dotted vertical marker in the waveform view using the stored position.
4. [x] Add/update tests for marker state updates and view clamping, then manually verify the visual differences between hover cursor, playhead, and the dotted marker.
5. [x] Bind Shift+Space to replay from the stored marker (with playhead fallback) and cover it with a focused test.

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
