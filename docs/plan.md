Goal
- Add alt+drag waveform selection with visible overlay/handles, allow clearing via alt+click, and make spacebar/loop playback honor the marked region without blocking plain click-to-play; add a build step that bumps the app’s minor version on each build.

Proposed solutions
- Track waveform selection (normalized start/end) in the Rust state, updating it from alt+drag and handle drags while keeping existing click-to-play untouched.
- Render a selection overlay with draggable handles in the Slint UI, showing only when a region exists and clearing it on alt+click outside.
- Extend playback controls so spacebar and an added toolbar loop toggle play the marked range (or whole sample when none) with optional looping.
- Introduce a build script that auto-increments the Cargo package minor version on each build while keeping changes reversible and validated.

Step-by-step plan
1. [-] Map the current waveform rendering and input flow (Slint TouchArea, DropHandler events, AudioPlayer) to identify integration points for selection and looping.
2. [-] Add selection state (start/end, active flags) and interaction handling for alt+drag creation/resize plus alt+click clearing without disrupting normal clicks.
3. [-] Implement UI overlay rendering for the selected region with left/right resize handles and ensure it stays in sync with selection state.
4. [-] Wire playback to respect selection: spacebar plays marked span (or full track), click-to-play remains, and loop toggle plays the appropriate range repeatedly.
5. [-] Update AudioPlayer or related helpers to support bounded/looped playback with accurate timing for the playhead during selection playback.
6. [-] Add a build script that bumps the Cargo minor version on each build and include tests/checks to keep versioning predictable.
