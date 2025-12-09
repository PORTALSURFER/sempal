## Goal
- Add robust audio regression tests by generating wav fixtures (varying sample rates, lengths, channels) with known beeps and validating playback math/process matches expectations.

## Proposed solutions
- Build a small fixture generator that synthesizes deterministic beeps at different sample rates, durations, and channel counts, and writes temporary wavs for tests.
- Add unit/integration tests that load these wavs through the existing decode/playback paths, asserting duration handling, seek/span math, and sample integrity.
- Cover edge cases: very short clips, long clips, mono vs. stereo, odd sample rates, and selections/offsets to ensure half-span cutoffs never occur.
- Keep tests hermetic by generating audio in-memory or in temp dirs, avoiding reliance on external assets.

## Step-by-step plan
1. [x] Design the fixture generator API (inputs: sample rate, duration, channels, tone frequency/pattern) and decide temp file handling for tests.
2. [x] Implement the generator and helpers to load/normalize generated wavs via existing decode utilities.
3. [x] Add tests for full-length playback math: duration detection, span boundaries, and playhead/progress across varied sample rates/durations/channels.
4. [x] Add selection/offset tests to confirm playback spans cover the full requested range (no half-span audio cutoff) for looped and non-looped cases.
5. [x] Validate tests are deterministic, performant, and isolated (clean up temp files) and document the new coverage.

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
