## Goal
- Make hover highlighting for list items more intense and clearly visible, including when rows carry triage flags.

## Proposed solutions
- Increase hover contrast in the shared row styling (e.g., `row_hover_fill`) to ensure visibility against current backgrounds.
- Adjust list row rendering so hover treatments remain readable alongside triage markers and other overlays (selection, duplicate hover, drop targets).
- Validate hover behaviour in sample browser and collections panels with triage-flagged items, tweaking layering or strokes if needed.
- Add regression notes or lightweight tests to guard the updated hover visuals where feasible.

## Step-by-step plan
1. [x] Review current list row hover rendering and colour palette in `src/egui_app/ui/helpers.rs`, `src/egui_app/ui/style.rs`, and panels using triage markers to understand existing layering.
2. [x] Implement higher-contrast hover styling and ensure it stays visible on triage-flagged rows (e.g., overlay or stroke adjustments) without affecting selection or drag states.
3. [-] Manually verify hover visibility in the sample browser and collections with triage flags and during drag/drop interactions; fine-tune visuals based on findings.
4. [-] Add any feasible tests or QA notes to cover the new hover behaviour and run relevant checks.

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
