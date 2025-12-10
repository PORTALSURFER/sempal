## Goal
- Improve waveform rendering quality so it remains stable, alias-free, and visually solid across all zoom levels (including extreme zoom-in), eliminating holes and sampling artifacts.

## Proposed solutions
- Tighten the sample-to-column aggregation in `waveform.rs` with oversampling/coverage-aware math that resists aliasing and avoids gaps when only a few samples map to many pixels.
- Revisit the view-to-sample slicing and texture sizing in the controller to prevent off-by-one gaps at high zoom and ensure consistent pixel density caps.
- Make mono vs. split-stereo rendering share the same anti-aliased pipeline so both modes stay consistent in thin/zoomed views.
- Expand automated checks around waveform sampling/downsampling to lock in the new behaviour and guard against regressions.

## Step-by-step plan
1. [x] Audit the existing renderer and controller flow (sampling, oversampling, view slicing, texture width caps) to pinpoint where holes and aliasing appear at various zoom levels.
2. [x] Refine sample-column aggregation to better handle tiny windows and partial coverage, keeping peaks visible while preventing alias artifacts across mono and split-stereo paths.
3. [x] Adjust view slicing and effective texture width calculations to remove off-by-one gaps at extreme zoom, keeping min/max view widths and GPU caps coherent.
4. [x] Add targeted unit tests for sampling/downsampling and controller view sizing (including high-zoom cases) to lock in the improved rendering behaviour.
5. [-] Manually exercise the waveform at multiple zoom levels and channel modes to confirm stability, visual solidity, and absence of aliasing or holes.

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
