## Goal
- Render triage flags as a small marker on the far right of each item instead of coloring the entire row, keeping list visuals cleaner while preserving existing interactions.

## Proposed solutions
- Reuse the current triage palette but draw a compact right-edge indicator (solid rect or narrow bar) instead of a full-row background.
- Extend the list row rendering to support optional right-edge markers so selection/hover fills stay visible and consistent.
- Validate that drag, hover, and selection affordances remain clear with the new marker placement and sizing.

## Step-by-step plan
1. [x] Review current triage row rendering flow in the sample browser (e.g., `src/egui_app/ui/sample_browser_panel.rs`, `src/egui_app/ui/helpers.rs`, `src/egui_app/ui/style.rs`) to locate where the triage background is applied and where a right-edge marker can slot in.
2. [x] Implement the right-edge triage marker: adjust style helpers and row rendering so flags draw as a narrow indicator on the far right, keeping existing hover/selection styling intact.
3. [x] Verify visuals and interactions (selection, hover, drag targets) and run available checks/tests as feasible to ensure no regressions.

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
