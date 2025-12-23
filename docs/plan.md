## Goal
- Update the similarity map UX so panning only happens on right-click drag, and add a left-click drag “paint to play” interaction that scrubs/plays samples under the cursor as the user drags.

## Proposed solutions
- Add explicit mouse button handling in the map controller: gate pan logic to right button, leave left button free for playback gesture.
- Implement a drag-to-play handler that tracks cursor over map coordinates, finds nearest sample(s), and streams short previews as the cursor moves.
- Debounce/limit playback triggers (e.g., threshold movement distance or time) to avoid flooding audio playback while painting.
- Keep existing zoom/hover/selection behavior intact; ensure new gestures don’t conflict with current shortcuts.

## Step-by-step plan
1. [-] Inspect current similarity map input handling (controller + UI) to see how pan/zoom, hover, and playback are wired.
2. [-] Gate panning to right-click drag only and confirm zoom/selection still work as before.
3. [-] Add left-click drag “paint to play”: track cursor movement, find nearest samples under the path, and trigger audio playback.
4. [-] Add throttling/debouncing so playback events don’t spam while dragging; ensure audio stop/cleanup on release.
5. [-] Test the new interactions (right-click pan, left-click paint-to-play) and adjust any conflicting shortcuts or behaviors.

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
