## Goal
- Redesign the UI layout to remove double borders/frames so adjacent sections share a single border with tighter spacing and no inset stacking.

## Proposed solutions
- Audit the existing egui panels/cards to locate nested frames, duplicated strokes, and inset padding that create double outlines.
- Standardize a single border style (thickness/color) and apply it to panel wrappers and headers while stripping redundant inner frames.
- Adjust padding and spacing so sections butt against each other with only one separator line, preserving the current layout architecture.
- Validate the updated layout across primary views (sources, waveform/triage stack, collections) to ensure consistent single-border treatment.

## Step-by-step plan
1. [-] Survey current egui layout containers (top bar, side panels, central stack, cards) to identify sources of double borders and inset frames.
2. [-] Define and implement a shared single-border primitive and apply it to panel wrappers/headers, removing nested frame draws.
3. [-] Tighten spacing and padding between adjacent sections so only one border remains where panels meet; adjust card layouts as needed.
4. [-] Review key screens (sources list, triage columns, collections/drops, status footer) to confirm single-border consistency and update tests or visual captures if available.

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
