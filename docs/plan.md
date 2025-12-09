## Goal
- Make waveform texture generation smarter so zoomed views stay sharp without generating oversized textures, improving performance and avoiding GPU limits.

## Proposed solutions
- Render only the visible waveform slice at an appropriate resolution and reuse cached slices when panning or zooming.
- Introduce a multi-resolution pyramid or dynamic downsampling/upsampling strategy to balance detail and memory.
- Add adaptive texture sizing based on viewport and zoom, with safe caps and incremental updates.
- Defer rendering updates until needed (e.g., after zoom/scroll settles) to reduce churn.

## Step-by-step plan
1. [-] Analyze current waveform rendering pipeline and identify where decoded samples are transformed into textures and how zoom/view bounds are applied.
2. [-] Design a visible-region render strategy (slice-only or tiled) with sensible resolution targets and texture size caps; outline cache invalidation rules.
3. [-] Implement adaptive texture generation for the active view, including panning/zoom hooks, and ensure GPU limits are respected.
4. [-] Add guards for update frequency (debounce/throttle if needed) and verify playback/selection overlays still align with the rendered slice.
5. [-] Write/adjust tests or diagnostics to cover zoomed rendering correctness and performance ceilings.
6. [-] Manually verify zoom/pan interactions (wheel, arrows, chords) for clarity, no blurring, and no regressions.

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
