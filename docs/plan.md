## Goal
- Add a progress bar popup for slow tasks (e.g., moving trashed items into the configured trash folder) within the egui UI.

## Proposed solutions
- Track long-running task progress in controller/UI state (message, counts, visibility) so the egui layer can render a responsive popup without blocking.
- Instrument the trash move workflow to emit progress updates while preserving current behaviour and error handling; keep the hook extensible for future slow tasks.
- Render a lightweight egui modal/progress overlay that matches existing styling and provides clear status plus optional cancel/dismiss affordances.

## Step-by-step plan
1. [-] Review existing egui overlay/popup patterns (status bar, waveform dialog, drag/hotkey overlays) to align progress UI with current structure and threading model.
2. [-] Add shared progress tracking state in the controller/UI (e.g., label, completed/total counters, visibility, cancellation support) with safe defaults.
3. [-] Wire the trash move path to publish progress updates and completion/errors into the shared state while keeping current file moves and collection updates intact.
4. [-] Implement the egui popup rendering (progress bar, status text, close/cancel handling) and ensure it does not block other UI interactions.
5. [-] Add tests for progress state transitions where feasible (e.g., controller unit tests around trash move instrumentation) and perform a manual sanity pass of the trash move flow.

## Code Style & Architecture Rules Reminder
- File and module structure
  - Keep files under 400 lines; split when necessary.
  - When functions require more than 5 arguments, group related values into a struct.
  - Each module must have one clear responsibility; split when responsibilities mix.
  - Do not use generic buckets like `misc.rs` or `util.rs`. Name modules by domain or purpose.
  - Name folders by feature first, not layer first.
- Functions
  - Keep functions under 30 lines; extract helpers as needed.
  - Each function must have a single clear responsibility.
  - Prefer many small structs over large ones.
- Documentation
  - All public objects, functions, structs, traits, and modules must be documented.
- Testing
  - All code should be well tested whenever feasible.
  - “Feasible” should be interpreted broadly: tests are expected in almost all cases.
  - Prefer small, focused unit tests that validate behaviour clearly.
  - Do not allow untested logic unless explicitly approved by the user.
