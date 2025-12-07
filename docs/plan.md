## Goal
- Add external drag-and-drop support so samples and selection clips can be dragged from Sempal into external targets (e.g., Windows Explorer or DAWs) without regressing existing in-app drag flows.

## Proposed solutions
- Leverage the eframe/winit backend or platform-specific drag/drop APIs (Windows shell drop, macOS NSPasteboard, Linux Xdnd) via a small abstraction to start external file drags from egui interactions.
- Ensure selection drags export clips to disk before initiating external drags, reusing existing selection export paths or temporary files when appropriate.
- Keep current internal drag-and-drop behaviour intact, gating external drag attempts on capability detection and clear user feedback.

## Step-by-step plan
1. [x] Review current drag/selection/export paths (`EguiApp` gesture handling, `EguiController` drag + export logic) to locate hook points for initiating external drags without breaking in-app targets.
2. [x] Evaluate feasible APIs for starting external drags from eframe/winit (platform-specific shims or a helper crate) and design a small cross-platform abstraction with graceful fallback when unsupported.
3. [x] Implement external drag initiation for samples and selection clips, ensuring files exist (exporting clips when needed), wiring through controller/UI state, and preserving existing internal drop behaviour.
4. [x] Update UI feedback and cancellation handling around external drags (cursor/icon or status text) so users understand when a drag is external versus internal.
5. [~] Add tests or targeted QA steps for selection export/regression and document manual verification for dragging into Explorer/DAWs.

## Code Style & Architecture Rules Reminder
- Keep files under 400 lines; split when necessary.
- When functions require more than 5 arguments, group related values into a struct.
- Each module must have one clear responsibility; split when responsibilities mix.
- Do not use generic buckets like misc.rs or util.rs. Name modules by domain or purpose.
- Name folders by feature first, not layer first.
- Keep functions under 30 lines; extract helpers as needed.
- Each function must have a single clear responsibility.
- Prefer many small structs over large ones.
- All public objects, functions, structs, traits, and modules must be documented.
- All code should be well tested whenever feasible.
- “Feasible” should be interpreted broadly: tests are expected in almost all cases.
- Prefer small, focused unit tests that validate behaviour clearly.
- Do not allow untested logic unless explicitly approved by the user.
