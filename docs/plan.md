## Goal
- Replace the current “smooth edges” selection tool with a click removal tool that processes an audio selection to remove single-sample discontinuities (clicks/pops) using DSP-only methods.

## Proposed solutions
- Implement a simple, high-quality interpolation repair (cubic by default with linear fallback) for short selections, keeping processing offline and selection-scoped.
- Add an optional higher-quality AR/LPC repair path for longer selections if interpolation is insufficient, while keeping the UI and settings minimal.
- Reuse existing selection processing pipeline (formerly “smooth edges”) to minimize new wiring and ensure consistent undo/redo behavior.

## Step-by-step plan
1. [-] Audit current “smooth edges” selection pipeline (UI entry point, controller action, audio processing function, undo/redo flow) to identify replacement points and required data inputs.
2. [-] Design the click-removal DSP algorithm and parameters: selection length limits, window size, edge handling, and chosen interpolation method(s).
3. [-] Implement a shared click-repair function in the audio processing layer (pure Rust), with unit tests covering 1-sample and small multi-sample selections and edge cases.
4. [-] Replace the “smooth edges” action wiring with “click removal” (UI label, controller command, analytics/telemetry if any), keeping selection behavior and undo/redo intact.
5. [-] Add integration tests for selection processing to validate artifacts removal and ensure no regression in selection handling.
6. [-] Update documentation/help text to describe the click removal tool and any limits (e.g., recommended selection size).

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
