## Goal
- Improve waveform rendering quality and selection accuracy, ensure all audio edit functions (fade, silence/mute, etc.) apply correctly to selections, and back everything with reliable automated tests.

## Proposed solutions
- Audit waveform rendering pipeline and selection math to find precision or aliasing issues; tighten sampling, interpolation, and drawing rules to keep visuals synced with audio data.
- Review selection application path for fades, silence/mute, and related edits to ensure they act only within selection bounds, honor channels, and maintain metadata/cache consistency.
- Introduce targeted property/fixture-based tests for waveform generation, selection math, and audio edit functions to guard against regressions.
- Add integration-level tests that validate end-to-end selection application and resulting waveform/image updates where feasible with existing helpers.

## Step-by-step plan
1. [-] Review waveform rendering and selection math (data preparation, downsampling, interpolation, draw) to identify where quality or accuracy is lost.
2. [-] Trace selection-bound audio edit flows (fade in/out, directional fades, silence/mute) to confirm they clamp to selection, respect channels, and update caches/exports.
3. [-] Implement rendering and selection accuracy fixes (e.g., improved downsampling/interpolation, bounds handling) without altering established UX.
4. [-] Harden audio edit functions to correctly apply to selections (including edge fades for silence/mute) and keep metadata/waveform artifacts in sync.
5. [-] Add comprehensive tests: unit/property tests for waveform math and selection operations, plus integration tests covering selection edits, rendering outputs, and cache/export updates.
6. [-] Perform smoke checks for selection interaction, visual fidelity, playback, and export to validate changes.

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
