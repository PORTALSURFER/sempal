## Goal
- Add a slice system that detects non-silent regions from the selected sample, shows blue resizable overlays in the waveform view, lets the user accept with Enter, and exports each slice to new audio files suffixed with `_slicexxx`.

## Proposed solutions
- Reuse the existing silence hysteresis thresholds to derive non-silent intervals, then map them to normalized waveform ranges for UI overlays.
- Introduce a dedicated slice model/state in the waveform controller and render overlays with selection-style handles, reusing selection drag logic where possible.
- Implement a new export path that writes each slice as a WAV clip with a `_slicexxx` suffix and updates the browser/library entries.

## Step-by-step plan
1. [-] Review current waveform selection/overlay and silence analysis code paths, then design a slice state struct (normalized bounds + edit state) stored with waveform UI/controller state.
2. [-] Add silence segmentation that returns non-silent intervals from loaded audio (using hysteresis thresholds), with normalization to waveform space and tests around interval detection.
3. [-] Implement waveform slice overlays (blue fill, selection-style handles) and drag interactions for resizing/moving slices without breaking selection behavior.
4. [-] Add user actions to compute slices and accept with Enter, then export each slice to new WAV files with `_slicexxx` names and update the browser/library entries; add tests for naming/export.
5. [-] Add UX polish and guardrails (clear/reset slices, error handling, selection conflicts), plus targeted tests for the slice workflow end-to-end.

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
