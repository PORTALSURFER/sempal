## Goal
<<<<<<< HEAD
- Add a sub-5ms fade-in when starting playback via mouse click and when restarting mid-play from the stop location to avoid pops, keeping behaviour aligned with file length, zoom level, and scroll position.

## Proposed solutions
- Extend the existing `AudioPlayer` segment pipeline (`fade_in`, `EdgeFade`, `SEGMENT_FADE` in src/audio.rs) with an explicit start-fade path for manual seeks/restarts, clamping fade duration against the active span length.
- Thread the faded-start option through the egui controller paths that handle waveform clicks and play/pause/restart (`seek_to`, `set_playhead_and_seek`, looped playback) so normalized positions still map correctly regardless of zoom/scroll.
- Add focused tests around short clips, offset seeks, and stop-location restarts to ensure the fade envelope prevents pops without altering playhead/view synchronisation.

## Step-by-step plan
1. [x] Trace current playback entry points (waveform click seeks, play/pause toggles, stop-and-restart flows) to map how they hit `play_audio` and `AudioPlayer::start_with_span`.
2. [x] Update `AudioPlayer` to support a minimal start fade for arbitrary offsets (including restarts), clamping duration to the active span and keeping existing end fades intact.
3. [x] Wire controller/UI flows to opt into the faded start on mouse seeks and mid-play restarts while preserving selection, loop, zoom, and scroll behaviours.
4. [x] Add/extend tests covering fade length clamping, seek offsets, and short files to guard against popping regressions.
5. [-] Manually verify playhead visibility and waveform view synchronisation during seek/restart with the new fades.
=======
- Stop Windows system beeps when triggering hotkeys (e.g., `N` normalize and similar shortcuts) while keeping existing shortcut behaviour intact.

## Proposed solutions
- Trace the hotkey event flow in `src/egui_app/ui.rs` and related controller code to find where handled key events fall through and let Windows play the default error beep.
- Adjust key consumption so handled single-key and chord hotkeys are swallowed consistently (including pending chord roots) without disrupting text input or overlay visibility logic.
- Add regression coverage and manual checks to ensure hotkeys still dispatch their actions without beeps on Windows (normalize, delete, overlay toggle, focus changes).

## Step-by-step plan
1. [-] Reproduce the beep on Windows and map the current hotkey processing path (event collection, chord handling, focus checks) to pinpoint unconsumed key events.
2. [-] Update hotkey handling to consume matched keys—including chord roots where appropriate—while preserving existing focus rules and overlay behaviour.
3. [-] Add targeted tests or harness coverage for hotkey dispatch/consumption and manually verify key flows on Windows to confirm beeps are eliminated and commands still run.
>>>>>>> 389933a32f6429630772b268eeac03758db82f51

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
