## Goal
- Add a “smooth” selection edit that rounds sharp edges in the chosen audio span to remove clicks from abrupt transitions (e.g., softening squared wave sections).

## Proposed solutions
- Introduce a new destructive selection edit that applies a short edge crossfade (e.g., raised-cosine) over the selection boundaries while preserving the interior loudness.
- Reuse the existing selection edit pipeline (buffer load → in-place processing → wav rewrite) with a dedicated smoothing helper and tests for small spans and multichannel audio.
- Extend the waveform UI actions to trigger the smooth edit with the same confirmation flow and status messaging used by other destructive operations.

## Step-by-step plan
1. [-] Audit current selection edit flow (controller, prompts, UI buttons) to spot integration points for a new smooth edit and any constraints on selection bounds.
2. [-] Design the smoothing algorithm (window shape, default fade duration, channel-aware frame math) and implement a pure helper with unit tests covering edge cases like tiny selections.
3. [-] Wire the smooth edit into the controller/destructive edit enums and status messages so the operation can process the selected frames and rewrite the wav.
4. [-] Add a waveform UI control to request the smooth edit, reusing the existing confirmation/yolo modes and hover help text.
5. [-] Verify via tests (existing selection edit tests plus new smoothing cases) and, if feasible, a quick manual pass to ensure waveform refresh/export paths remain intact.

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
